//! CMUX session: deframe physical RX, route per DLCI, encode TX.

use crate::cmux::frame::{
    encode_frame, encode_uih, DecodedFrame, Deframer, DISC, DM, PF, SABM, UA, UI, UIH,
};
use crate::cmux::io::CmuxPhysicalIo;
use crate::events::SerialEvent;
use crate::hub::shared::Outbox;
use crate::hub::ExchangeWaiter;
use serial_codec::framing::Framer;
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Per-DLCI routing state (virtual channel).
pub struct DlciChannel {
    pub path: String,
    pub exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    watch: Mutex<Option<DlciWatchSlot>>,
    stream_buffer: Mutex<Vec<u8>>,
    /// The session's outbox: events are queued under `watch`, in production order,
    /// and delivered by the session once no lock is held.
    outbox: Arc<Outbox>,
}

struct DlciWatchSlot {
    channel: crate::events::EventSink,
    batch_timeout_ms: u64,
    flush_at: Instant,
    /// Native parser: each frame is one `Data` event, no batching.
    framer: Option<Box<dyn Framer>>,
}

impl DlciChannel {
    fn new(path: String, outbox: Arc<Outbox>) -> Self {
        Self {
            path,
            exchange_waiter: Mutex::new(None),
            watch: Mutex::new(None),
            stream_buffer: Mutex::new(Vec::new()),
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
        *crate::sync_util::lock_or_recover(&self.watch) = Some(DlciWatchSlot {
            channel,
            batch_timeout_ms,
            flush_at: Instant::now(),
            framer,
        });
    }

    /// Stop the watch; a parser's buffered remainder is queued as a last `Data` event
    /// (after the frames already queued). The session delivers it on its next RX, tick
    /// or [`CmuxSession::clear_watch`] (which calls this and delivers).
    pub fn detach_watch(&self) {
        let mut guard = crate::sync_util::lock_or_recover(&self.watch);
        crate::sync_util::lock_or_recover(&self.stream_buffer).clear();
        if let Some(mut slot) = guard.take() {
            if let Some(rest) = slot.framer.as_mut().and_then(|f| f.flush()) {
                self.queue_frames(&slot.channel, vec![rest]);
            }
        }
    }

    /// Physical link lost: queue the batch / parser remainder, then `Disconnect`.
    fn queue_disconnect(&self, reason: &str) {
        let mut guard = crate::sync_util::lock_or_recover(&self.watch);
        let Some(slot) = guard.as_mut() else {
            return;
        };
        let batch = std::mem::take(&mut *crate::sync_util::lock_or_recover(&self.stream_buffer));
        let rest = slot.framer.as_mut().and_then(|f| f.flush());
        let frames: Vec<_> = [batch, rest.unwrap_or_default()]
            .into_iter()
            .filter(|d| !d.is_empty())
            .collect();
        self.queue_frames(&slot.channel, frames);
        self.outbox.push([(
            slot.channel.clone(),
            SerialEvent::Disconnect {
                path: self.path.clone(),
                reason: reason.to_string(),
            },
        )]);
    }

    /// Queue `Data` events; called with the `watch` lock held so the order is the
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

    /// Run the parser (push or poll) and queue its frames under the `watch` lock.
    /// `false` when the watch has no parser.
    fn with_framer(&self, run: impl FnOnce(&mut dyn Framer, &mut Vec<Vec<u8>>)) -> bool {
        let mut guard = crate::sync_util::lock_or_recover(&self.watch);
        let Some(slot) = guard.as_mut() else {
            return false;
        };
        let Some(f) = slot.framer.as_mut() else {
            return false;
        };
        let mut frames = Vec::new();
        run(f.as_mut(), &mut frames);
        self.queue_frames(&slot.channel, frames);
        true
    }

    fn push_stream(&self, payload: &[u8]) {
        if self.with_framer(|f, out| f.push(payload, Instant::now(), out)) {
            return;
        }
        let due = {
            let mut guard = crate::sync_util::lock_or_recover(&self.watch);
            let Some(slot) = guard.as_mut() else {
                return;
            };
            crate::sync_util::lock_or_recover(&self.stream_buffer).extend_from_slice(payload);
            slot.take_due()
        };
        // Flush outside the `watch` lock: flush_stream_buffer takes it again.
        if due {
            self.flush_stream_buffer();
        }
    }

    /// Flush a batch whose timeout elapsed with no new frame to trigger it.
    fn tick(&self) {
        if self.with_framer(|f, out| f.poll(Instant::now(), out)) {
            return;
        }
        let due = crate::sync_util::lock_or_recover(&self.watch)
            .as_mut()
            .is_some_and(DlciWatchSlot::take_due);
        if due {
            self.flush_stream_buffer();
        }
    }

    fn flush_stream_buffer(&self) {
        let guard = crate::sync_util::lock_or_recover(&self.watch);
        let data = std::mem::take(&mut *crate::sync_util::lock_or_recover(&self.stream_buffer));
        if let (Some(slot), false) = (guard.as_ref(), data.is_empty()) {
            self.queue_frames(&slot.channel, vec![data]);
        }
    }
}

impl DlciWatchSlot {
    fn take_due(&mut self) -> bool {
        if self.flush_at.elapsed() >= Duration::from_millis(self.batch_timeout_ms) {
            self.flush_at = Instant::now();
            true
        } else {
            false
        }
    }
}

/// N1 (maximum info bytes per frame) from an `AT+CMUX=` command: the 4th parameter
/// (27.007 +CMUX: mode, subset, port_speed, N1, T1, N2, T2, T3, k). 31 (the basic-option
/// default) when absent or empty.
pub fn cmux_n1(command: &str) -> usize {
    const DEFAULT_N1: usize = 31;
    let upper = command.to_ascii_uppercase();
    let Some(at) = upper.find("+CMUX=") else {
        return DEFAULT_N1;
    };
    upper[at + "+CMUX=".len()..]
        .split(',')
        .nth(3)
        .and_then(|n1| n1.trim().parse().ok())
        .unwrap_or(DEFAULT_N1)
}

/// Active GSM 07.10 multiplexer on a physical serial port.
pub struct CmuxSession {
    physical_path: String,
    io: Arc<dyn CmuxPhysicalIo>,
    deframer: Mutex<Deframer>,
    channels: Mutex<HashMap<u8, Arc<DlciChannel>>>,
    /// SABM sent, waiting for UA (true) or DM (false).
    pending_open: Mutex<HashMap<u8, mpsc::Sender<bool>>>,
    outbox: Arc<Outbox>,
}

impl CmuxSession {
    /// Session with the basic-option default N1 (31, the 4.0.0 behaviour);
    /// [`Self::with_n1`] for the N1 given to `AT+CMUX`.
    pub fn new(physical_path: String, io: Arc<dyn CmuxPhysicalIo>) -> Arc<Self> {
        Self::with_n1(physical_path, io, serial_codec::cmux::TX_N1)
    }

    /// `n1`: the negotiated maximum frame size (`AT+CMUX` N1, see [`cmux_n1`]).
    /// Frames up to `max(n1, DEFAULT_MAX_RX_INFO)` info bytes are accepted.
    pub fn with_n1(physical_path: String, io: Arc<dyn CmuxPhysicalIo>, n1: usize) -> Arc<Self> {
        Arc::new(Self {
            physical_path,
            io,
            deframer: Mutex::new(Deframer::with_max_info(
                n1.max(crate::cmux::frame::DEFAULT_MAX_RX_INFO),
            )),
            channels: Mutex::new(HashMap::new()),
            pending_open: Mutex::new(HashMap::new()),
            outbox: Arc::default(),
        })
    }

    pub fn physical_path(&self) -> &str {
        &self.physical_path
    }

    pub fn register_dlci(&self, dlci: u8, virtual_path: String) -> Arc<DlciChannel> {
        let ch = Arc::new(DlciChannel::new(virtual_path, self.outbox.clone()));
        crate::sync_util::lock_or_recover(&self.channels).insert(dlci, ch.clone());
        ch
    }

    pub fn unregister_dlci(&self, dlci: u8) {
        if crate::sync_util::lock_or_recover(&self.channels)
            .remove(&dlci)
            .is_some()
        {
            let _ = self.io.write_all(&encode_frame(dlci, true, DISC | PF, &[]));
        }
    }

    pub fn has_dlci(&self, dlci: u8) -> bool {
        crate::sync_util::lock_or_recover(&self.channels).contains_key(&dlci)
    }

    /// Establish a DLC: send SABM and wait for the modem's UA (DM = refused).
    /// DLCI 0 (the control channel) must be opened first.
    pub fn open_dlci(&self, dlci: u8, timeout: Duration) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        crate::sync_util::lock_or_recover(&self.pending_open).insert(dlci, tx);
        let sent = self.io.write_all(&encode_frame(dlci, true, SABM | PF, &[]));
        let result = sent.and_then(|()| match rx.recv_timeout(timeout) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!("modem refused CMUX DLCI {dlci} (DM)")),
            Err(_) => Err(format!(
                "no UA for CMUX DLCI {dlci} within {} ms",
                timeout.as_millis()
            )),
        });
        crate::sync_util::lock_or_recover(&self.pending_open).remove(&dlci);
        result
    }

    /// Ask the modem to leave multiplexer mode (CLD on the control channel).
    pub fn close_down(&self) {
        // CLD command: type octet 0xC3 (EA=1, C/R=1, type 0x30), length 0.
        let _ = self
            .io
            .write_all(&encode_frame(0, true, UIH, &[0xC3, 0x01]));
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

    pub fn send_uih(&self, dlci: u8, payload: &[u8]) -> Result<usize, String> {
        let frame = encode_uih(dlci, payload);
        self.io.write_all(&frame)?;
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
                }
                return;
            }
            UIH | UI => {}
            // SABM/DISC from the modem: we are the initiator, nothing to accept.
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

    /// Control-channel message from the modem. Commands (MSC, TEST, FCon/FCoff …) are
    /// acknowledged by echoing them back with the type octet's C/R bit cleared, as
    /// 27.010 §5.4.6 requires; responses to our own commands need no action.
    fn handle_control(&self, payload: &[u8]) {
        let Some(&type_octet) = payload.first() else {
            return;
        };
        if type_octet & 0x02 == 0 {
            return;
        }
        let mut reply = payload.to_vec();
        reply[0] &= !0x02;
        let _ = self.io.write_all(&encode_frame(0, true, UIH, &reply));
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
}
