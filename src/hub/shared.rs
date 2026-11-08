//! Shared RX hub state (desktop poll + Android push).

use crate::at::parse::{classify_final_line, is_likely_urc, ExchangeDemux, ExchangeMatch};
use crate::cmux::CmuxSession;
use crate::events::SerialEvent;
use crate::exchange::completion::check_exchange_complete;
use crate::exchange::options::ResolvedExchangeOptions;
use serial_codec::framing::Framer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

pub(crate) const IDLE_BUFFER_CAP: usize = 64 * 1024;

type ExchangeDone = Arc<(
    Mutex<Option<Result<(Vec<u8>, ExchangeMatch), String>>>,
    Condvar,
)>;
type ByteResultTx = SyncSender<Result<Vec<u8>, String>>;

/// Actions produced when routing incoming bytes in streaming mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RxRouteAction {
    StreamData(Vec<u8>),
    UrcLine(String),
}

/// Session waiting for an exchange to complete.
pub struct ExchangeWaiter {
    pub options: ResolvedExchangeOptions,
    buffer: Mutex<Vec<u8>>,
    done: ExchangeDone,
    pub cancel: Arc<AtomicBool>,
}

impl ExchangeWaiter {
    pub fn new(options: ResolvedExchangeOptions, cancel: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            options,
            buffer: Mutex::new(Vec::new()),
            done: Arc::new((Mutex::new(None), Condvar::new())),
            cancel,
        })
    }

    pub fn push_bytes(&self, chunk: &[u8]) {
        let mut buffer = crate::sync_util::lock_or_recover(&self.buffer);
        buffer.extend_from_slice(chunk);
        if self.cancel.load(Ordering::SeqCst) {
            self.finish(Err("exchange cancelled".into()));
            return;
        }
        if buffer.len() >= self.options.max_bytes {
            self.finish(Err(format!(
                "exchange response exceeded {} bytes",
                self.options.max_bytes
            )));
            return;
        }
        if let Some(matched) = check_exchange_complete(&buffer, &self.options) {
            let raw = std::mem::take(&mut *buffer);
            self.finish(Ok((raw, matched)));
        }
    }

    pub fn wait(self: &Arc<Self>, timeout_ms: u64) -> Result<(Vec<u8>, ExchangeMatch), String> {
        let (lock, cvar) = &*self.done;
        let mut guard = crate::sync_util::lock_or_recover(lock);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while guard.is_none() {
            if self.cancel.load(Ordering::SeqCst) {
                return Err("exchange cancelled".into());
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("exchange timed out after {} ms", timeout_ms));
            }
            let (g, timeout) = cvar
                .wait_timeout(guard, remaining.min(Duration::from_millis(50)))
                .map_err(|e| e.to_string())?;
            guard = g;
            if guard.is_none() && timeout.timed_out() && Instant::now() >= deadline {
                return Err(format!("exchange timed out after {} ms", timeout_ms));
            }
        }
        guard.take().unwrap()
    }

    fn finish(&self, result: Result<(Vec<u8>, ExchangeMatch), String>) {
        let (lock, cvar) = &*self.done;
        let mut guard = crate::sync_util::lock_or_recover(lock);
        *guard = Some(result);
        cvar.notify_all();
    }

    /// Fail an in-flight exchange immediately (e.g. USB error teardown).
    pub fn fail_with_reason(&self, reason: String) {
        self.finish(Err(reason));
    }
}

/// Line-oriented router for streaming when no exchange is active (`route_urc`).
///
/// Data lines go out byte-for-byte, terminator included; only URC lines are pulled out.
/// An unfinished line is held until the next chunk (or the batch flush), so a URC split
/// across two reads is still recognised and UTF-8 characters are never cut in half.
#[derive(Debug, Default)]
pub struct LineRouter {
    partial: Vec<u8>,
}

impl LineRouter {
    // ponytail: splits on LF only, so outside an exchange the quoted multi-line `+CUSD`
    // join and the glued `OK+CUSD:` peel of `serial_codec::at::split_lines` do not apply
    // here. Reusing them needs an incremental, byte-preserving splitter exported from
    // `at.rs` (the one in `ExchangeDemux::process_chunk` is private and returns trimmed
    // text, while this router must forward data lines byte-for-byte).
    pub fn route_streaming(
        &mut self,
        chunk: &[u8],
        solicited_prefixes: &[String],
    ) -> Vec<RxRouteAction> {
        self.partial.extend_from_slice(chunk);
        let mut actions = Vec::new();
        while let Some(pos) = self.partial.iter().position(|&b| b == b'\n') {
            let raw: Vec<u8> = self.partial.drain(..=pos).collect();
            let line = String::from_utf8_lossy(&raw).trim().to_string();
            if !line.is_empty()
                && is_likely_urc(&line, solicited_prefixes)
                && classify_final_line(&line).is_none()
            {
                actions.push(RxRouteAction::UrcLine(line));
            } else {
                actions.push(RxRouteAction::StreamData(raw));
            }
        }
        actions
    }

    /// Unfinished line bytes (handed to the data stream on batch flush).
    pub fn take_partial(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.partial)
    }
}

pub fn emit_urc(channel: &Channel<SerialEvent>, path: &str, line: &str) {
    let _ = channel.send(SerialEvent::Urc {
        path: path.to_string(),
        line: line.to_string(),
    });
}

/// Per-port routing state shared between desktop poll loop and Android push feed.
pub struct HubRoutingState {
    pub path: String,
    pub line_router: LineRouter,
    /// URC demux for the exchange it was built for; rebuilt when the waiter changes.
    pub exchange_demux: Option<(Arc<ExchangeWaiter>, ExchangeDemux)>,
    pub combined_buffer: Vec<u8>,
    pub flush_at: Instant,
    /// Watch/URC events queued under routing lock; dispatched after the lock is released.
    pub pending_events: Vec<SerialEvent>,
}

impl HubRoutingState {
    pub fn new(path: String) -> Self {
        Self {
            path,
            line_router: LineRouter::default(),
            exchange_demux: None,
            combined_buffer: Vec::with_capacity(1024),
            flush_at: Instant::now(),
            pending_events: Vec::new(),
        }
    }
}

pub(crate) struct WatchSlot {
    pub(crate) channel: Channel<SerialEvent>,
    pub(crate) batch_timeout_ms: u64,
    /// Poll read chunk size for the hub thread.
    pub(crate) read_size: usize,
    /// When true, idle watch RX goes through [`LineRouter`] (AT URC split).
    pub(crate) route_urc: bool,
    /// Native parser: each frame is its own `Data` event, no batching.
    ///
    /// With `route_urc` the URC split runs first and the parser sees only the data
    /// lines. That is the least surprising order: `onUrc` keeps its meaning whatever
    /// the parser, and a parser never has to know AT syntax. The line router's
    /// unterminated tail reaches the parser on the batch timer, as it reaches `data`
    /// without a parser.
    pub(crate) framer: Option<Box<dyn Framer>>,
}

impl WatchSlot {
    /// Feed watch bytes: into the parser (one event per frame) or the batch buffer.
    fn sink(&mut self, path: &str, bytes: &[u8], state: &mut HubRoutingState) {
        match self.framer.as_mut() {
            Some(f) => {
                let mut frames = Vec::new();
                f.push(bytes, Instant::now(), &mut frames);
                push_frames(path, frames, &mut state.pending_events);
            }
            None => state.combined_buffer.extend_from_slice(bytes),
        }
    }

    /// End of stream: the parser's buffered partial frame as a final `Data` event.
    fn take_remainder(&mut self, path: &str) -> Option<SerialEvent> {
        let data = self.framer.as_mut()?.flush()?;
        Some(data_event(path, data))
    }
}

fn data_event(path: &str, data: Vec<u8>) -> SerialEvent {
    SerialEvent::Data {
        path: path.to_string(),
        size: data.len(),
        data,
    }
}

fn push_frames(path: &str, frames: Vec<Vec<u8>>, pending: &mut Vec<SerialEvent>) {
    pending.extend(frames.into_iter().map(|f| data_event(path, f)));
}

pub(crate) struct DrainSlot {
    pub(crate) idle_ms: u64,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) buffer: Vec<u8>,
    pub(crate) last_byte_at: Option<Instant>,
    pub(crate) started_at: Instant,
    pub(crate) deadline: Instant,
    pub(crate) solicited_prefixes: Vec<String>,
    pub(crate) tx: ByteResultTx,
}

pub(crate) struct ReadSlot {
    pub(crate) max_bytes: usize,
    pub(crate) fill: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) buffer: Vec<u8>,
    pub(crate) deadline: Instant,
    pub(crate) tx: ByteResultTx,
}

/// Shared hub state between the RX thread and API handlers.
pub struct RxHubShared {
    pub(crate) exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    pub(crate) watch: Mutex<Option<WatchSlot>>,
    pub(crate) drain: Mutex<Option<DrainSlot>>,
    pub(crate) read_slot: Mutex<Option<ReadSlot>>,
    pub(crate) idle: Mutex<Vec<u8>>,
    pub(crate) cmux: Mutex<Option<Arc<CmuxSession>>>,
    /// Port path, for events sent outside the hub thread (watch remainder on detach).
    path: String,
}

impl Default for RxHubShared {
    fn default() -> Self {
        Self::new()
    }
}

impl RxHubShared {
    pub fn new() -> Self {
        Self::for_path(String::new())
    }

    pub fn for_path(path: String) -> Self {
        Self {
            path,
            exchange_waiter: Mutex::new(None),
            watch: Mutex::new(None),
            drain: Mutex::new(None),
            read_slot: Mutex::new(None),
            idle: Mutex::new(Vec::new()),
            cmux: Mutex::new(None),
        }
    }

    pub fn attach_watch(
        &self,
        channel: Channel<SerialEvent>,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
        framer: Option<Box<dyn Framer>>,
    ) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        *crate::sync_util::lock_or_recover(&self.watch) = Some(WatchSlot {
            channel,
            batch_timeout_ms,
            read_size,
            route_urc,
            framer,
        });
    }

    /// Stop the watch; a parser's buffered remainder goes out as a last `Data` event.
    pub fn detach_watch(&self) {
        let slot = crate::sync_util::lock_or_recover(&self.watch).take();
        if let Some(mut slot) = slot {
            if let Some(ev) = slot.take_remainder(&self.path) {
                crate::port::watch_registry::send_event(&slot.channel, ev);
            }
        }
    }

    pub fn attach_cmux(&self, session: Arc<CmuxSession>) {
        *crate::sync_util::lock_or_recover(&self.cmux) = Some(session);
    }

    pub fn detach_cmux(&self) {
        *crate::sync_util::lock_or_recover(&self.cmux) = None;
    }

    /// Install the waiter. Bytes that arrived with no consumer (`idle`) are handed to it
    /// first, under the idle lock, so they keep their order ahead of the reply.
    pub fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>) {
        let mut idle = crate::sync_util::lock_or_recover(&self.idle);
        if !idle.is_empty() {
            waiter.push_bytes(&std::mem::take(&mut *idle));
        }
        *crate::sync_util::lock_or_recover(&self.exchange_waiter) = Some(waiter);
    }

    pub fn clear_exchange_waiter(&self) {
        *crate::sync_util::lock_or_recover(&self.exchange_waiter) = None;
    }

    /// Wake an in-flight exchange waiter when [`crate::api::serial::SerialPort::cancel_exchange`] is invoked.
    pub fn cancel_active_exchange(&self) {
        if let Some(waiter) = crate::sync_util::lock_or_recover(&self.exchange_waiter).as_ref() {
            waiter.fail_with_reason("exchange cancelled".into());
        }
    }

    /// Push-model entry: route incoming bytes (Android JNI / tests).
    pub fn feed_bytes(&self, chunk: &[u8], state: &mut HubRoutingState) {
        if chunk.is_empty() {
            return;
        }
        let path = state.path.clone();
        if let Some(session) = crate::sync_util::lock_or_recover(&self.cmux).clone() {
            session.feed_physical_rx(chunk);
            return;
        }
        if route_drain_chunk(self, chunk) {
            return;
        }
        if let Some(waiter) = crate::sync_util::lock_or_recover(&self.exchange_waiter).clone() {
            route_exchange_chunk(self, &path, chunk, state, waiter);
            return;
        }
        if route_read_slot_chunk(self, chunk) {
            return;
        }
        if self.has_watch() {
            route_watch_chunk(self, &path, chunk, state);
            return;
        }
        push_idle(self, chunk);
    }

    /// Idle timers for push model: drain completion + watch batch flush + read deadlines.
    pub fn tick(&self, path: &str, state: &mut HubRoutingState) {
        tick_read_slot(self);
        try_complete_drain(self);
        if let Some(session) = crate::sync_util::lock_or_recover(&self.cmux).clone() {
            session.tick();
        }

        let (batch_timeout_ms, read_size) = {
            let mut guard = crate::sync_util::lock_or_recover(&self.watch);
            if let Some(w) = guard.as_mut().filter(|w| w.framer.is_some()) {
                // Parser watch: frames already went out one by one. Release the line
                // router tail on the batch timer, then fire time-based frames.
                if state.flush_at.elapsed() >= Duration::from_millis(w.batch_timeout_ms) {
                    state.flush_at = Instant::now();
                    let partial = state.line_router.take_partial();
                    if !partial.is_empty() {
                        w.sink(path, &partial, state);
                    }
                }
                let mut frames = Vec::new();
                if let Some(f) = w.framer.as_mut() {
                    f.poll(Instant::now(), &mut frames);
                }
                push_frames(path, frames, &mut state.pending_events);
                // Bytes batched before a parser watch replaced a plain one.
                let stale = std::mem::take(&mut state.combined_buffer);
                if !stale.is_empty() {
                    state.pending_events.insert(0, data_event(path, stale));
                }
                return;
            }
            guard
                .as_ref()
                .map(|w| (w.batch_timeout_ms, w.read_size))
                .unwrap_or((1000, usize::MAX))
        };
        // Flush on the batch timer, or as soon as a full `size` worth is buffered,
        // so a fast stream does not grow one huge event per window.
        if state.flush_at.elapsed() >= Duration::from_millis(batch_timeout_ms)
            || state.combined_buffer.len() >= read_size.max(1)
        {
            state.flush_at = Instant::now();
            let partial = state.line_router.take_partial();
            state.combined_buffer.extend_from_slice(&partial);
            flush_watch_data(
                self,
                path,
                &mut state.combined_buffer,
                &mut state.pending_events,
            );
        }
    }

    /// Immediately fail exchange waiters, active drain, and pending read (USB error teardown).
    pub fn fail_all_waiters(&self, reason: &str) {
        if let Some(waiter) = crate::sync_util::lock_or_recover(&self.exchange_waiter).take() {
            waiter.fail_with_reason(reason.to_string());
        }
        finish_drain(self, Err(reason.to_string()));
        finish_read_slot(self, Err(reason.to_string()));
    }

    pub fn buffered_len(&self) -> usize {
        let idle_len = crate::sync_util::lock_or_recover(&self.idle).len();
        let read_len = crate::sync_util::lock_or_recover(&self.read_slot)
            .as_ref()
            .map(|slot| slot.buffer.len())
            .unwrap_or(0);
        idle_len + read_len
    }

    pub fn purge_buffers(&self) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
    }

    /// Take any bytes buffered without an active consumer (e.g. early RX before exchange waiter).
    pub fn take_idle_bytes(&self) -> Vec<u8> {
        std::mem::take(&mut *crate::sync_util::lock_or_recover(&self.idle))
    }

    pub fn cancel_pending_read(&self) {
        finish_read_slot(self, Err("read cancelled".into()));
    }

    /// Blocking poll-read via the hub (raw bytes, bypasses [`LineRouter`]).
    pub fn read_request(
        &self,
        max_bytes: usize,
        timeout_ms: u64,
        fill: bool,
    ) -> Result<Vec<u8>, String> {
        if self.has_watch() {
            return Err("Cannot read while watch is active; use watch or exchange".into());
        }

        let max_bytes = max_bytes.max(1);
        let wait = Duration::from_millis(timeout_ms);
        let deadline = Instant::now() + wait;

        let mut initial = Vec::new();
        {
            let mut idle = crate::sync_util::lock_or_recover(&self.idle);
            if !idle.is_empty() {
                if fill {
                    let n = max_bytes.min(idle.len());
                    initial.extend(idle.drain(..n));
                    if initial.len() >= max_bytes {
                        return Ok(initial);
                    }
                } else {
                    let n = idle.len().min(max_bytes);
                    return Ok(idle.drain(..n).collect());
                }
            }
        }

        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut guard = crate::sync_util::lock_or_recover(&self.read_slot);
            if guard.is_some() {
                return Err("read already in progress".into());
            }
            *guard = Some(ReadSlot {
                max_bytes,
                fill,
                timeout_ms,
                buffer: initial,
                deadline,
                tx,
            });
        }

        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).take() {
                    if slot.buffer.is_empty() {
                        Err(format!("no data received within {} ms", timeout_ms))
                    } else {
                        Ok(slot.buffer)
                    }
                } else {
                    // Completer took the slot then sends — brief wait covers that gap.
                    rx.recv_timeout(Duration::from_millis(50))
                        .unwrap_or_else(|_| {
                            Err(format!("no data received within {} ms", timeout_ms))
                        })
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                Err(format!("no data received within {} ms", timeout_ms))
            }
        }
    }

    pub fn pending_watch_bytes(&self, state: &HubRoutingState) -> usize {
        state.combined_buffer.len()
    }

    pub fn flush_watch_now(&self, state: &mut HubRoutingState) {
        let partial = state.line_router.take_partial();
        self.sink_watch(&partial, state);
        flush_watch_data(
            self,
            &state.path,
            &mut state.combined_buffer,
            &mut state.pending_events,
        );
    }

    /// Send `Disconnect` on the watch, after the parser's remainder as a last `Data`.
    pub fn emit_disconnect(&self, path: &str, reason: &str) {
        let (channel, remainder) = match crate::sync_util::lock_or_recover(&self.watch).as_mut() {
            Some(watch) => (Some(watch.channel.clone()), watch.take_remainder(path)),
            None => (None, None),
        };
        if let Some(channel) = channel {
            if let Some(ev) = remainder {
                crate::port::watch_registry::send_event(&channel, ev);
            }
            let _ = channel.send(SerialEvent::Disconnect {
                path: path.to_string(),
                reason: reason.to_string(),
            });
        }
    }

    /// Port going away: flush the batch and the parser remainder into `pending_events`.
    pub fn finish_watch_stream(&self, state: &mut HubRoutingState) {
        self.flush_watch_now(state);
        if let Some(watch) = crate::sync_util::lock_or_recover(&self.watch).as_mut() {
            state
                .pending_events
                .extend(watch.take_remainder(&state.path));
        }
    }

    /// Bytes for the watch with no URC routing (line router tail): parser or batch.
    fn sink_watch(&self, bytes: &[u8], state: &mut HubRoutingState) {
        if bytes.is_empty() {
            return;
        }
        match crate::sync_util::lock_or_recover(&self.watch).as_mut() {
            Some(watch) => watch.sink(&state.path.clone(), bytes, state),
            None => state.combined_buffer.extend_from_slice(bytes),
        }
    }

    pub fn has_watch(&self) -> bool {
        crate::sync_util::lock_or_recover(&self.watch).is_some()
    }

    /// Deliver events queued while holding the routing mutex (avoids channel.send under lock).
    pub fn dispatch_pending_events(&self, events: Vec<SerialEvent>) {
        if events.is_empty() {
            return;
        }
        let channel = crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .map(|watch| watch.channel.clone());
        if let Some(channel) = channel {
            for ev in events {
                crate::port::watch_registry::send_event(&channel, ev);
            }
        }
    }

    /// Soft-drain via the hub thread (single reader); URC lines are emitted on the watch channel.
    pub fn drain(
        &self,
        idle_ms: u64,
        max_ms: u64,
        cancel: Arc<AtomicBool>,
        solicited_prefixes: Vec<String>,
    ) -> Result<Vec<u8>, String> {
        let wait = Duration::from_millis(max_ms.saturating_add(500));
        let (tx, rx) = mpsc::sync_channel(1);
        {
            let mut guard = crate::sync_util::lock_or_recover(&self.drain);
            if guard.is_some() {
                return Err("drain already in progress".into());
            }
            *guard = Some(DrainSlot {
                idle_ms,
                cancel,
                buffer: Vec::new(),
                last_byte_at: None,
                started_at: Instant::now(),
                deadline: Instant::now() + Duration::from_millis(max_ms),
                solicited_prefixes,
                tx,
            });
        }

        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.drain).take() {
                    emit_drain_urcs(self, &slot);
                    if slot.buffer.is_empty() {
                        Err("drain timed out waiting for hub".into())
                    } else {
                        Ok(slot.buffer)
                    }
                } else {
                    // Completer took the slot then sends — brief wait covers that gap.
                    rx.recv_timeout(Duration::from_millis(50))
                        .unwrap_or_else(|_| Err("drain timed out waiting for hub".into()))
                }
            }
            Err(RecvTimeoutError::Disconnected) => Err("drain timed out waiting for hub".into()),
        }
    }
}

/// Append RX into the active drain slot. Returns `false` if there is no slot
/// (caller must route elsewhere — never silently drop the chunk).
/// URCs in the drained bytes go out when the drain completes, judged over the whole
/// buffer: a two-line URC (`+CMT:` + PDU) or a multi-line `+CUSD` split across reads
/// stays together. That delays them by at most the drain window (default 200 ms).
pub(crate) fn route_drain_chunk(shared: &RxHubShared, chunk: &[u8]) -> bool {
    let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
    let Some(drain) = guard.as_mut() else {
        return false;
    };
    drain.buffer.extend_from_slice(chunk);
    drain.last_byte_at = Some(Instant::now());
    true
}

pub(crate) fn route_exchange_chunk(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    state: &mut HubRoutingState,
    waiter: Arc<ExchangeWaiter>,
) {
    let stale = !matches!(&state.exchange_demux, Some((w, _)) if Arc::ptr_eq(w, &waiter));
    if stale {
        let cmd = waiter.options.command.clone().unwrap_or_default();
        let demux = ExchangeDemux::new(&cmd, &waiter.options.solicited_prefixes);
        state.exchange_demux = Some((waiter.clone(), demux));
    }
    if let Some((_, demux)) = state.exchange_demux.as_mut() {
        for line in demux.process_chunk(chunk) {
            if shared.has_watch() {
                state.pending_events.push(SerialEvent::Urc {
                    path: path.to_string(),
                    line,
                });
            }
        }
    }
    waiter.push_bytes(chunk);
}

pub(crate) fn route_watch_chunk(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    state: &mut HubRoutingState,
) {
    state.exchange_demux = None;
    let mut guard = crate::sync_util::lock_or_recover(&shared.watch);
    let Some(watch) = guard.as_mut() else {
        // Watch went away since `has_watch`: the batch is dropped on flush anyway.
        return;
    };
    if !watch.route_urc {
        // Raw stream: do not UTF-8-lossify, split on 0x0A, or steal URC-looking lines.
        state.line_router = LineRouter::default();
        watch.sink(path, chunk, state);
        return;
    }
    for action in state.line_router.route_streaming(chunk, &[]) {
        match action {
            RxRouteAction::UrcLine(line) => {
                state.pending_events.push(SerialEvent::Urc {
                    path: path.to_string(),
                    line,
                });
            }
            RxRouteAction::StreamData(bytes) => watch.sink(path, &bytes, state),
        }
    }
}

/// Feed a read slot. Returns `false` if there is no slot (fall through).
pub(crate) fn route_read_slot_chunk(shared: &RxHubShared, chunk: &[u8]) -> bool {
    let completed = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let ready = {
            let Some(slot) = guard.as_mut() else {
                return false;
            };
            let remaining = slot.max_bytes.saturating_sub(slot.buffer.len());
            if remaining > 0 {
                let n = chunk.len().min(remaining);
                slot.buffer.extend_from_slice(&chunk[..n]);
            }
            remaining == 0 || !slot.fill || slot.buffer.len() >= slot.max_bytes
        };
        if !ready {
            None
        } else {
            let slot = guard.take().unwrap();
            Some((slot.tx, Ok(slot.buffer)))
        }
    };
    if let Some((tx, result)) = completed {
        let _ = tx.send(result);
    }
    true
}

pub(crate) fn tick_read_slot(shared: &RxHubShared) {
    let completed = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let expired = match guard.as_ref() {
            Some(slot) => Instant::now() >= slot.deadline,
            None => return,
        };
        if !expired {
            return;
        }
        let slot = guard.take().unwrap();
        let result = if slot.buffer.is_empty() {
            Err(format!("no data received within {} ms", slot.timeout_ms))
        } else {
            Ok(slot.buffer)
        };
        Some((slot.tx, result))
    };
    if let Some((tx, result)) = completed {
        let _ = tx.send(result);
    }
}

/// Shared idle/deadline/cancel completion for drain (desktop poll + Android tick).
pub(crate) fn try_complete_drain(shared: &RxHubShared) {
    let completed = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
        let kind = match guard.as_ref() {
            None => None,
            Some(d) if d.cancel.load(Ordering::SeqCst) => Some(DrainCompleteKind::Cancel),
            Some(d) if Instant::now() >= d.deadline => Some(DrainCompleteKind::Buffer),
            Some(d)
                if d.last_byte_at
                    .is_some_and(|t| t.elapsed() >= Duration::from_millis(d.idle_ms)) =>
            {
                Some(DrainCompleteKind::Buffer)
            }
            Some(d)
                if d.last_byte_at.is_none()
                    && d.started_at.elapsed() >= Duration::from_millis(d.idle_ms) =>
            {
                Some(DrainCompleteKind::Empty)
            }
            _ => None,
        };
        kind.map(|k| {
            let d = guard.take().unwrap();
            emit_drain_urcs(shared, &d);
            let result = match k {
                DrainCompleteKind::Cancel => Err("exchange cancelled".into()),
                DrainCompleteKind::Buffer => Ok(d.buffer),
                DrainCompleteKind::Empty => Ok(Vec::new()),
            };
            (d.tx, result)
        })
    };
    if let Some((tx, result)) = completed {
        let _ = tx.send(result);
    }
}

#[derive(Clone, Copy)]
enum DrainCompleteKind {
    Cancel,
    Buffer,
    Empty,
}

pub(crate) fn finish_read_slot(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    if let Some(slot) = crate::sync_util::lock_or_recover(&shared.read_slot).take() {
        let _ = slot.tx.send(result);
    }
}

pub(crate) fn push_idle(shared: &RxHubShared, chunk: &[u8]) {
    let mut idle = crate::sync_util::lock_or_recover(&shared.idle);
    // A waiter may have been installed after `feed_bytes` checked for one.
    if let Some(waiter) = crate::sync_util::lock_or_recover(&shared.exchange_waiter).as_ref() {
        waiter.push_bytes(chunk);
        return;
    }
    idle.extend_from_slice(chunk);
    if idle.len() > IDLE_BUFFER_CAP {
        let excess = idle.len() - IDLE_BUFFER_CAP;
        idle.drain(..excess);
    }
}

pub(crate) fn finish_drain(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    if let Some(drain) = crate::sync_util::lock_or_recover(&shared.drain).take() {
        let _ = drain.tx.send(result);
    }
}

/// Send the URC lines of a finished drain on the watch channel (if any).
fn emit_drain_urcs(shared: &RxHubShared, drain: &DrainSlot) {
    if drain.buffer.is_empty() {
        return;
    }
    let channel = crate::sync_util::lock_or_recover(&shared.watch)
        .as_ref()
        .map(|watch| watch.channel.clone());
    if let Some(channel) = channel {
        let lines =
            crate::at::parse::urc_lines_from_drain(&drain.buffer, &drain.solicited_prefixes);
        for line in lines {
            emit_urc(&channel, &shared.path, &line);
        }
    }
}

pub(crate) fn flush_watch_data(
    shared: &RxHubShared,
    path: &str,
    combined_buffer: &mut Vec<u8>,
    pending: &mut Vec<SerialEvent>,
) {
    if combined_buffer.is_empty() {
        return;
    }
    if shared.has_watch() {
        let size = combined_buffer.len();
        let data = std::mem::take(combined_buffer);
        pending.push(SerialEvent::Data {
            path: path.to_string(),
            data,
            size,
        });
    } else {
        combined_buffer.clear();
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::at::parse::ExchangeMatch;
    use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
    use std::path::PathBuf;
    use std::thread;
    use tauri::ipc::Channel;

    #[test]
    fn exchange_waiter_completes_on_final_ok_line() {
        let cancel = Arc::new(AtomicBool::new(false));
        let options = ResolvedExchangeOptions {
            timeout_ms: 5000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::Drain,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        };
        let waiter = ExchangeWaiter::new(options, cancel);
        waiter.push_bytes(b"AT\r\r\nOK\r\n");
        let result = waiter.wait(1000).expect("complete");
        assert!(matches!(result.1, ExchangeMatch::Ok));
    }

    #[test]
    fn line_router_emits_vendor_urc() {
        let mut router = LineRouter::default();
        let actions = router.route_streaming(b"^CARDLOCK: 1\r\n", &[]);
        assert!(actions
            .iter()
            .any(|a| matches!(a, RxRouteAction::UrcLine(s) if s.starts_with("^CARDLOCK"))));
    }

    #[test]
    fn watch_without_route_urc_preserves_mavlink_bytes() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);
        // MAVLink v2 HEARTBEAT: start 0xFD, payload contains 0x0A, CRC contains 0xFD.
        let frame: Vec<u8> = vec![
            0xFD, 0x09, 0x00, 0x00, 0x2A, 0x01, 0x01, 0x00, 0x00, 0x00, 0x0A, 0x2B, 0x00, 0x00,
            0x01, 0x03, 0x51, 0x04, 0x03, 0xFD, 0x72,
        ];
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(&frame, &mut routing);
        assert_eq!(routing.combined_buffer, frame);
        assert!(
            routing
                .pending_events
                .iter()
                .all(|e| !matches!(e, SerialEvent::Urc { .. })),
            "binary stream must not emit URC events"
        );
    }

    #[test]
    fn watch_without_route_urc_keeps_nmea_in_data_stream() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);
        let nmea = b"$GPGGA,123519,4807.038,N,01131.000,E,1,08,0.9,545.4,M,46.9,M,,*47\r\n$GPRMC,123519,A,4807.038,N,01131.000,E,022.4,084.4,230394,003.1,W*6A\r\n";
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(nmea, &mut routing);
        assert_eq!(routing.combined_buffer.as_slice(), nmea.as_slice());
        assert!(
            routing
                .pending_events
                .iter()
                .all(|e| !matches!(e, SerialEvent::Urc { .. })),
            "NMEA `$` lines must stay in onData when route_urc is off"
        );
    }

    #[test]
    fn watch_with_route_urc_classifies_idle_urc_lines() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, true, None);
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"^CARDLOCK: 1\r\nOK\r\n", &mut routing);
        assert!(
            routing.pending_events.iter().any(
                |e| matches!(e, SerialEvent::Urc { line, .. } if line.starts_with("^CARDLOCK"))
            ),
            "expected URC, got {:?}",
            routing.pending_events
        );
        assert!(
            routing.combined_buffer.windows(2).any(|w| w == b"OK"),
            "non-URC line should remain in data stream, got {:?}",
            routing.combined_buffer
        );
    }

    /// Regression: data lines lost their `\r\n`, and a URC split across reads went
    /// out as data.
    #[test]
    fn line_router_keeps_terminators_and_joins_split_lines() {
        let mut router = LineRouter::default();
        let mut data = Vec::new();
        let mut urc = Vec::new();
        for chunk in [&b"A\r\nB\r\n+CRE"[..], b"G: 0,1\r\n\xD0", b"\x9F\r\n"] {
            for action in router.route_streaming(chunk, &[]) {
                match action {
                    RxRouteAction::StreamData(b) => data.extend(b),
                    RxRouteAction::UrcLine(l) => urc.push(l),
                }
            }
        }
        assert_eq!(data, "A\r\nB\r\nП\r\n".as_bytes());
        assert_eq!(urc, vec!["+CREG: 0,1".to_string()]);
        assert!(router.take_partial().is_empty());
    }

    #[test]
    fn feed_bytes_exchange_with_watch_emits_live_urc_pending() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);

        let cancel = Arc::new(AtomicBool::new(false));
        let options = ResolvedExchangeOptions {
            timeout_ms: 5000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::None,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT+CSQ".into()),
            solicited_prefixes: vec![],
        };
        let waiter = ExchangeWaiter::new(options, cancel);
        shared.set_exchange_waiter(waiter.clone());

        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(
            b"\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n",
            &mut routing,
        );

        let urcs: Vec<_> = routing
            .pending_events
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Urc { line, .. } => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert!(
            urcs.iter().any(|l| l.contains("+CREG:")),
            "expected +CREG in pending URC, got {:?}",
            urcs
        );
        assert!(waiter.wait(100).is_ok());
    }

    #[test]
    fn fail_all_waiters_completes_exchange_immediately() {
        let cancel = Arc::new(AtomicBool::new(false));
        let options = ResolvedExchangeOptions {
            timeout_ms: 5000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::Drain,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        };
        let shared = RxHubShared::new();
        let waiter = ExchangeWaiter::new(options, cancel);
        shared.set_exchange_waiter(waiter.clone());
        shared.fail_all_waiters("usb error");
        let result = waiter.wait(100);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("usb error"));
    }

    #[test]
    fn push_drain_idle_completes_via_tick() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let drain_handle = thread::spawn(move || shared_bg.drain(20, 5000, cancel, vec![]));
        thread::sleep(Duration::from_millis(5));
        let mut routing = HubRoutingState::new("port".into());
        shared.feed_bytes(b"AT\r\n", &mut routing);
        thread::sleep(Duration::from_millis(30));
        shared.tick("port", &mut routing);
        let result = drain_handle.join().unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), b"AT\r\n");
    }

    #[test]
    fn read_request_returns_idle_bytes_without_watch() {
        let shared = Arc::new(RxHubShared::new());
        crate::sync_util::lock_or_recover(&shared.idle).extend_from_slice(b"hello");
        let result = shared.read_request(64, 100, false).expect("read");
        assert_eq!(result, b"hello");
    }

    #[test]
    fn read_request_fill_accumulates_until_max() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let reader = thread::spawn(move || shared_bg.read_request(6, 500, true));
        thread::sleep(Duration::from_millis(5));
        shared.feed_bytes(b"abc", &mut HubRoutingState::new("p".into()));
        shared.feed_bytes(b"def", &mut HubRoutingState::new("p".into()));
        let result = reader.join().unwrap().expect("fill read");
        assert_eq!(result, b"abcdef");
    }

    #[test]
    fn read_request_rejects_second_concurrent_slot() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let reader = thread::spawn(move || shared_bg.read_request(64, 5000, false));
        thread::sleep(Duration::from_millis(5));
        let err = shared.read_request(64, 100, false).unwrap_err();
        assert!(err.contains("already in progress"));
        shared.fail_all_waiters("cleanup");
        let _ = reader.join();
    }

    #[test]
    fn purge_buffers_clears_idle() {
        let shared = Arc::new(RxHubShared::new());
        crate::sync_util::lock_or_recover(&shared.idle).extend_from_slice(b"stale");
        shared.purge_buffers();
        assert!(crate::sync_util::lock_or_recover(&shared.idle).is_empty());
    }

    #[test]
    fn idle_buffer_drops_oldest_beyond_cap() {
        let shared = Arc::new(RxHubShared::new());
        let huge = vec![0u8; IDLE_BUFFER_CAP + 1024];
        shared.feed_bytes(&huge, &mut HubRoutingState::new("p".into()));
        assert!(crate::sync_util::lock_or_recover(&shared.idle).len() <= IDLE_BUFFER_CAP);
    }

    #[test]
    fn fail_all_waiters_completes_read_slot() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let reader = thread::spawn(move || shared_bg.read_request(64, 5000, false));
        thread::sleep(Duration::from_millis(5));
        shared.fail_all_waiters("usb error");
        let result = reader.join().unwrap();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("usb error"));
    }

    #[test]
    fn take_idle_bytes_returns_early_rx_before_waiter() {
        let shared = Arc::new(RxHubShared::new());
        shared.feed_bytes(b"early", &mut HubRoutingState::new("p".into()));
        let stale = shared.take_idle_bytes();
        assert_eq!(stale, b"early");
    }

    #[test]
    fn read_request_rejects_when_watch_active() {
        let shared = Arc::new(RxHubShared::new());
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);
        let err = shared.read_request(64, 100, false).unwrap_err();
        assert!(err.contains("watch"));
    }

    #[test]
    fn read_request_times_out_without_bytes() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let reader = thread::spawn(move || shared_bg.read_request(64, 50, false));
        let result = reader.join().unwrap();
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("no data") || err.contains("timed out") || err.contains("timeout"),
            "unexpected: {err}"
        );
    }

    #[test]
    fn attach_watch_clears_idle() {
        let shared = Arc::new(RxHubShared::new());
        shared.feed_bytes(b"stale", &mut HubRoutingState::new("p".into()));
        assert!(!crate::sync_util::lock_or_recover(&shared.idle).is_empty());
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);
        assert!(crate::sync_util::lock_or_recover(&shared.idle).is_empty());
    }

    // --- Oneshot slot model (post Condvar) ---

    fn mock_read_slot(tx: ByteResultTx, buffer: Vec<u8>, max_bytes: usize, fill: bool) -> ReadSlot {
        ReadSlot {
            max_bytes,
            fill,
            timeout_ms: 100,
            buffer,
            deadline: Instant::now() + Duration::from_secs(5),
            tx,
        }
    }

    #[test]
    fn route_read_slot_chunk_returns_false_when_no_slot() {
        let shared = RxHubShared::new();
        assert!(!route_read_slot_chunk(&shared, b"x"));
    }

    #[test]
    fn route_read_slot_chunk_completing_removes_slot_before_post() {
        let shared = RxHubShared::new();
        let (tx, rx) = mpsc::sync_channel(1);
        *crate::sync_util::lock_or_recover(&shared.read_slot) =
            Some(mock_read_slot(tx, Vec::new(), 3, false));
        assert!(route_read_slot_chunk(&shared, b"xyz"));
        assert!(crate::sync_util::lock_or_recover(&shared.read_slot).is_none());
        assert_eq!(rx.recv().unwrap().unwrap(), b"xyz");
    }

    #[test]
    fn feed_bytes_read_fallthrough_to_idle_when_slot_gone() {
        // Mirror drain fallthrough: route_read_slot_chunk false → idle, no drop.
        let shared = RxHubShared::new();
        shared.feed_bytes(b"saved", &mut HubRoutingState::new("p".into()));
        assert_eq!(
            &crate::sync_util::lock_or_recover(&shared.idle)[..],
            b"saved"
        );
    }

    #[test]
    fn tick_read_slot_deadline_takes_whole_slot() {
        let shared = RxHubShared::new();
        let (tx, rx) = mpsc::sync_channel(1);
        *crate::sync_util::lock_or_recover(&shared.read_slot) = Some(ReadSlot {
            max_bytes: 64,
            fill: false,
            timeout_ms: 50,
            buffer: b"late".to_vec(),
            deadline: Instant::now() - Duration::from_millis(1),
            tx,
        });
        tick_read_slot(&shared);
        assert!(crate::sync_util::lock_or_recover(&shared.read_slot).is_none());
        assert_eq!(rx.recv().unwrap().unwrap(), b"late");
    }

    #[test]
    fn try_complete_drain_takes_whole_slot() {
        let shared = RxHubShared::new();
        let (tx, rx) = mpsc::sync_channel(1);
        *crate::sync_util::lock_or_recover(&shared.drain) = Some(DrainSlot {
            idle_ms: 1,
            cancel: Arc::new(AtomicBool::new(false)),
            buffer: b"buf".to_vec(),
            last_byte_at: Some(Instant::now() - Duration::from_millis(50)),
            started_at: Instant::now() - Duration::from_millis(100),
            deadline: Instant::now() + Duration::from_secs(5),
            solicited_prefixes: vec![],
            tx,
        });
        try_complete_drain(&shared);
        assert!(crate::sync_util::lock_or_recover(&shared.drain).is_none());
        assert_eq!(rx.recv().unwrap().unwrap(), b"buf");
    }

    #[test]
    fn try_complete_drain_empty_waits_idle_from_started_at() {
        let shared = RxHubShared::new();
        let (tx, rx) = mpsc::sync_channel(1);
        *crate::sync_util::lock_or_recover(&shared.drain) = Some(DrainSlot {
            idle_ms: 30,
            cancel: Arc::new(AtomicBool::new(false)),
            buffer: Vec::new(),
            last_byte_at: None,
            started_at: Instant::now(),
            deadline: Instant::now() + Duration::from_secs(5),
            solicited_prefixes: vec![],
            tx,
        });
        try_complete_drain(&shared);
        assert!(crate::sync_util::lock_or_recover(&shared.drain).is_some());
        thread::sleep(Duration::from_millis(40));
        try_complete_drain(&shared);
        assert!(crate::sync_util::lock_or_recover(&shared.drain).is_none());
        assert_eq!(rx.recv().unwrap().unwrap(), b"");
    }

    fn wait_until_drain_slot(shared: &RxHubShared) {
        let start = Instant::now();
        while crate::sync_util::lock_or_recover(&shared.drain).is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "drain worker never installed slot"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    fn wait_until_read_slot(shared: &RxHubShared) {
        let start = Instant::now();
        while crate::sync_util::lock_or_recover(&shared.read_slot).is_none() {
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "read worker never installed slot"
            );
            thread::sleep(Duration::from_millis(1));
        }
    }

    #[test]
    fn route_drain_chunk_returns_false_when_no_slot() {
        let shared = RxHubShared::new();
        assert!(!route_drain_chunk(&shared, b"x"));
    }

    #[test]
    fn feed_bytes_without_drain_goes_to_idle() {
        let shared = RxHubShared::new();
        shared.feed_bytes(b"saved", &mut HubRoutingState::new("p".into()));
        assert_eq!(
            &crate::sync_util::lock_or_recover(&shared.idle)[..],
            b"saved"
        );
    }

    #[test]
    fn drain_rejects_second_concurrent_slot() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || {
            shared_bg.drain(10_000, 5_000, Arc::new(AtomicBool::new(false)), vec![])
        });
        wait_until_drain_slot(&shared);
        let err = shared
            .drain(10, 100, cancel, vec![])
            .expect_err("second drain");
        assert!(err.contains("already in progress"), "unexpected: {err}");
        finish_drain(&shared, Ok(Vec::new()));
        let _ = handle.join();
    }

    #[test]
    fn drain_timeout_recovers_result_already_posted() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || shared_bg.drain(10_000, 0, cancel, vec![]));
        wait_until_drain_slot(&shared);
        finish_drain(&shared, Ok(b"drained".to_vec()));
        assert_eq!(handle.join().unwrap().expect("recovered"), b"drained");
    }

    #[test]
    fn drain_timeout_reclaim_returns_buffer_left_in_slot() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || shared_bg.drain(10_000, 0, cancel, vec![]));
        wait_until_drain_slot(&shared);
        {
            let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
            let slot = guard.as_mut().expect("drain slot still live");
            slot.buffer.extend_from_slice(b"kept");
            slot.last_byte_at = Some(Instant::now());
        }
        assert_eq!(handle.join().unwrap().expect("slot buffer"), b"kept");
    }

    #[test]
    fn drain_timeout_reclaim_empty_slot_is_timeout_error() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let err = shared
            .drain(10_000, 0, cancel, vec![])
            .expect_err("empty reclaim");
        assert!(err.contains("drain timed out"), "unexpected: {err}");
    }

    #[test]
    fn drain_timeout_preserves_finish_drain_error() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || shared_bg.drain(10_000, 0, cancel, vec![]));
        wait_until_drain_slot(&shared);
        finish_drain(&shared, Err("drain read failed: boom".into()));
        let err = handle.join().unwrap().expect_err("posted Err");
        assert!(
            err.contains("drain read failed"),
            "must not rewrite to generic timeout, got: {err}"
        );
    }

    #[test]
    fn read_request_timeout_recovers_result_already_posted() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || shared_bg.read_request(64, 500, false));
        wait_until_read_slot(&shared);
        finish_read_slot(&shared, Ok(b"late".to_vec()));
        assert_eq!(handle.join().unwrap().expect("recovered"), b"late");
    }

    fn at_csq_options() -> ResolvedExchangeOptions {
        ResolvedExchangeOptions {
            timeout_ms: 5000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::None,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT+CSQ".into()),
            solicited_prefixes: vec![],
        }
    }

    fn decode_hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex"))
            .collect()
    }

    #[test]
    fn hub_script_creg_csq_ok_fixture_feeds_urc_and_completes() {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("src/tests/fixtures/hub_scripts/creg_csq_ok.json");
        let text = std::fs::read_to_string(&path).expect("fixture");
        let fixture: serde_json::Value = serde_json::from_str(&text).expect("json");
        let command = fixture["command"].as_str().unwrap();
        let chunks: Vec<Vec<u8>> = fixture["chunks_hex"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| decode_hex(v.as_str().unwrap()))
            .collect();
        let expected_urc = fixture["expected_urc_contains"].as_str().unwrap();
        let expected_match = fixture["expected_match"].as_str().unwrap();

        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024, false, None);
        let mut options = at_csq_options();
        options.command = Some(command.to_string());
        let waiter = ExchangeWaiter::new(options, Arc::new(AtomicBool::new(false)));
        shared.set_exchange_waiter(waiter.clone());

        let mut routing = HubRoutingState::new("p".into());
        for chunk in &chunks {
            shared.feed_bytes(chunk, &mut routing);
        }

        let urcs: Vec<_> = routing
            .pending_events
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Urc { line, .. } => Some(line.clone()),
                _ => None,
            })
            .collect();
        assert!(
            urcs.iter().any(|l| l.contains(expected_urc)),
            "expected {expected_urc} in {:?}",
            urcs
        );
        let (_, matched) = waiter.wait(100).expect("complete");
        match expected_match {
            "ok" => assert!(
                matches!(matched, ExchangeMatch::Ok),
                "expected Ok, got {matched:?}"
            ),
            "error" => assert!(
                matches!(matched, ExchangeMatch::Error),
                "expected Error, got {matched:?}"
            ),
            other => panic!("unsupported expected_match `{other}` in fixture"),
        }
    }

    #[test]
    fn feed_bytes_chunked_vs_oneshot_same_exchange_match() {
        let transcript = b"\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n";

        let run = |chunks: Vec<&[u8]>| {
            let shared = RxHubShared::new();
            let waiter = ExchangeWaiter::new(at_csq_options(), Arc::new(AtomicBool::new(false)));
            shared.set_exchange_waiter(waiter.clone());
            let mut routing = HubRoutingState::new("p".into());
            for chunk in chunks {
                shared.feed_bytes(chunk, &mut routing);
            }
            waiter.wait(100).expect("complete").1
        };

        let oneshot = run(vec![&transcript[..]]);
        let bytewise = run(transcript.chunks(1).collect());
        assert_eq!(oneshot, bytewise);
        assert!(matches!(oneshot, ExchangeMatch::Ok));
    }

    #[test]
    fn fail_all_waiters_during_active_drain_completes_with_error() {
        let shared = Arc::new(RxHubShared::new());
        let shared_bg = shared.clone();
        let handle = thread::spawn(move || {
            shared_bg.drain(10_000, 5_000, Arc::new(AtomicBool::new(false)), vec![])
        });
        wait_until_drain_slot(&shared);
        shared.fail_all_waiters("usb error");
        let err = handle.join().unwrap().expect_err("drain must fail");
        assert!(err.contains("usb error"), "unexpected: {err}");
    }

    // --- Native parser on the watch ---

    fn parser(json: serde_json::Value) -> Option<Box<dyn Framer>> {
        let cfg: serial_codec::framing::FramerConfig = serde_json::from_value(json).unwrap();
        Some(cfg.build().unwrap())
    }

    /// Channel that records every event, in order.
    fn recording_channel() -> (Channel<SerialEvent>, Arc<Mutex<Vec<SerialEvent>>>) {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let channel = Channel::<SerialEvent>::new(move |body| {
            if let tauri::ipc::InvokeResponseBody::Json(json) = body {
                sink.lock()
                    .unwrap()
                    .push(serde_json::from_str(&json).unwrap());
            }
            Ok(())
        });
        (channel, events)
    }

    fn data_frames(events: &[SerialEvent]) -> Vec<Vec<u8>> {
        events
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Data { data, size, .. } => {
                    assert_eq!(*size, data.len());
                    Some(data.clone())
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn parser_readline_frames_across_split_chunks_without_batching() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(
            channel,
            10_000,
            1024,
            false,
            parser(serde_json::json!({"type": "readline", "delimiter": "\r\n"})),
        );
        let mut routing = HubRoutingState::new("p".into());
        for chunk in [&b"$GPGGA,1"[..], b"23\r", b"\n$GPRMC\r\nPART"] {
            shared.feed_bytes(chunk, &mut routing);
        }
        // Frames are ready at once: no batch timer, nothing left in the batch buffer.
        assert!(routing.combined_buffer.is_empty());
        assert_eq!(
            data_frames(&routing.pending_events),
            vec![b"$GPGGA,123".to_vec(), b"$GPRMC".to_vec()]
        );
    }

    #[test]
    fn parser_inter_byte_timeout_fires_from_tick_without_more_data() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(
            channel,
            10_000,
            1024,
            false,
            parser(serde_json::json!({"type": "interByteTimeout", "interval": 20})),
        );
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"\x01\x02", &mut routing);
        shared.feed_bytes(b"\x03", &mut routing);
        shared.tick("p", &mut routing);
        assert!(routing.pending_events.is_empty(), "silence not over yet");
        let deadline = crate::sync_util::lock_or_recover(&shared.watch)
            .as_ref()
            .and_then(|w| w.framer.as_ref()?.deadline())
            .expect("deadline armed");
        thread::sleep(
            deadline.saturating_duration_since(Instant::now()) + Duration::from_millis(2),
        );
        shared.tick("p", &mut routing);
        assert_eq!(data_frames(&routing.pending_events), vec![vec![1, 2, 3]]);
    }

    #[test]
    fn parser_remainder_flushed_on_detach_and_before_disconnect() {
        let shared = RxHubShared::for_path("p".into());
        let (channel, events) = recording_channel();
        let cfg = serde_json::json!({"type": "readline"});
        shared.attach_watch(channel.clone(), 20, 1024, false, parser(cfg.clone()));
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"done\ntail", &mut routing);
        shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
        shared.detach_watch();
        assert_eq!(
            data_frames(&events.lock().unwrap()),
            vec![b"done".to_vec(), b"tail".to_vec()]
        );

        events.lock().unwrap().clear();
        shared.attach_watch(channel, 20, 1024, false, parser(cfg));
        shared.feed_bytes(b"half", &mut routing);
        shared.emit_disconnect("p", "gone");
        let got = events.lock().unwrap().clone();
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(matches!(&got[0], SerialEvent::Data { data, .. } if data == b"half"));
        assert!(matches!(&got[1], SerialEvent::Disconnect { .. }));
    }

    #[test]
    fn parser_with_route_urc_splits_urc_first() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(
            channel,
            10_000,
            1024,
            true,
            parser(serde_json::json!({"type": "readline", "delimiter": "\r\n"})),
        );
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"+CREG: 0,1\r\nhello\r\nwor", &mut routing);
        shared.feed_bytes(b"ld\r\n", &mut routing);
        let urcs: Vec<_> = routing
            .pending_events
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Urc { line, .. } => Some(line.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(urcs, vec!["+CREG: 0,1"]);
        assert_eq!(
            data_frames(&routing.pending_events),
            vec![b"hello".to_vec(), b"world".to_vec()]
        );
    }

    /// Regression: drain URCs were judged per read chunk, so a `+CMT:` header went out
    /// without the PDU line that arrived in the next chunk.
    #[test]
    fn drain_emits_two_line_urc_together_across_chunks() {
        let shared = Arc::new(RxHubShared::for_path("p".into()));
        let (channel, events) = recording_channel();
        shared.attach_watch(channel, 20, 1024, false, None);
        let bg = shared.clone();
        let handle =
            thread::spawn(move || bg.drain(20, 2_000, Arc::new(AtomicBool::new(false)), vec![]));
        wait_until_drain_slot(&shared);
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"\r\n+CMT: ,23\r\n", &mut routing);
        shared.feed_bytes(b"0791447758100650\r\nOK\r\n", &mut routing);
        thread::sleep(Duration::from_millis(30));
        shared.tick("p", &mut routing);
        handle.join().unwrap().expect("drain");
        let urcs: Vec<String> = events
            .lock()
            .unwrap()
            .iter()
            .filter_map(|e| match e {
                SerialEvent::Urc { line, path } => {
                    assert_eq!(path, "p");
                    Some(line.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(urcs, vec!["+CMT: ,23", "0791447758100650"]);
    }

    #[test]
    fn no_parser_watch_still_batches_until_tick() {
        let shared = RxHubShared::new();
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 0, 1024, false, None);
        let mut routing = HubRoutingState::new("p".into());
        shared.feed_bytes(b"a\nb", &mut routing);
        shared.feed_bytes(b"\nc", &mut routing);
        assert!(routing.pending_events.is_empty());
        assert_eq!(routing.combined_buffer, b"a\nb\nc");
        shared.tick("p", &mut routing);
        assert_eq!(
            data_frames(&routing.pending_events),
            vec![b"a\nb\nc".to_vec()]
        );
    }

    #[test]
    fn cancel_during_active_drain_completes_with_error() {
        let shared = Arc::new(RxHubShared::new());
        let cancel = Arc::new(AtomicBool::new(false));
        let shared_bg = shared.clone();
        let cancel_bg = cancel.clone();
        let handle = thread::spawn(move || shared_bg.drain(10_000, 5_000, cancel_bg, vec![]));
        wait_until_drain_slot(&shared);
        cancel.store(true, Ordering::SeqCst);
        shared.tick("p", &mut HubRoutingState::new("p".into()));
        let err = handle.join().unwrap().expect_err("drain must cancel");
        assert!(err.contains("cancel"), "unexpected: {err}");
    }
}
