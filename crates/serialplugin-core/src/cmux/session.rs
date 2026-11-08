//! CMUX session: deframe physical RX, route per DLCI, encode TX.

use crate::cmux::frame::{
    encode_frame, encode_uih, DecodedFrame, Deframer, DISC, DM, PF, SABM, UA, UI, UIH,
};
use crate::cmux::io::CmuxPhysicalIo;
use crate::events::SerialEvent;
use crate::hub::shared::Outbox;
use crate::hub::ExchangeWaiter;
use serial_codec::framing::Framer;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// Per-DLCI routing state (virtual channel).
pub struct DlciChannel {
    pub path: String,
    pub exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    watches: Mutex<DlciWatches>,
    /// The session's outbox: events are queued under `watches`, in production order,
    /// and delivered by the session once no lock is held.
    outbox: Arc<Outbox>,
}

/// The old watch (at most one) and the `watch_core` streams of one channel.
#[derive(Default)]
struct DlciWatches {
    old: Option<DlciWatchSlot>,
    core: Vec<(u64, DlciWatchSlot)>,
}

impl DlciWatches {
    fn each(&mut self, mut f: impl FnMut(&mut DlciWatchSlot)) {
        self.core.retain(|(_, w)| w.alive());
        self.old.iter_mut().for_each(&mut f);
        self.core.iter_mut().for_each(|(_, w)| f(w));
    }
}

struct DlciWatchSlot {
    channel: crate::events::EventSink,
    batch_timeout_ms: u64,
    flush_at: Instant,
    /// Native parser: each frame is one `Data` event, no batching.
    framer: Option<Box<dyn Framer>>,
    buffer: Vec<u8>,
    /// `watch_core` only: cleared once the subscriber is gone.
    alive: Option<Arc<AtomicBool>>,
}

impl DlciWatchSlot {
    fn new(
        channel: crate::events::EventSink,
        batch_timeout_ms: u64,
        framer: Option<Box<dyn Framer>>,
        alive: Option<Arc<AtomicBool>>,
    ) -> Self {
        Self {
            channel,
            batch_timeout_ms,
            flush_at: Instant::now(),
            framer,
            buffer: Vec::new(),
            alive,
        }
    }

    fn alive(&self) -> bool {
        self.alive.as_ref().is_none_or(|a| a.load(Ordering::SeqCst))
    }

    fn take_due(&mut self) -> bool {
        if self.flush_at.elapsed() >= Duration::from_millis(self.batch_timeout_ms) {
            self.flush_at = Instant::now();
            true
        } else {
            false
        }
    }

    /// Frames ready after `payload` (parser), or the batch once its timer is due.
    fn push(&mut self, payload: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        if let Some(f) = self.framer.as_mut() {
            f.push(payload, Instant::now(), &mut frames);
            return frames;
        }
        self.buffer.extend_from_slice(payload);
        if self.take_due() {
            frames.push(std::mem::take(&mut self.buffer));
        }
        frames
    }

    /// Time-based frames, or a batch whose timeout elapsed with no new data.
    fn poll(&mut self) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        if let Some(f) = self.framer.as_mut() {
            f.poll(Instant::now(), &mut frames);
        } else if self.take_due() && !self.buffer.is_empty() {
            frames.push(std::mem::take(&mut self.buffer));
        }
        frames
    }

    /// End of stream: the batch, then the parser remainder.
    fn finish(&mut self) -> Vec<Vec<u8>> {
        let batch = std::mem::take(&mut self.buffer);
        let rest = self.framer.as_mut().and_then(|f| f.flush());
        [batch, rest.unwrap_or_default()]
            .into_iter()
            .filter(|d| !d.is_empty())
            .collect()
    }
}

impl DlciChannel {
    fn new(path: String, outbox: Arc<Outbox>) -> Self {
        Self {
            path,
            exchange_waiter: Mutex::new(None),
            watches: Mutex::new(DlciWatches::default()),
            outbox,
        }
    }

    /// 4.0.0 API (feature `tauri`): [`Self::attach_watch_with`] for a Tauri channel, no parser.
    #[cfg(feature = "tauri")]
    pub fn attach_watch(&self, channel: tauri::ipc::Channel<SerialEvent>, batch_timeout_ms: u64) {
        self.attach_watch_with(crate::events::channel_sink(channel), batch_timeout_ms, None);
    }

    pub fn attach_watch_with(
        &self,
        channel: crate::events::EventSink,
        batch_timeout_ms: u64,
        framer: Option<Box<dyn Framer>>,
    ) {
        crate::sync_util::lock_or_recover(&self.watches).old =
            Some(DlciWatchSlot::new(channel, batch_timeout_ms, framer, None));
    }

    /// Stop the watch; a parser's buffered remainder is queued as a last `Data` event
    /// (after the frames already queued). The session delivers it on its next RX, tick
    /// or [`CmuxSession::clear_watch`] (which calls this and delivers).
    pub fn detach_watch(&self) {
        let mut guard = crate::sync_util::lock_or_recover(&self.watches);
        if let Some(mut slot) = guard.old.take() {
            // 4.x: the unflushed batch is dropped, the parser remainder kept.
            slot.buffer.clear();
            let rest = slot.finish();
            self.queue_frames(&slot.channel, rest);
        }
    }

    fn attach_core_watch(&self, id: u64, slot: DlciWatchSlot) {
        crate::sync_util::lock_or_recover(&self.watches)
            .core
            .push((id, slot));
    }

    /// Remove a `watch_core` stream; its batch and remainder are queued last.
    fn detach_core_watch(&self, id: u64) -> bool {
        let mut guard = crate::sync_util::lock_or_recover(&self.watches);
        let Some(i) = guard.core.iter().position(|(c, _)| *c == id) else {
            return false;
        };
        let (_, mut slot) = guard.core.remove(i);
        let rest = slot.finish();
        self.queue_frames(&slot.channel, rest);
        true
    }

    /// Physical link lost: queue the batch / parser remainder, then `Disconnect`.
    fn queue_disconnect(&self, reason: &str) {
        let mut guard = crate::sync_util::lock_or_recover(&self.watches);
        guard.each(|slot| {
            let frames = slot.finish();
            self.queue_frames(&slot.channel, frames);
            self.outbox.push([(
                slot.channel.clone(),
                SerialEvent::Disconnect {
                    path: self.path.clone(),
                    reason: reason.to_string(),
                },
            )]);
        });
    }

    /// Queue `Data` events; called with the `watches` lock held so the order is the
    /// production order whatever thread queues next.
    fn queue_frames(&self, channel: &crate::events::EventSink, frames: Vec<Vec<u8>>) {
        self.outbox.push(frames.into_iter().map(|data| {
            (
                channel.clone(),
                SerialEvent::Data {
                    path: self.path.clone(),
                    size: data.len(),
                    data,
                },
            )
        }));
    }

    fn push_stream(&self, payload: &[u8]) {
        crate::sync_util::lock_or_recover(&self.watches).each(|slot| {
            let frames = slot.push(payload);
            self.queue_frames(&slot.channel, frames);
        });
    }

    /// Parser timers and batches whose timeout elapsed with no new frame.
    fn tick(&self) {
        crate::sync_util::lock_or_recover(&self.watches).each(|slot| {
            let frames = slot.poll();
            self.queue_frames(&slot.channel, frames);
        });
    }
}

/// Parameter `index` (0-based) of an `AT+CMUX=` command
/// (27.007 +CMUX: mode, subset, port_speed, N1, T1, N2, T2, T3, k).
fn cmux_param(command: &str, index: usize) -> Option<u64> {
    let upper = command.to_ascii_uppercase();
    let at = upper.find("+CMUX=")?;
    upper[at + "+CMUX=".len()..]
        .split(',')
        .nth(index)?
        .trim()
        .parse()
        .ok()
}

/// N1 (maximum info bytes per frame) from an `AT+CMUX=` command: the 4th parameter.
/// 31 (the basic-option default) when absent or empty.
pub fn cmux_n1(command: &str) -> usize {
    cmux_param(command, 3).map_or(31, |n1| n1 as usize)
}

/// 27.010 link timers, from the `AT+CMUX=` parameters (see [`CmuxTimers::from_command`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CmuxTimers {
    /// T1, acknowledgement timer: how long to wait for UA before resending SABM.
    pub t1: Duration,
    /// N2, maximum number of retransmissions.
    pub n2: u32,
    /// T2, response timer for control-channel commands (CLD).
    pub t2: Duration,
}

impl Default for CmuxTimers {
    /// The 27.010 defaults: T1 100 ms, N2 3, T2 300 ms.
    fn default() -> Self {
        Self {
            t1: Duration::from_millis(100),
            n2: 3,
            t2: Duration::from_millis(300),
        }
    }
}

impl CmuxTimers {
    /// T1, N2 and T2 are the 5th–7th `+CMUX` parameters (T1/T2 in 10 ms units);
    /// absent, empty or out of the 27.007 range (T1 1..=255, N2 0..=100, T2 2..=255):
    /// the default. With neither T1 nor N2 in the command SABM is sent once (N2 = 0)
    /// and its UA gets the whole open timeout, as in 4.x.
    pub fn from_command(command: &str) -> Self {
        let d = Self::default();
        let tens = |i, min, default| {
            cmux_param(command, i)
                .filter(|v| (min..=255).contains(v))
                .map_or(default, |v| Duration::from_millis(v * 10))
        };
        let retransmit = cmux_param(command, 4).is_some() || cmux_param(command, 5).is_some();
        Self {
            t1: tens(4, 1, d.t1),
            n2: if retransmit {
                cmux_param(command, 5)
                    .filter(|v| *v <= 100)
                    .map_or(d.n2, |v| v as u32)
            } else {
                0
            },
            t2: tens(6, 2, d.t2),
        }
    }
}

// Control-channel message type octets (27.010 §5.4.6.3), C/R and EA bits included.
const CR: u8 = 0x02;
const PN_CMD: u8 = 0x83;
const PSC_CMD: u8 = 0x43;
const TEST_CMD: u8 = 0x23;
const MSC_CMD: u8 = 0xE3;
const RPN_CMD: u8 = 0x93;
const RLS_CMD: u8 = 0x53;
const FCON_CMD: u8 = 0xA3;
const FCOFF_CMD: u8 = 0x63;
const CLD_CMD: u8 = 0xC3;
const CLD_RSP: u8 = 0xC1;
/// Non-supported command response (§5.4.6.3.8).
const NSC_RSP: u8 = 0x11;
/// Commands answered by echoing them with C/R cleared.
const ECHOED: [u8; 9] = [
    PN_CMD, PSC_CMD, TEST_CMD, MSC_CMD, RPN_CMD, RLS_CMD, FCON_CMD, FCOFF_CMD, CLD_CMD,
];
/// MSC V.24 signals we send: EA | RTC | RTR | DV.
const V24_READY: u8 = 0x8D;
/// MSC V.24 flow control bit: the sender of the MSC cannot accept frames on that DLC.
const V24_FC: u8 = 0x02;

/// Flow control from the modem: FCoff stops every DLC, MSC with FC=1 one DLC.
/// Writes wait (on `CmuxSession::tx_cv`) until it ends or their write timeout.
#[derive(Default)]
struct TxGate {
    fc_off: bool,
    held_dlcis: HashSet<u8>,
}

/// The modem closed channels: their virtual paths, the reason, and whether it left
/// multiplexer mode altogether (DISC / DM on DLCI 0, CLD).
pub(crate) type ClosedHook = Box<dyn Fn(&[String], &str, bool) + Send + Sync>;

/// Active GSM 07.10 multiplexer on a physical serial port.
pub struct CmuxSession {
    physical_path: String,
    io: Arc<dyn CmuxPhysicalIo>,
    deframer: Mutex<Deframer>,
    channels: Mutex<HashMap<u8, Arc<DlciChannel>>>,
    /// SABM or DISC sent, waiting for UA (true) or DM (false).
    pending_open: Mutex<HashMap<u8, mpsc::Sender<bool>>>,
    /// CLD sent, waiting for its response.
    pending_cld: Mutex<Option<mpsc::Sender<()>>>,
    /// DLCs the modem closed (DISC / DM): writes fail until the DLCI is registered again.
    closed: Mutex<HashSet<u8>>,
    tx: Mutex<TxGate>,
    tx_cv: Condvar,
    /// Set when the session ended (closed down, or the modem left multiplexer mode).
    dead: Mutex<Option<String>>,
    on_closed: OnceLock<ClosedHook>,
    timers: CmuxTimers,
    outbox: Arc<Outbox>,
}

impl CmuxSession {
    /// Session with the basic-option default N1 (31, the 4.0.0 behaviour);
    /// [`Self::with_n1`] for the N1 given to `AT+CMUX`.
    pub fn new(physical_path: String, io: Arc<dyn CmuxPhysicalIo>) -> Arc<Self> {
        Self::with_n1(physical_path, io, serial_codec::cmux::TX_N1)
    }

    /// `n1`: the negotiated maximum frame size (`AT+CMUX` N1, see [`cmux_n1`]).
    /// Frames up to `max(n1, DEFAULT_MAX_RX_INFO)` info bytes are accepted. One SABM
    /// per open (4.x).
    pub fn with_n1(physical_path: String, io: Arc<dyn CmuxPhysicalIo>, n1: usize) -> Arc<Self> {
        let timers = CmuxTimers {
            n2: 0,
            ..CmuxTimers::default()
        };
        Self::with_timers(physical_path, io, n1, timers)
    }

    /// Session for the `AT+CMUX=` command that started it: N1 ([`cmux_n1`]) and the
    /// T1/N2/T2 timers ([`CmuxTimers::from_command`]).
    pub fn for_command(
        physical_path: String,
        io: Arc<dyn CmuxPhysicalIo>,
        command: &str,
    ) -> Arc<Self> {
        Self::with_timers(
            physical_path,
            io,
            cmux_n1(command),
            CmuxTimers::from_command(command),
        )
    }

    fn with_timers(
        physical_path: String,
        io: Arc<dyn CmuxPhysicalIo>,
        n1: usize,
        timers: CmuxTimers,
    ) -> Arc<Self> {
        Arc::new(Self {
            physical_path,
            io,
            deframer: Mutex::new(Deframer::with_max_info(
                n1.max(crate::cmux::frame::DEFAULT_MAX_RX_INFO),
            )),
            channels: Mutex::new(HashMap::new()),
            pending_open: Mutex::new(HashMap::new()),
            pending_cld: Mutex::new(None),
            closed: Mutex::new(HashSet::new()),
            tx: Mutex::default(),
            tx_cv: Condvar::new(),
            dead: Mutex::new(None),
            on_closed: OnceLock::new(),
            timers,
            outbox: Arc::default(),
        })
    }

    pub fn physical_path(&self) -> &str {
        &self.physical_path
    }

    /// Run `hook` whenever the modem closes channels (set once, by the manager).
    pub(crate) fn set_on_closed(&self, hook: ClosedHook) {
        let _ = self.on_closed.set(hook);
    }

    /// `Err` once the session ended.
    fn check_alive(&self) -> Result<(), String> {
        match crate::sync_util::lock_or_recover(&self.dead).as_ref() {
            Some(why) => Err(why.clone()),
            None => Ok(()),
        }
    }

    fn end(&self, why: &str) {
        crate::sync_util::lock_or_recover(&self.dead).get_or_insert_with(|| why.to_string());
        self.wake_writers();
    }

    /// Wake writers waiting on flow control (state changed: FCon, a channel closed …).
    fn wake_writers(&self) {
        drop(crate::sync_util::lock_or_recover(&self.tx));
        self.tx_cv.notify_all();
    }

    pub fn register_dlci(&self, dlci: u8, virtual_path: String) -> Arc<DlciChannel> {
        let ch = Arc::new(DlciChannel::new(virtual_path, self.outbox.clone()));
        crate::sync_util::lock_or_recover(&self.channels).insert(dlci, ch.clone());
        crate::sync_util::lock_or_recover(&self.closed).remove(&dlci);
        ch
    }

    pub fn unregister_dlci(&self, dlci: u8) {
        crate::sync_util::lock_or_recover(&self.closed).remove(&dlci);
        crate::sync_util::lock_or_recover(&self.tx)
            .held_dlcis
            .remove(&dlci);
        let removed = crate::sync_util::lock_or_recover(&self.channels)
            .remove(&dlci)
            .is_some();
        self.wake_writers();
        if removed {
            // Wait (up to T1) for the UA / DM: a CLD sent while the DISC is still in
            // progress is answered by the M26 only after ~0.5 s, past T2.
            let (tx, rx) = mpsc::channel();
            crate::sync_util::lock_or_recover(&self.pending_open).insert(dlci, tx);
            if self
                .io
                .write_all(&encode_frame(dlci, true, DISC | PF, &[]))
                .is_ok()
            {
                let _ = rx.recv_timeout(self.timers.t1);
            }
            crate::sync_util::lock_or_recover(&self.pending_open).remove(&dlci);
        }
    }

    pub fn has_dlci(&self, dlci: u8) -> bool {
        crate::sync_util::lock_or_recover(&self.channels).contains_key(&dlci)
    }

    /// Establish a DLC: send SABM and wait for the modem's UA (DM = refused). SABM is
    /// resent every T1, at most N2 times; the last try waits out `timeout` (at least T1).
    /// A data DLC gets an MSC (V.24 signals ready) once established.
    /// DLCI 0 (the control channel) must be opened first.
    pub fn open_dlci(&self, dlci: u8, timeout: Duration) -> Result<(), String> {
        self.check_alive()?;
        let (tx, rx) = mpsc::channel();
        crate::sync_util::lock_or_recover(&self.pending_open).insert(dlci, tx);
        let CmuxTimers { t1, n2, .. } = self.timers;
        let total = timeout.max(t1 * (n2 + 1));
        let sabm = encode_frame(dlci, true, SABM | PF, &[]);
        let start = Instant::now();
        let mut answer = Ok(None);
        for attempt in 0..=n2 {
            if let Err(e) = self.io.write_all(&sabm) {
                answer = Err(e);
                break;
            }
            let wait = if attempt == n2 {
                total.saturating_sub(start.elapsed())
            } else {
                t1
            };
            if let Ok(ua) = rx.recv_timeout(wait) {
                answer = Ok(Some(ua));
                break;
            }
        }
        crate::sync_util::lock_or_recover(&self.pending_open).remove(&dlci);
        match answer? {
            Some(true) => {
                if dlci != 0 {
                    let msc = [MSC_CMD, 0x05, (dlci << 2) | 0x03, V24_READY];
                    let _ = self.io.write_all(&encode_frame(0, true, UIH, &msc));
                }
                Ok(())
            }
            Some(false) => Err(format!("modem refused CMUX DLCI {dlci} (DM)")),
            None => Err(format!(
                "no UA for CMUX DLCI {dlci} within {} ms",
                total.as_millis()
            )),
        }
    }

    /// Ask the modem to leave multiplexer mode: CLD on the control channel, then wait
    /// up to T2 for its response. Channel writes fail from here on.
    pub fn close_down(&self) {
        self.end("CMUX multiplexer is closed");
        let (tx, rx) = mpsc::channel();
        *crate::sync_util::lock_or_recover(&self.pending_cld) = Some(tx);
        let cld = encode_frame(0, true, UIH, &[CLD_CMD, 0x01]);
        if self.io.write_all(&cld).is_ok() {
            let _ = rx.recv_timeout(self.timers.t2);
        }
        *crate::sync_util::lock_or_recover(&self.pending_cld) = None;
    }

    /// The modem closed `dlci` (DISC / DM; 0: the whole multiplexer): its watches get
    /// their remainder and `Disconnect`, an exchange in flight fails, held data is dropped.
    fn closed_by_modem(&self, dlci: u8) {
        let gone: Vec<(u8, Arc<DlciChannel>)> = {
            let mut channels = crate::sync_util::lock_or_recover(&self.channels);
            if dlci == 0 {
                channels.drain().collect()
            } else {
                channels
                    .remove(&dlci)
                    .map(|ch| (dlci, ch))
                    .into_iter()
                    .collect()
            }
        };
        if dlci == 0 {
            self.end("CMUX multiplexer closed by the modem");
        }
        let mut paths = Vec::new();
        for (d, ch) in gone {
            crate::sync_util::lock_or_recover(&self.closed).insert(d);
            let reason = format!("CMUX DLCI {d} closed by the modem");
            if let Some(waiter) = crate::sync_util::lock_or_recover(&ch.exchange_waiter).as_ref() {
                waiter.fail_with_reason(reason.clone());
            }
            ch.queue_disconnect(&reason);
            paths.push(ch.path.clone());
        }
        self.wake_writers();
        if let Some(hook) = self.on_closed.get() {
            let reason = if dlci == 0 {
                "CMUX multiplexer closed by the modem".to_string()
            } else {
                format!("CMUX DLCI {dlci} closed by the modem")
            };
            hook(&paths, &reason, dlci == 0);
        }
    }

    /// Batch-timeout flush for virtual-channel watches (called from the hub tick).
    pub fn tick(&self) {
        let channels: Vec<_> = crate::sync_util::lock_or_recover(&self.channels)
            .values()
            .cloned()
            .collect();
        for ch in channels {
            ch.tick();
        }
        self.outbox.deliver();
    }

    fn channel(&self, dlci: u8) -> Option<Arc<DlciChannel>> {
        crate::sync_util::lock_or_recover(&self.channels)
            .get(&dlci)
            .cloned()
    }

    /// Physical link lost: every channel watch gets its remainder, then `Disconnect`
    /// with the channel's virtual path.
    pub fn emit_disconnect(&self, reason: &str) {
        let channels: Vec<_> = crate::sync_util::lock_or_recover(&self.channels)
            .values()
            .cloned()
            .collect();
        for ch in channels {
            ch.queue_disconnect(reason);
        }
        self.outbox.deliver();
    }

    /// 4.0.0 API (feature `tauri`): [`Self::set_watch_with`] for a Tauri channel, no parser.
    #[cfg(feature = "tauri")]
    pub fn set_watch(
        &self,
        dlci: u8,
        channel: tauri::ipc::Channel<SerialEvent>,
        batch_timeout_ms: u64,
    ) {
        self.set_watch_with(
            dlci,
            crate::events::channel_sink(channel),
            batch_timeout_ms,
            None,
        );
    }

    pub fn set_watch_with(
        &self,
        dlci: u8,
        channel: crate::events::EventSink,
        batch_timeout_ms: u64,
        framer: Option<Box<dyn Framer>>,
    ) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            ch.attach_watch_with(channel, batch_timeout_ms, framer);
        }
    }

    /// Stop the channel watch; its parser remainder is the last `Data`. The sink runs
    /// here, so call with no manager lock held.
    pub fn clear_watch(&self, dlci: u8) {
        if let Some(ch) = self.channel(dlci) {
            ch.detach_watch();
        }
        self.outbox.deliver();
    }

    /// Add a `watch_core` stream on `dlci` (its own batch / parser). `false`: no channel.
    pub(crate) fn add_core_watch(
        &self,
        dlci: u8,
        id: u64,
        sink: crate::events::EventSink,
        alive: Arc<AtomicBool>,
        batch_timeout_ms: u64,
        framer: Option<Box<dyn Framer>>,
    ) -> bool {
        let Some(ch) = self.channel(dlci) else {
            return false;
        };
        ch.attach_core_watch(
            id,
            DlciWatchSlot::new(sink, batch_timeout_ms, framer, Some(alive)),
        );
        true
    }

    /// Remove a `watch_core` stream; its remainder is delivered here (no lock held).
    pub(crate) fn remove_core_watch(&self, dlci: u8, id: u64) -> bool {
        let removed = self
            .channel(dlci)
            .is_some_and(|ch| ch.detach_core_watch(id));
        self.outbox.deliver();
        removed
    }

    pub fn set_exchange_waiter(&self, dlci: u8, waiter: Arc<ExchangeWaiter>) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            *crate::sync_util::lock_or_recover(&ch.exchange_waiter) = Some(waiter);
        }
    }

    pub fn clear_exchange_waiter(&self, dlci: u8) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            *crate::sync_util::lock_or_recover(&ch.exchange_waiter) = None;
        }
    }

    /// Fail the in-flight exchange waiter on `dlci` (used by `cancel_exchange`).
    pub fn cancel_active_exchange(&self, dlci: u8) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            if let Some(waiter) = crate::sync_util::lock_or_recover(&ch.exchange_waiter).as_ref() {
                waiter.fail_with_reason("exchange cancelled".into());
            }
        }
    }

    /// Send `payload` on `dlci` as UIH frames. While the modem holds flow control off
    /// (FCoff, or MSC with FC=1 for this DLC) the write waits for it to end, at most the
    /// write timeout of the frames, then fails; nothing is queued to go out later.
    pub fn send_uih(&self, dlci: u8, payload: &[u8]) -> Result<usize, String> {
        let frame = encode_uih(dlci, payload);
        let deadline = Instant::now() + self.io.write_timeout(frame.len());
        // Written under the gate lock: no data frame follows our FCoff response.
        let mut gate = crate::sync_util::lock_or_recover(&self.tx);
        let mut waited = false;
        loop {
            self.check_alive()?;
            if crate::sync_util::lock_or_recover(&self.closed).contains(&dlci) {
                return Err(format!("CMUX DLCI {dlci} closed by the modem"));
            }
            if waited && !self.has_dlci(dlci) {
                return Err(format!("CMUX DLCI {dlci} is closed"));
            }
            let held = if gate.fc_off {
                "CMUX flow control off (FCoff)".to_string()
            } else if gate.held_dlcis.contains(&dlci) {
                format!("CMUX DLCI {dlci} flow control off (MSC FC)")
            } else {
                break;
            };
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(held);
            }
            gate = self
                .tx_cv
                .wait_timeout(gate, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
            waited = true;
        }
        self.io.write_all(&frame)?;
        drop(gate);
        Ok(payload.len())
    }

    /// Feed raw bytes from the physical RX hub thread.
    pub fn feed_physical_rx(&self, chunk: &[u8]) {
        let frames = crate::sync_util::lock_or_recover(&self.deframer).feed(chunk);
        for frame in frames {
            self.dispatch_frame(frame);
        }
        self.outbox.deliver();
    }

    fn dispatch_frame(&self, frame: DecodedFrame) {
        match frame.kind {
            UA | DM => {
                if let Some(tx) =
                    crate::sync_util::lock_or_recover(&self.pending_open).get(&frame.dlci)
                {
                    let _ = tx.send(frame.kind == UA);
                } else if frame.kind == DM {
                    // DM on an established DLC: the modem has closed it.
                    self.closed_by_modem(frame.dlci);
                }
                return;
            }
            DISC => {
                // UA when the DLC is open (DLCI 0: the whole multiplexer), else DM.
                let open = frame.dlci == 0 || self.has_dlci(frame.dlci);
                let reply = if open { UA } else { DM };
                let _ = self
                    .io
                    .write_all(&encode_frame(frame.dlci, false, reply | PF, &[]));
                if open {
                    self.closed_by_modem(frame.dlci);
                }
                return;
            }
            UIH | UI => {}
            // SABM from the modem: we are the initiator, nothing to accept.
            _ => return,
        }
        if frame.dlci == 0 {
            self.handle_control(&frame.payload);
            return;
        }
        let Some(ch) = self.channel(frame.dlci) else {
            return;
        };
        let waiter = crate::sync_util::lock_or_recover(&ch.exchange_waiter).clone();
        if let Some(waiter) = waiter {
            waiter.push_bytes(&frame.payload);
            return;
        }
        ch.push_stream(&frame.payload);
    }

    /// Control-channel message from the modem. The commands we know (PN, PSC, TEST,
    /// MSC, RPN, RLS, FCon/FCoff, CLD) are acknowledged by echoing them back with the
    /// type octet's C/R bit cleared, as 27.010 §5.4.6 requires; any other command gets
    /// NSC (§5.4.6.3.8). FCoff holds UIH data on every DLC until FCon, an MSC with FC
    /// set holds that DLC until an MSC without it; CLD from the modem closes every
    /// channel. Of the responses only CLD's is waited for.
    fn handle_control(&self, payload: &[u8]) {
        let Some(&type_octet) = payload.first() else {
            return;
        };
        if type_octet & CR == 0 {
            if type_octet == CLD_RSP {
                if let Some(tx) = crate::sync_util::lock_or_recover(&self.pending_cld).as_ref() {
                    let _ = tx.send(());
                }
            }
            return;
        }
        // Gate first: no data frame may follow the FCoff response.
        if type_octet == FCOFF_CMD {
            crate::sync_util::lock_or_recover(&self.tx).fc_off = true;
        }
        if let (MSC_CMD, Some(&address), Some(&v24)) = (type_octet, payload.get(2), payload.get(3))
        {
            let dlci = address >> 2;
            let mut gate = crate::sync_util::lock_or_recover(&self.tx);
            if v24 & V24_FC != 0 {
                gate.held_dlcis.insert(dlci);
            } else {
                gate.held_dlcis.remove(&dlci);
            }
            drop(gate);
            self.tx_cv.notify_all();
        }
        let reply = if ECHOED.contains(&type_octet) {
            let mut reply = payload.to_vec();
            reply[0] &= !CR;
            reply
        } else {
            vec![NSC_RSP, 0x03, type_octet]
        };
        let _ = self.io.write_all(&encode_frame(0, true, UIH, &reply));
        match type_octet {
            FCON_CMD => {
                crate::sync_util::lock_or_recover(&self.tx).fc_off = false;
                self.tx_cv.notify_all();
            }
            CLD_CMD => self.closed_by_modem(0),
            _ => {}
        }
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
    use crate::exchange::options::ResolvedExchangeOptions;
    use crate::hub::ExchangeWaiter;
    use std::sync::atomic::AtomicBool;

    fn test_options() -> ResolvedExchangeOptions {
        ResolvedExchangeOptions {
            timeout_ms: 3000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::None,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        }
    }

    #[cfg(unix)]
    #[test]
    fn feed_physical_rx_routes_to_exchange_waiter() {
        use crate::cmux::SerialPortIo;
        use crate::sync_util::pty_pair_locked;
        use serialport::SerialPort;
        let (_pty, port, _master) = pty_pair_locked();
        let session = CmuxSession::with_n1(
            "pty".into(),
            Arc::new(SerialPortIo(Arc::new(Mutex::new(
                Box::new(port) as Box<dyn SerialPort>
            )))),
            31,
        );
        session.register_dlci(2, "pty#dlci=2".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let waiter = ExchangeWaiter::new(test_options(), cancel);
        session.set_exchange_waiter(2, waiter.clone());
        let payload = b"AT\r\r\nOK\r\n";
        let frame = encode_uih(2, payload);
        session.feed_physical_rx(&frame);
        let (raw, matched) = waiter.wait(1000).expect("complete");
        assert!(String::from_utf8_lossy(&raw).contains("OK"));
        assert!(matches!(matched, crate::at::parse::ExchangeMatch::Ok));
    }

    #[test]
    fn cancel_active_exchange_fails_waiter_quickly() {
        use crate::cmux::io::CmuxPhysicalIo;
        struct NoopIo;
        impl CmuxPhysicalIo for NoopIo {
            fn write_all(&self, _data: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        let session = CmuxSession::with_n1("mock".into(), Arc::new(NoopIo), 31);
        session.register_dlci(2, "mock#dlci=2".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let waiter = ExchangeWaiter::new(test_options(), cancel);
        session.set_exchange_waiter(2, waiter.clone());
        let waiter_bg = waiter.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_bg = done.clone();
        std::thread::spawn(move || {
            let _ = waiter_bg.wait(5000);
            done_bg.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        let start = std::time::Instant::now();
        session.cancel_active_exchange(2);
        while !done.load(std::sync::atomic::Ordering::SeqCst)
            && start.elapsed() < std::time::Duration::from_millis(500)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(done.load(std::sync::atomic::Ordering::SeqCst));
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }

    #[test]
    fn dlci_watch_parser_emits_one_event_per_frame() {
        use crate::cmux::io::CmuxPhysicalIo;
        use serial_codec::framing::FramerConfig;
        struct NoopIo;
        impl CmuxPhysicalIo for NoopIo {
            fn write_all(&self, _data: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        let session = CmuxSession::with_n1("mock".into(), Arc::new(NoopIo), 31);
        session.register_dlci(2, "mock#dlci=2".into());
        let frames = Arc::new(Mutex::new(Vec::new()));
        let sink = frames.clone();
        let channel: crate::events::EventSink = Arc::new(move |event: SerialEvent| {
            if let SerialEvent::Data { data, .. } = event {
                sink.lock().unwrap().push(data);
            }
            true
        });
        let cfg: FramerConfig =
            serde_json::from_value(serde_json::json!({"type": "readline"})).unwrap();
        session.set_watch_with(2, channel, 10_000, Some(cfg.build().unwrap()));
        session.feed_physical_rx(&encode_uih(2, b"one\ntw"));
        session.feed_physical_rx(&encode_uih(2, b"o\nthr"));
        assert_eq!(
            *frames.lock().unwrap(),
            vec![b"one".to_vec(), b"two".to_vec()]
        );
        session.clear_watch(2);
        assert_eq!(frames.lock().unwrap().last().unwrap(), b"thr");
    }

    struct NoopIo;
    impl crate::cmux::io::CmuxPhysicalIo for NoopIo {
        fn write_all(&self, _data: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    /// The remainder sink ran under the session's `channels` lock (and the manager's
    /// `mux` lock), so a sink calling back into the session deadlocked.
    #[test]
    fn clear_watch_remainder_sink_may_call_back_into_session() {
        use serial_codec::framing::FramerConfig;
        let session = CmuxSession::with_n1("mock".into(), Arc::new(NoopIo), 31);
        session.register_dlci(2, "mock#dlci=2".into());
        let weak = Arc::downgrade(&session);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let channel: crate::events::EventSink = Arc::new(move |event: SerialEvent| {
            if let SerialEvent::Data { data, .. } = event {
                let known = weak.upgrade().is_some_and(|s| s.has_dlci(2));
                let _ = tx.lock().unwrap().send((data, known));
            }
            true
        });
        let cfg: FramerConfig =
            serde_json::from_value(serde_json::json!({"type": "readline"})).unwrap();
        session.set_watch_with(2, channel, 10_000, Some(cfg.build().unwrap()));
        session.feed_physical_rx(&encode_uih(2, b"partial"));
        let bg = session.clone();
        std::thread::spawn(move || bg.clear_watch(2));
        let (data, known) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("clear_watch deadlocked: sink ran under the channels lock");
        assert_eq!(data, b"partial");
        assert!(known);
    }

    #[test]
    fn n1_is_the_fourth_cmux_parameter() {
        assert_eq!(cmux_n1("AT+CMUX=0,0,5,2048"), 2048);
        assert_eq!(cmux_n1("at+cmux=0,0,5,127,10,3,30,10,2"), 127);
        assert_eq!(cmux_n1("AT+CMUX=0"), 31);
        assert_eq!(cmux_n1("AT+CMUX=0,0,,"), 31);
        assert_eq!(cmux_n1("AT+CMUX=0,0,5, 1600 ,10"), 1600);
        assert_eq!(cmux_n1("AT"), 31);
    }

    /// Audit p4: the session always used the 1509-byte default deframer, so with
    /// `AT+CMUX=…,N1=2048` every larger modem frame was dropped as corrupt.
    #[test]
    fn session_accepts_frames_up_to_negotiated_n1() {
        let n1 = cmux_n1("AT+CMUX=0,0,5,2048");
        let session = CmuxSession::with_n1("mock".into(), Arc::new(NoopIo), n1);
        session.register_dlci(1, "mock#dlci=1".into());
        let got = Arc::new(Mutex::new(0usize));
        let sink = got.clone();
        let channel: crate::events::EventSink = Arc::new(move |event: SerialEvent| {
            if let SerialEvent::Data { size, .. } = event {
                *sink.lock().unwrap() += size;
            }
            true
        });
        session.set_watch_with(1, channel, 0, None);
        let frame = encode_frame(1, true, UIH, &[b'A'; 2000]);
        session.feed_physical_rx(&frame);
        assert_eq!(*got.lock().unwrap(), 2000, "2000-byte frame dropped");
    }

    // ------------------------------------------------ fake 27.010 modem (item 11)

    type Log = Arc<Mutex<Vec<DecodedFrame>>>;
    type HookLog = Arc<Mutex<Vec<(Vec<String>, bool)>>>;
    type Events = Arc<Mutex<Vec<SerialEvent>>>;

    struct FakeIo(Mutex<mpsc::Sender<Vec<u8>>>);
    impl crate::cmux::io::CmuxPhysicalIo for FakeIo {
        fn write_all(&self, data: &[u8]) -> Result<(), String> {
            let _ = self.0.lock().unwrap().send(data.to_vec());
            Ok(())
        }
        /// Short, so FCoff tests time out quickly.
        fn write_timeout(&self, _len: usize) -> Duration {
            Duration::from_millis(300)
        }
    }

    /// Fake modem on its own thread: logs every frame the session writes, and feeds
    /// `reply(frame)` back as physical RX.
    fn fake_modem(
        command: &str,
        mut reply: impl FnMut(&DecodedFrame) -> Vec<u8> + Send + 'static,
    ) -> (Arc<CmuxSession>, Log) {
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let session =
            CmuxSession::for_command("m".into(), Arc::new(FakeIo(Mutex::new(tx))), command);
        let weak = Arc::downgrade(&session);
        let log: Log = Arc::default();
        let seen = log.clone();
        std::thread::spawn(move || {
            let mut deframer = Deframer::default();
            for bytes in rx {
                for f in deframer.feed(&bytes) {
                    let out = reply(&f);
                    seen.lock().unwrap().push(f);
                    if let (false, Some(s)) = (out.is_empty(), weak.upgrade()) {
                        s.feed_physical_rx(&out);
                    }
                }
            }
        });
        (session, log)
    }

    fn ua_to_sabm(f: &DecodedFrame) -> Vec<u8> {
        if f.kind == SABM {
            encode_frame(f.dlci, true, UA | PF, &[])
        } else {
            Vec::new()
        }
    }

    fn wait_log(
        log: &Log,
        what: &str,
        pred: impl Fn(&[DecodedFrame]) -> bool,
    ) -> Vec<DecodedFrame> {
        let start = Instant::now();
        loop {
            let frames = log.lock().unwrap().clone();
            if pred(&frames) {
                return frames;
            }
            assert!(
                start.elapsed() < Duration::from_secs(3),
                "timed out: {what}: {frames:?}"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    fn count(frames: &[DecodedFrame], pred: impl Fn(&DecodedFrame) -> bool) -> usize {
        frames.iter().filter(|f| pred(f)).count()
    }

    fn event_sink() -> (crate::events::EventSink, Events) {
        let events: Events = Arc::default();
        let sink = events.clone();
        let channel: crate::events::EventSink = Arc::new(move |event: SerialEvent| {
            sink.lock().unwrap().push(event);
            true
        });
        (channel, events)
    }

    fn disconnects(events: &Events) -> Vec<String> {
        events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Disconnect { path, .. } => Some(path.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn cmux_timers_from_command_parameters() {
        let ms = Duration::from_millis;
        let default = CmuxTimers::default();
        assert_eq!((default.t1, default.n2, default.t2), (ms(100), 3, ms(300)));
        // No T1 / N2 in the command: one SABM, as in 4.x.
        let single = CmuxTimers { n2: 0, ..default };
        assert_eq!(CmuxTimers::from_command("AT+CMUX=0"), single);
        assert_eq!(CmuxTimers::from_command("AT"), single);
        let t = CmuxTimers::from_command("AT+CMUX=0,0,5,31,10,2");
        assert_eq!((t.t1, t.n2, t.t2), (ms(100), 2, ms(300)));
        let t = CmuxTimers::from_command("at+cmux=0,0,5,127,25, 5 ,40,10,2");
        assert_eq!((t.t1, t.n2, t.t2), (ms(250), 5, ms(400)));
        // Out of the 27.007 ranges (T1 1..=255, N2 0..=100, T2 2..=255): the default.
        let t = CmuxTimers::from_command("AT+CMUX=0,0,5,31,0,101,1");
        assert_eq!(t, default);
        assert_eq!(CmuxTimers::from_command("AT+CMUX=0,0,5,31,,0").n2, 0);
    }

    #[test]
    fn msc_follows_ua_on_each_data_dlci() {
        let (s, log) = fake_modem("AT+CMUX=0", ua_to_sabm);
        s.open_dlci(0, Duration::from_secs(1)).unwrap();
        s.register_dlci(1, "m#dlci=1".into());
        s.open_dlci(1, Duration::from_secs(1)).unwrap();
        let ctrl = |f: &DecodedFrame| f.dlci == 0 && f.kind == UIH;
        let frames = wait_log(&log, "MSC", |f| count(f, ctrl) >= 1);
        let msc: Vec<_> = frames.iter().filter(|f| ctrl(f)).collect();
        // MSC command, length 2, DLCI 1 address octet, V.24: EA|RTC|RTR|DV.
        assert_eq!(msc[0].payload, vec![0xE3, 0x05, 0x07, 0x8D]);
        assert!(msc[0].cr, "MSC is our command");
        let sabm1 = frames
            .iter()
            .position(|f| f.dlci == 1 && f.kind == SABM)
            .unwrap();
        let msc_at = frames.iter().position(ctrl).unwrap();
        assert!(sabm1 < msc_at, "MSC after the DLC is established");

        // The modem's MSC command is answered (C/R cleared); its response to ours is not.
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xE3, 0x05, 0x07, 0x8D]));
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xE1, 0x05, 0x07, 0x8D]));
        wait_log(&log, "MSC response", |f| count(f, ctrl) >= 2);
        std::thread::sleep(Duration::from_millis(50));
        let frames = log.lock().unwrap().clone();
        let ctrl_payloads: Vec<_> = frames
            .iter()
            .filter(|f| ctrl(f))
            .map(|f| f.payload.clone())
            .collect();
        assert_eq!(
            ctrl_payloads,
            vec![vec![0xE3, 0x05, 0x07, 0x8D], vec![0xE1, 0x05, 0x07, 0x8D]],
            "one MSC of ours (none for DLCI 0), one response to the modem's"
        );
    }

    #[test]
    fn sabm_retried_n2_times_with_t1_then_error() {
        // T1 = 2 (20 ms), N2 = 2: three SABMs in all, then the error.
        let (s, log) = fake_modem("AT+CMUX=0,0,5,31,2,2", |_| Vec::new());
        s.register_dlci(1, "m#dlci=1".into());
        let start = Instant::now();
        let err = s.open_dlci(1, Duration::from_millis(1)).unwrap_err();
        let took = start.elapsed();
        assert!(err.starts_with("no UA for CMUX DLCI 1 within"), "{err}");
        assert!(took >= Duration::from_millis(60), "{took:?}");
        assert!(took < Duration::from_secs(1), "{took:?}");
        let sabm = |f: &DecodedFrame| f.dlci == 1 && f.kind == SABM;
        wait_log(&log, "3 SABM", |f| count(f, sabm) == 3);
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(
            count(&log.lock().unwrap(), sabm),
            3,
            "no retransmission after the error"
        );

        // A modem that only answers the second SABM: open succeeds after one retry.
        let mut seen = 0;
        let (s, log) = fake_modem("AT+CMUX=0,0,5,31,2,2", move |f| {
            if f.kind != SABM {
                return Vec::new();
            }
            seen += 1;
            if seen == 2 {
                encode_frame(f.dlci, true, UA | PF, &[])
            } else {
                Vec::new()
            }
        });
        s.register_dlci(1, "m#dlci=1".into());
        s.open_dlci(1, Duration::from_millis(1)).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        assert_eq!(count(&log.lock().unwrap(), sabm), 2);

        // Without T1/N2 in the command the 4.x wait (the caller's timeout) still holds.
        let (s, _log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        let start = Instant::now();
        let err = s.open_dlci(0, Duration::from_millis(700)).unwrap_err();
        assert_eq!(err, "no UA for CMUX DLCI 0 within 700 ms");
        assert!(start.elapsed() >= Duration::from_millis(700));
    }

    #[test]
    fn modem_disc_closes_only_that_channel() {
        let (s, log) = fake_modem("AT+CMUX=0", ua_to_sabm);
        s.register_dlci(1, "m#dlci=1".into());
        s.register_dlci(2, "m#dlci=2".into());
        let (old1, ev_old1) = event_sink();
        let (core1, ev_core1) = event_sink();
        let (old2, ev_old2) = event_sink();
        s.set_watch_with(1, old1, 0, None);
        assert!(s.add_core_watch(1, 7, core1, Arc::new(AtomicBool::new(true)), 0, None));
        s.set_watch_with(2, old2, 0, None);

        s.feed_physical_rx(&encode_frame(1, true, DISC | PF, &[]));
        let frames = wait_log(&log, "UA to DISC", |f| {
            count(f, |f| f.dlci == 1 && f.kind == UA) == 1
        });
        let ua = frames.iter().find(|f| f.dlci == 1 && f.kind == UA).unwrap();
        assert!(!ua.cr, "UA is our response");
        assert_eq!(disconnects(&ev_old1), vec!["m#dlci=1".to_string()]);
        assert_eq!(disconnects(&ev_core1), vec!["m#dlci=1".to_string()]);
        assert!(disconnects(&ev_old2).is_empty());
        assert!(!s.has_dlci(1));
        assert!(s.has_dlci(2));
        assert_eq!(
            s.send_uih(1, b"x").unwrap_err(),
            "CMUX DLCI 1 closed by the modem"
        );
        assert_eq!(s.send_uih(2, b"x"), Ok(1));
        // Data for the closed channel goes nowhere.
        s.feed_physical_rx(&encode_uih(1, b"late"));
        assert!(!ev_old1
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e, SerialEvent::Data { .. })));

        // DM on an open DLC: closed too, but no UA.
        s.feed_physical_rx(&encode_frame(2, true, DM | PF, &[]));
        assert_eq!(disconnects(&ev_old2), vec!["m#dlci=2".to_string()]);
        assert!(!s.has_dlci(2));
        // DISC on a DLC that is not open is answered with DM.
        s.feed_physical_rx(&encode_frame(5, true, DISC | PF, &[]));
        let frames = wait_log(&log, "DM to DISC", |f| {
            count(f, |f| f.dlci == 5 && f.kind == DM) == 1
        });
        assert_eq!(count(&frames, |f| f.kind == UA), 1, "{frames:?}");

        // Re-opening the channel clears the closed state.
        s.register_dlci(1, "m#dlci=1".into());
        s.open_dlci(1, Duration::from_secs(1)).unwrap();
        assert_eq!(s.send_uih(1, b"x"), Ok(1));
    }

    #[test]
    fn fcoff_blocks_writes_until_fcon() {
        let (s, log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        s.register_dlci(1, "m#dlci=1".into());
        let data = |f: &DecodedFrame| f.dlci != 0 && f.kind == UIH;
        let ctrl = |f: &DecodedFrame, p: &[u8]| f.dlci == 0 && f.kind == UIH && f.payload == p;

        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0x63, 0x01])); // FCoff
        wait_log(&log, "FCoff response", |f| {
            count(f, |f| ctrl(f, &[0x61, 0x01])) == 1
        });
        let bg = s.clone();
        let writer = std::thread::spawn(move || bg.send_uih(1, b"a"));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!writer.is_finished(), "write returned during FCoff");
        assert_eq!(
            count(&log.lock().unwrap(), data),
            0,
            "data sent during FCoff"
        );

        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xA3, 0x01])); // FCon
        assert_eq!(writer.join().unwrap(), Ok(1));
        let frames = wait_log(&log, "data after FCon", |f| count(f, data) == 1);
        let fcon = frames.iter().position(|f| ctrl(f, &[0xA1, 0x01])).unwrap();
        let first = frames.iter().position(data).unwrap();
        assert!(fcon < first, "FCon answered first");
    }

    /// FCoff for longer than the write timeout: the write fails, and nothing stale
    /// goes out on a later FCon (27.010 §5.4.6.3.5/6: the sender stops).
    #[test]
    fn fcoff_write_times_out_and_is_not_sent_later() {
        let (s, log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        s.register_dlci(1, "m#dlci=1".into());
        let data = |f: &DecodedFrame| f.dlci != 0 && f.kind == UIH;
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0x63, 0x01])); // FCoff
        let t = Instant::now();
        let err = s.send_uih(1, b"ATD+123;\r").unwrap_err();
        assert_eq!(err, "CMUX flow control off (FCoff)");
        assert!(
            t.elapsed() >= Duration::from_millis(300),
            "{:?}",
            t.elapsed()
        );
        assert!(t.elapsed() < Duration::from_secs(2), "{:?}", t.elapsed());
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xA3, 0x01])); // FCon
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(
            count(&log.lock().unwrap(), data),
            0,
            "stale data after FCon"
        );
    }

    /// A write waiting on FCoff fails as soon as its channel goes away.
    #[test]
    fn fcoff_waiter_fails_when_the_channel_closes() {
        let (s, _log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        s.register_dlci(1, "m#dlci=1".into());
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0x63, 0x01])); // FCoff
        let bg = s.clone();
        let t = Instant::now();
        let writer = std::thread::spawn(move || bg.send_uih(1, b"a"));
        std::thread::sleep(Duration::from_millis(50));
        s.feed_physical_rx(&encode_frame(1, true, DISC | PF, &[]));
        assert_eq!(
            writer.join().unwrap().unwrap_err(),
            "CMUX DLCI 1 closed by the modem"
        );
        assert!(
            t.elapsed() < Duration::from_millis(250),
            "{:?}",
            t.elapsed()
        );
    }

    /// MSC with FC=1 from the modem holds only that DLC; FC=0 releases it.
    #[test]
    fn msc_fc_bit_holds_only_that_channel() {
        let (s, log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        s.register_dlci(1, "m#dlci=1".into());
        s.register_dlci(2, "m#dlci=2".into());
        // MSC command: DLCI 1, V.24 EA|FC|RTC|RTR|DV.
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xE3, 0x05, 0x07, 0x8F]));
        assert_eq!(s.send_uih(2, b"b"), Ok(1));
        let bg = s.clone();
        let held = std::thread::spawn(move || bg.send_uih(1, b"a"));
        std::thread::sleep(Duration::from_millis(100));
        assert!(!held.is_finished(), "DLCI 1 not held by MSC FC");
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xE3, 0x05, 0x07, 0x8D]));
        assert_eq!(held.join().unwrap(), Ok(1));
        let data = |f: &DecodedFrame| f.dlci != 0 && f.kind == UIH;
        wait_log(&log, "both data frames", |f| count(f, data) == 2);
    }

    /// 27.010 §5.4.6.3.8: an unknown control command is answered with NSC; the known
    /// ones are still echoed.
    #[test]
    fn unknown_control_command_gets_nsc() {
        let (s, log) = fake_modem("AT+CMUX=0", |_| Vec::new());
        let ctrl = |f: &DecodedFrame, p: &[u8]| f.dlci == 0 && f.kind == UIH && f.payload == p;
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0xF3, 0x01]));
        wait_log(&log, "NSC", |f| {
            count(f, |f| ctrl(f, &[0x11, 0x03, 0xF3])) == 1
        });
        s.feed_physical_rx(&encode_frame(0, true, UIH, &[0x23, 0x03, 0x55]));
        wait_log(&log, "TEST echo", |f| {
            count(f, |f| ctrl(f, &[0x21, 0x03, 0x55])) == 1
        });
    }

    /// DISC on DLCI 0, CLD or an unsolicited DM on DLCI 0: the modem left multiplexer
    /// mode. Writes and opens fail, channels get `Disconnect`, the manager hook runs.
    #[test]
    fn modem_leaving_mux_kills_the_session() {
        for leave in [
            encode_frame(0, true, DISC | PF, &[]),
            encode_frame(0, true, UIH, &[CLD_CMD, 0x01]),
            encode_frame(0, true, DM | PF, &[]),
        ] {
            let (s, _log) = fake_modem("AT+CMUX=0", ua_to_sabm);
            let hook: HookLog = Arc::default();
            let seen = hook.clone();
            s.set_on_closed(Box::new(
                move |paths: &[String], _reason: &str, whole: bool| {
                    seen.lock().unwrap().push((paths.to_vec(), whole));
                },
            ));
            s.register_dlci(1, "m#dlci=1".into());
            let (old, events) = event_sink();
            s.set_watch_with(1, old, 0, None);
            s.feed_physical_rx(&leave);
            assert_eq!(disconnects(&events), vec!["m#dlci=1".to_string()]);
            assert_eq!(
                hook.lock().unwrap().as_slice(),
                [(vec!["m#dlci=1".to_string()], true)]
            );
            s.register_dlci(2, "m#dlci=2".into());
            assert!(s.send_uih(2, b"AT\r").is_err(), "write on a dead mux");
            assert!(s.open_dlci(2, Duration::from_millis(100)).is_err());
        }
    }

    /// `AT+CMUX` without T1 / N2: one SABM, the whole timeout for its UA (4.x).
    #[test]
    fn default_command_sends_a_single_sabm() {
        let (s, log) = fake_modem("AT+CMUX=0", |f| {
            if f.kind == SABM {
                std::thread::sleep(Duration::from_millis(250));
            }
            ua_to_sabm(f)
        });
        s.register_dlci(1, "m#dlci=1".into());
        s.open_dlci(1, Duration::from_secs(2)).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(count(&log.lock().unwrap(), |f| f.kind == SABM), 1);
    }

    #[test]
    fn close_down_waits_for_the_cld_response() {
        let cld = |f: &DecodedFrame| f.dlci == 0 && f.kind == UIH && f.payload == [0xC3, 0x01];
        // T2 = 50 (500 ms); the modem answers after 100 ms.
        let (s, _log) = fake_modem("AT+CMUX=0,0,5,31,10,3,50", move |f| {
            if cld(f) {
                std::thread::sleep(Duration::from_millis(100));
                encode_frame(0, true, UIH, &[0xC1, 0x01])
            } else {
                Vec::new()
            }
        });
        let start = Instant::now();
        s.close_down();
        let took = start.elapsed();
        assert!(
            took >= Duration::from_millis(100),
            "returned before the response: {took:?}"
        );
        assert!(
            took < Duration::from_millis(450),
            "missed the response: {took:?}"
        );

        // No response: close_down gives up after T2 (5 = 50 ms).
        let (s, log) = fake_modem("AT+CMUX=0,0,5,31,10,3,5", |_| Vec::new());
        let start = Instant::now();
        s.close_down();
        let took = start.elapsed();
        assert!(took >= Duration::from_millis(50), "{took:?}");
        assert!(took < Duration::from_secs(1), "{took:?}");
        wait_log(&log, "CLD", |f| count(f, cld) == 1);
    }
}
