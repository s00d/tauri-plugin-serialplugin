//! Shared RX hub state (desktop poll + Android push).

use crate::at::parse::{classify_final_line, is_likely_urc, ExchangeDemux, ExchangeMatch};
use crate::cmux::CmuxSession;
use crate::events::{EventSink, SerialEvent};
use crate::exchange::completion::Completion;
use crate::exchange::options::ResolvedExchangeOptions;
use crate::hub::stream::{call_sink, EventStream, Target, WatchOwner};
use serial_codec::framing::Framer;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

pub(crate) const IDLE_BUFFER_CAP: usize = 64 * 1024;

/// What waits on a port (or CMUX channel) fails with this when it is closed.
pub(crate) const PORT_CLOSED: &str = "port closed";

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
    buffer: Mutex<(Vec<u8>, Completion)>,
    done: ExchangeDone,
    pub cancel: Arc<AtomicBool>,
    /// Set by the first `finish`; later results (a late cancel / failure) are ignored.
    finished: AtomicBool,
}

impl ExchangeWaiter {
    pub fn new(options: ResolvedExchangeOptions, cancel: Arc<AtomicBool>) -> Arc<Self> {
        Arc::new(Self {
            buffer: Mutex::new((Vec::new(), Completion::new(&options))),
            options,
            done: Arc::new((Mutex::new(None), Condvar::new())),
            cancel,
            finished: AtomicBool::new(false),
        })
    }

    pub fn push_bytes(&self, chunk: &[u8]) {
        let mut guard = crate::sync_util::lock_or_recover(&self.buffer);
        let (buffer, completion) = &mut *guard;
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
        if let Some(matched) = completion.push(buffer, chunk.len()) {
            let raw = std::mem::take(buffer);
            *completion = Completion::new(&self.options);
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

    /// The first result wins: a reply that already completed is not replaced by a
    /// cancel or failure arriving after it.
    fn finish(&self, result: Result<(Vec<u8>, ExchangeMatch), String>) {
        let (lock, cvar) = &*self.done;
        let mut guard = crate::sync_util::lock_or_recover(lock);
        if !self.finished.swap(true, Ordering::SeqCst) {
            *guard = Some(result);
        }
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

/// An event and the sink it was produced for (captured when it was produced, so an
/// `unwatch` racing the hub cannot drop frames that were already decoded).
pub type QueuedEvent = (EventSink, SerialEvent);

pub use crate::hub::stream::Outbox;

/// 4.0.0 API (feature `tauri`): [`emit_urc_to`] for a Tauri channel.
#[cfg(feature = "tauri")]
pub fn emit_urc(channel: &tauri::ipc::Channel<SerialEvent>, path: &str, line: &str) {
    let _ = channel.send(SerialEvent::Urc {
        path: path.to_string(),
        line: line.to_string(),
    });
}

pub fn emit_urc_to(channel: &crate::events::EventSink, path: &str, line: &str) {
    let event = SerialEvent::Urc {
        path: path.to_string(),
        line: line.to_string(),
    };
    let _ = call_sink(channel, event);
}

/// Per-port routing state shared between desktop poll loop and Android push feed.
///
/// Since 4.2 each watch owns its line router and batch, and events are queued in the hub as
/// they are produced: [`Self::line_router`], [`Self::combined_buffer`], [`Self::flush_at`]
/// and [`Self::pending_events`] stay for the 4.x API only. The hub neither fills nor reads
/// them: [`RxHubShared::deliver_queued`] delivers what a routing call produced.
pub struct HubRoutingState {
    pub path: String,
    /// Not used any more.
    pub line_router: LineRouter,
    /// URC demux for the exchange it was built for; rebuilt when the waiter changes.
    pub exchange_demux: Option<(Arc<ExchangeWaiter>, ExchangeDemux)>,
    /// Not used any more.
    pub combined_buffer: Vec<u8>,
    /// Not used any more.
    pub flush_at: Instant,
    /// Not filled any more; [`RxHubShared::dispatch_queued_events`] still delivers what it
    /// is given, after the hub's own events.
    pub pending_events: Vec<QueuedEvent>,
}

impl HubRoutingState {
    pub fn new(path: String) -> Self {
        Self {
            path,
            line_router: LineRouter::default(),
            exchange_demux: None,
            combined_buffer: Vec::new(),
            flush_at: Instant::now(),
            pending_events: Vec::new(),
        }
    }
}

/// One watch (the old one, or a [`SerialManager::watch_core`](crate::SerialManager::watch_core)
/// stream): its own line router, batch buffer and parser over the hub's byte stream, so one
/// watch's state never affects another's and all of it goes out when the watch ends.
///
/// With `route_urc` the URC split runs before the parser, which sees only the data lines:
/// `onUrc` keeps its meaning whatever the parser, and a parser never has to know AT syntax.
/// The line router's unterminated tail follows on the batch timer.
pub(crate) struct CoreWatchSlot {
    pub(crate) id: u64,
    pub(crate) target: Target,
    pub(crate) batch_timeout_ms: u64,
    pub(crate) read_size: usize,
    pub(crate) route_urc: bool,
    pub(crate) framer: Option<Box<dyn Framer>>,
    pub(crate) line_router: LineRouter,
    pub(crate) buffer: Vec<u8>,
    pub(crate) flush_at: Instant,
}

impl CoreWatchSlot {
    /// `id` 0: the old watch.
    pub(crate) fn new(
        id: u64,
        target: Target,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
        framer: Option<Box<dyn Framer>>,
    ) -> Self {
        Self {
            id,
            target,
            batch_timeout_ms,
            read_size,
            route_urc,
            framer,
            line_router: LineRouter::default(),
            buffer: Vec::new(),
            flush_at: Instant::now(),
        }
    }

    /// Bytes past the URC split: into the parser (one event per frame) or the batch.
    fn sink_bytes(&mut self, path: &str, bytes: &[u8], out: &mut Vec<SerialEvent>) {
        match self.framer.as_mut() {
            Some(f) => {
                let mut frames = Vec::new();
                f.push(bytes, Instant::now(), &mut frames);
                push_frames(path, frames, out);
            }
            None => self.buffer.extend_from_slice(bytes),
        }
    }

    fn feed(&mut self, path: &str, chunk: &[u8], out: &mut Vec<SerialEvent>) {
        if !self.route_urc {
            self.sink_bytes(path, chunk, out);
            return;
        }
        for action in self.line_router.route_streaming(chunk, &[]) {
            match action {
                RxRouteAction::UrcLine(line) => out.push(SerialEvent::Urc {
                    path: path.to_string(),
                    line,
                }),
                RxRouteAction::StreamData(bytes) => self.sink_bytes(path, &bytes, out),
            }
        }
    }

    fn tick(&mut self, path: &str, out: &mut Vec<SerialEvent>) {
        let timer = self.flush_at.elapsed() >= Duration::from_millis(self.batch_timeout_ms);
        if self.framer.is_some() {
            if timer {
                self.flush_at = Instant::now();
                let partial = self.line_router.take_partial();
                if !partial.is_empty() {
                    self.sink_bytes(path, &partial, out);
                }
            }
            let mut frames = Vec::new();
            if let Some(f) = self.framer.as_mut() {
                f.poll(Instant::now(), &mut frames);
            }
            push_frames(path, frames, out);
            return;
        }
        if timer || self.buffer.len() >= self.read_size.max(1) {
            self.flush_at = Instant::now();
            self.flush_buffers(path, out);
        }
    }

    /// Line-router tail and batch out now (parser state kept).
    fn flush_buffers(&mut self, path: &str, out: &mut Vec<SerialEvent>) {
        let partial = self.line_router.take_partial();
        if !partial.is_empty() {
            self.sink_bytes(path, &partial, out);
        }
        let data = std::mem::take(&mut self.buffer);
        if !data.is_empty() {
            out.push(data_event(path, data));
        }
    }

    /// End of stream: batch, then the parser's remainder.
    fn finish(&mut self, path: &str, out: &mut Vec<SerialEvent>) {
        self.flush_buffers(path, out);
        if let Some(rest) = self.framer.as_mut().and_then(|f| f.flush()) {
            out.push(data_event(path, rest));
        }
    }

    /// [`Self::finish`], then `last`: nothing more is sent.
    fn end(&mut self, path: &str, last: Option<&SerialEvent>) {
        let mut out = Vec::new();
        self.finish(path, &mut out);
        out.extend(last.cloned());
        self.target.end(&mut out);
    }
}

fn data_event(path: &str, data: Vec<u8>) -> SerialEvent {
    SerialEvent::Data {
        path: path.to_string(),
        size: data.len(),
        data,
    }
}

fn push_frames(path: &str, frames: Vec<Vec<u8>>, out: &mut Vec<SerialEvent>) {
    out.extend(frames.into_iter().map(|f| data_event(path, f)));
}

pub(crate) struct DrainSlot {
    /// Whose slot this is: a caller that gives up takes only its own.
    pub(crate) id: u64,
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
    /// See [`DrainSlot::id`].
    pub(crate) id: u64,
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
    pub(crate) watch: Mutex<Option<CoreWatchSlot>>,
    pub(crate) drain: Mutex<Option<DrainSlot>>,
    pub(crate) read_slot: Mutex<Option<ReadSlot>>,
    pub(crate) idle: Mutex<Vec<u8>>,
    pub(crate) cmux: Mutex<Option<Arc<CmuxSession>>>,
    /// `watch_core` streams (any number, beside the one old watch).
    pub(crate) core_watches: Mutex<Vec<CoreWatchSlot>>,
    /// Set by an intentional close: a read error from now on is not a `Disconnect`, and
    /// waiters fail at once.
    closing: AtomicBool,
    /// `Disconnect` went out (it is sent once, even when two paths report the loss).
    disconnected: AtomicBool,
    outbox: Outbox,
    next_slot: AtomicU64,
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
            core_watches: Mutex::new(Vec::new()),
            closing: AtomicBool::new(false),
            disconnected: AtomicBool::new(false),
            outbox: Outbox::default(),
            next_slot: AtomicU64::new(0),
        }
    }

    /// 4.0.0 API (feature `tauri`): [`Self::attach_watch_with`] for a Tauri channel, no parser.
    #[cfg(feature = "tauri")]
    pub fn attach_watch(
        &self,
        channel: tauri::ipc::Channel<SerialEvent>,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
    ) {
        let sink = crate::events::channel_sink(channel);
        self.attach_watch_with(sink, batch_timeout_ms, read_size, route_urc, None);
    }

    pub fn attach_watch_with(
        &self,
        channel: crate::events::EventSink,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
        framer: Option<Box<dyn Framer>>,
    ) {
        self.attach_stream(channel, batch_timeout_ms, read_size, route_urc, framer);
    }

    /// [`Self::attach_watch_with`]; the stream is the watch's identity. A watch it replaces
    /// ends with its tail queued.
    pub(crate) fn attach_stream(
        &self,
        channel: EventSink,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
        framer: Option<Box<dyn Framer>>,
    ) -> Arc<EventStream> {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        let stream = self.outbox.stream(channel);
        let slot = CoreWatchSlot::new(
            0,
            Target::Old(stream.clone()),
            batch_timeout_ms,
            read_size,
            route_urc,
            framer,
        );
        let mut watch = crate::sync_util::lock_or_recover(&self.watch);
        let mut replaced = watch.replace(slot);
        if let Some(old) = replaced.as_mut() {
            old.end(&self.path, None);
        }
        // Unlocked before the replaced watch's sink is dropped.
        drop(watch);
        stream
    }

    /// Take the old watch (if `which` accepts it); its tail is queued.
    fn take_watch(&self, which: impl Fn(&CoreWatchSlot) -> bool) -> Option<CoreWatchSlot> {
        let mut watch = crate::sync_util::lock_or_recover(&self.watch);
        if !watch.as_ref().is_some_and(which) {
            return None;
        }
        let mut slot = watch.take();
        if let Some(slot) = slot.as_mut() {
            slot.end(&self.path, None);
        }
        slot
    }

    /// Stop the watch; its batch, its unfinished line and its parser's remainder go out
    /// last, after everything already decoded, before this returns (a sink that calls it
    /// does not wait for another thread's delivery). Call with no lock held (the sink runs
    /// here); under locks use [`Self::detach_watch_queued`] + [`Self::deliver_queued`].
    pub fn detach_watch(&self) {
        if let Some(CoreWatchSlot {
            target: Target::Old(stream),
            ..
        }) = self.take_watch(|_| true)
        {
            self.outbox.settle(&stream);
        }
    }

    /// [`Self::detach_watch`] without running the sink: its last events are queued.
    pub fn detach_watch_queued(&self) {
        self.take_watch(|_| true);
    }

    /// Add a `watch_core` stream (bytes from now on; idle bytes are dropped, as on attach).
    pub(crate) fn attach_core_watch(&self, slot: CoreWatchSlot) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        crate::sync_util::lock_or_recover(&self.core_watches).push(slot);
    }

    /// Remove a `watch_core` stream; its batch and parser remainder go out last.
    /// `false` when no such stream.
    pub(crate) fn detach_core_watch(&self, id: u64) -> bool {
        let mut cores = crate::sync_util::lock_or_recover(&self.core_watches);
        let Some(i) = cores.iter().position(|c| c.id == id) else {
            return false;
        };
        let mut slot = cores.remove(i);
        slot.end(&self.path, None);
        drop(cores);
        true
    }

    pub fn has_core_watches(&self) -> bool {
        !crate::sync_util::lock_or_recover(&self.core_watches).is_empty()
    }

    /// An intentional close begins: no `Disconnect` from here on, and what waits (or starts
    /// to wait) fails, so a sink in an exchange does not hold up the close.
    pub(crate) fn begin_close(&self) {
        self.closing.store(true, Ordering::SeqCst);
        self.fail_waiters(PORT_CLOSED);
    }

    /// Called by whoever installed a waiter, drain or read slot.
    fn fail_if_closing(&self) {
        if self.is_closing() {
            self.fail_waiters(PORT_CLOSED);
        }
    }

    /// The port is neither being closed nor lost.
    pub(crate) fn check_open(&self) -> Result<(), String> {
        if self.is_closing() {
            return Err(PORT_CLOSED.into());
        }
        if self.disconnected.load(Ordering::SeqCst) {
            return Err("Serial port disconnected".into());
        }
        Ok(())
    }

    pub(crate) fn is_closing(&self) -> bool {
        self.closing.load(Ordering::SeqCst)
    }

    /// Run `f` over the old watch, then the `watch_core` streams (those whose sink is gone
    /// are dropped first); what it produces is sent under the watch's lock, so nothing of a
    /// watch is queued once it is detached. `false`: there was no watch.
    fn each_watch(&self, mut f: impl FnMut(&mut CoreWatchSlot, &mut Vec<SerialEvent>)) -> bool {
        let mut events = Vec::new();
        let mut any = false;
        let mut visit = |slot: &mut CoreWatchSlot| {
            any = true;
            f(slot, &mut events);
            slot.target.send(&mut events);
        };
        let gone = {
            let mut watch = crate::sync_util::lock_or_recover(&self.watch);
            match watch.as_mut() {
                Some(slot) if !slot.target.gone() => {
                    visit(slot);
                    None
                }
                _ => watch.take(),
            }
        };
        // Unlocked: it drops the sink.
        drop(gone);
        let mut cores = crate::sync_util::lock_or_recover(&self.core_watches);
        cores.retain(|c| !c.target.gone());
        cores.iter_mut().for_each(visit);
        any
    }

    /// End every watch: its tail, then `last`.
    fn end_watches(&self, path: &str, last: Option<&SerialEvent>) {
        let old = {
            let mut watch = crate::sync_util::lock_or_recover(&self.watch);
            let mut old = watch.take();
            if let Some(slot) = old.as_mut() {
                slot.end(path, last);
            }
            old
        };
        drop(old);
        let mut cores = crate::sync_util::lock_or_recover(&self.core_watches);
        for mut slot in cores.drain(..) {
            slot.end(path, last);
        }
    }

    /// Queue copies of `event` for the old watch and every `watch_core` stream (`urc_only`:
    /// only those with `route_urc`).
    fn fan_out(&self, event: &SerialEvent, urc_only: bool) {
        self.each_watch(|c, out| {
            // Id 0: the old watch gets URCs of exchanges and drains either way (4.x).
            if c.id == 0 || !urc_only || c.route_urc {
                out.push(event.clone());
            }
        });
    }

    /// Earliest parser timer of all watches (the hub wakes for it).
    pub(crate) fn next_framer_deadline(&self) -> Option<Instant> {
        let old = crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .and_then(|w| w.framer.as_ref()?.deadline());
        let core = crate::sync_util::lock_or_recover(&self.core_watches)
            .iter()
            .filter_map(|c| c.framer.as_ref()?.deadline())
            .min();
        match (old, core) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// Read chunk size: the old watch's, else the largest `watch_core` one.
    pub(crate) fn read_size(&self) -> Option<usize> {
        if let Some(w) = crate::sync_util::lock_or_recover(&self.watch).as_ref() {
            return Some(w.read_size);
        }
        crate::sync_util::lock_or_recover(&self.core_watches)
            .iter()
            .map(|c| c.read_size)
            .max()
    }

    /// Deliver events queued by `*_queued` calls. Call with no lock held.
    pub fn deliver_queued(&self) {
        self.outbox.deliver();
    }

    /// Queue `event` for the current watch (if any) and deliver.
    pub fn emit(&self, event: SerialEvent) {
        self.fan_out(&event, false);
        self.outbox.deliver();
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
        drop(idle);
        self.fail_if_closing();
    }

    pub fn clear_exchange_waiter(&self) {
        *crate::sync_util::lock_or_recover(&self.exchange_waiter) = None;
    }

    /// Wake an in-flight exchange waiter when [`SerialManager::cancel_exchange`](crate::SerialManager::cancel_exchange) is invoked.
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
        // Bound first: the guard must not live through the session's sink calls.
        let session = crate::sync_util::lock_or_recover(&self.cmux).clone();
        if let Some(session) = session {
            session.feed_physical_rx(chunk);
            return;
        }
        if route_drain_chunk(self, chunk) {
            return;
        }
        if let Some(waiter) = crate::sync_util::lock_or_recover(&self.exchange_waiter).clone() {
            route_exchange_chunk(self, chunk, state, waiter);
            return;
        }
        // A read takes what fits; the rest of the chunk goes on to the next consumer.
        let chunk = match route_read_slot_chunk(self, chunk) {
            Some(taken) if taken == chunk.len() => return,
            Some(taken) => &chunk[taken..],
            None => chunk,
        };
        state.exchange_demux = None;
        if !self.each_watch(|c, out| c.feed(&state.path, chunk, out)) {
            push_idle(self, chunk);
        }
    }

    /// Idle timers for push model: drain completion + watch batch flush + read deadlines.
    pub fn tick(&self, path: &str, _state: &mut HubRoutingState) {
        tick_read_slot(self);
        try_complete_drain(self);
        let session = crate::sync_util::lock_or_recover(&self.cmux).clone();
        if let Some(session) = session {
            session.tick();
        }
        self.each_watch(|c, out| c.tick(path, out));
    }

    /// Immediately fail exchange waiters, active drain, and pending read (USB error teardown).
    pub fn fail_all_waiters(&self, reason: &str) {
        self.fail_waiters(reason);
        self.outbox.deliver();
    }

    /// [`Self::fail_all_waiters`] without running a sink (the URCs of a drain are queued).
    pub(crate) fn fail_waiters(&self, reason: &str) {
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

    /// Drop bytes the hub holds for no one yet (idle RX, a pending read's partial fill).
    pub fn purge_buffers(&self) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).as_mut() {
            slot.buffer.clear();
        }
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
        if self.has_watch() || self.has_core_watches() {
            return Err("Cannot read while watch is active; use watch or exchange".into());
        }

        let max_bytes = max_bytes.max(1);
        let wait = Duration::from_millis(timeout_ms);
        let deadline = Instant::now() + wait;

        // Idle hand-off and slot install happen under one idle lock: a chunk routed in
        // between would otherwise land in `idle` behind an installed slot (see `push_idle`).
        let (tx, rx) = mpsc::sync_channel(1);
        let id = self.next_slot.fetch_add(1, Ordering::Relaxed);
        {
            let mut idle = crate::sync_util::lock_or_recover(&self.idle);
            let mut guard = crate::sync_util::lock_or_recover(&self.read_slot);
            if guard.is_some() {
                return Err("read already in progress".into());
            }
            let mut initial = Vec::new();
            if !idle.is_empty() {
                let n = max_bytes.min(idle.len());
                if !fill {
                    return Ok(idle.drain(..n).collect());
                }
                initial.extend(idle.drain(..n));
                if initial.len() >= max_bytes {
                    return Ok(initial);
                }
            }
            *guard = Some(ReadSlot {
                id,
                max_bytes,
                fill,
                timeout_ms,
                buffer: initial,
                deadline,
                tx,
            });
        }

        self.fail_if_closing();

        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                let mine = crate::sync_util::lock_or_recover(&self.read_slot)
                    .take_if(|slot| slot.id == id);
                if let Some(slot) = mine {
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

    /// Batched bytes of the old watch.
    pub fn pending_watch_bytes(&self, _state: &HubRoutingState) -> usize {
        crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .map_or(0, |w| w.buffer.len())
    }

    /// Batches and unfinished lines out now (parser state kept).
    pub fn flush_watch_now(&self, state: &mut HubRoutingState) {
        self.each_watch(|c, out| c.flush_buffers(&state.path, out));
    }

    /// Send `Disconnect` on the watches, after their batch and parser's remainder: their
    /// last event. CMUX channel watches get the same on their path, and what waits on a
    /// channel fails before any sink runs.
    /// Once per hub: a second call (Android reports a USB failure from the hub loop
    /// and from `fail_port`) does nothing.
    pub fn emit_disconnect(&self, path: &str, reason: &str) {
        if self.disconnected.swap(true, Ordering::SeqCst) {
            return;
        }
        let session = crate::sync_util::lock_or_recover(&self.cmux).clone();
        if let Some(session) = &session {
            session.disconnect_queued(reason);
        }
        let event = SerialEvent::Disconnect {
            path: path.to_string(),
            reason: reason.to_string(),
        };
        self.end_watches(path, Some(&event));
        self.outbox.deliver();
        if let Some(session) = session {
            session.deliver_queued();
        }
    }

    /// Port going away: the watches end, what they still hold is queued.
    pub fn finish_watch_stream(&self, state: &mut HubRoutingState) {
        self.end_watches(&state.path, None);
    }

    pub fn has_watch(&self) -> bool {
        crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .is_some_and(|w| !w.target.gone())
    }

    /// 4.0.0 API (feature `tauri`): deliver `events` to the current watch.
    #[cfg(feature = "tauri")]
    pub fn dispatch_pending_events(&self, events: Vec<SerialEvent>) {
        self.each_watch(|c, out| {
            if c.id == 0 {
                out.extend(events.iter().cloned());
            }
        });
        self.outbox.deliver();
    }

    /// Deliver the events the hub queued, then `events`. Each goes to the sink it was
    /// produced for, even if the watch was detached since. Call with no lock held, after
    /// every routing call.
    pub fn dispatch_queued_events(&self, events: Vec<QueuedEvent>) {
        self.outbox.deliver();
        for (sink, event) in events {
            let _ = call_sink(&sink, event);
        }
    }

    /// Everything queued so far has been delivered when this returns (in a sink: unless
    /// another thread is delivering it). Call with no lock held.
    pub(crate) fn settle_events(&self) {
        self.outbox.settle_all();
    }

    /// Soft-drain via the hub thread (single reader); URC lines are emitted on the watch channel.
    /// Bytes already in the idle buffer are dropped with the drained ones (a late reply to
    /// a timed-out exchange is exactly what a drain throws away).
    pub fn drain(
        &self,
        idle_ms: u64,
        max_ms: u64,
        cancel: Arc<AtomicBool>,
        solicited_prefixes: Vec<String>,
    ) -> Result<Vec<u8>, String> {
        let wait = Duration::from_millis(max_ms.saturating_add(500));
        let (tx, rx) = mpsc::sync_channel(1);
        let id = self.next_slot.fetch_add(1, Ordering::Relaxed);
        {
            // Under the idle lock (order: idle, then drain): a chunk `feed_bytes` routes
            // to idle while the slot goes in lands either before (cleared here) or after
            // (`push_idle` hands it to the slot), never in idle behind the drain.
            let mut idle = crate::sync_util::lock_or_recover(&self.idle);
            let mut guard = crate::sync_util::lock_or_recover(&self.drain);
            if guard.is_some() {
                return Err("drain already in progress".into());
            }
            idle.clear();
            *guard = Some(DrainSlot {
                id,
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

        self.fail_if_closing();

        match rx.recv_timeout(wait) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => {
                let mine =
                    crate::sync_util::lock_or_recover(&self.drain).take_if(|slot| slot.id == id);
                if let Some(slot) = mine {
                    queue_drain_urcs(self, &slot);
                    self.outbox.deliver();
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
    chunk: &[u8],
    state: &mut HubRoutingState,
    waiter: Arc<ExchangeWaiter>,
) {
    let stale = !matches!(&state.exchange_demux, Some((w, _)) if Arc::ptr_eq(w, &waiter));
    if stale {
        let cmd = waiter.options.command.clone().unwrap_or_default();
        let demux = ExchangeDemux::with_format(
            &cmd,
            &waiter.options.solicited_prefixes,
            waiter.options.result_format,
        );
        state.exchange_demux = Some((waiter.clone(), demux));
    }
    if let Some((_, demux)) = state.exchange_demux.as_mut() {
        for line in demux.process_chunk(chunk) {
            let event = SerialEvent::Urc {
                path: state.path.clone(),
                line,
            };
            shared.fan_out(&event, true);
        }
    }
    waiter.push_bytes(chunk);
}

/// Feed a read slot: the bytes it took, or `None` if there is no slot (fall through).
/// A read takes at most its size; the caller routes the rest of the chunk on (they
/// were once dropped).
pub(crate) fn route_read_slot_chunk(shared: &RxHubShared, chunk: &[u8]) -> Option<usize> {
    let taken;
    let completed = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let ready = {
            let slot = guard.as_mut()?;
            let remaining = slot.max_bytes.saturating_sub(slot.buffer.len());
            taken = chunk.len().min(remaining);
            slot.buffer.extend_from_slice(&chunk[..taken]);
            !slot.fill || slot.buffer.len() >= slot.max_bytes
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
    Some(taken)
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
        kind.map(|k| (k, guard.take().unwrap()))
    };
    // The drain lock is released: the URC sink may touch the drain slot.
    if let Some((k, d)) = completed {
        queue_drain_urcs(shared, &d);
        shared.outbox.deliver();
        let result = match k {
            DrainCompleteKind::Cancel => Err("exchange cancelled".into()),
            DrainCompleteKind::Buffer => Ok(d.buffer),
            DrainCompleteKind::Empty => Ok(Vec::new()),
        };
        let _ = d.tx.send(result);
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
    // A drain, waiter or read slot may have been installed after `feed_bytes` checked
    // for one (lock order: idle, then drain / exchange_waiter / read_slot). A drain
    // first, as in `feed_bytes`: idle bytes left behind it would reach the next exchange.
    if route_drain_chunk(shared, chunk) {
        return;
    }
    if let Some(waiter) = crate::sync_util::lock_or_recover(&shared.exchange_waiter).as_ref() {
        waiter.push_bytes(chunk);
        return;
    }
    // Same for a read slot installed after `feed_bytes` checked for one.
    let chunk = match route_read_slot_chunk(shared, chunk) {
        Some(taken) => &chunk[taken..],
        None => chunk,
    };
    idle.extend_from_slice(chunk);
    let excess = idle.len().saturating_sub(IDLE_BUFFER_CAP);
    idle.drain(..excess);
}

/// End the drain with `result`; URCs it collected are queued for the watch.
pub(crate) fn finish_drain(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    let drain = crate::sync_util::lock_or_recover(&shared.drain).take();
    if let Some(drain) = drain {
        queue_drain_urcs(shared, &drain);
        let _ = drain.tx.send(result);
    }
}

/// Queue the URC lines of a finished drain for the watch channel (if any).
fn queue_drain_urcs(shared: &RxHubShared, drain: &DrainSlot) {
    if drain.buffer.is_empty() {
        return;
    }
    if !shared.has_watch() && !shared.has_core_watches() {
        return;
    }
    let lines = crate::at::parse::urc_lines_from_drain(&drain.buffer, &drain.solicited_prefixes);
    for line in lines {
        let event = SerialEvent::Urc {
            path: shared.path.clone(),
            line,
        };
        shared.fan_out(&event, true);
    }
}

impl WatchOwner for RxHubShared {
    fn stop(&self, stream: &Arc<EventStream>) {
        // The slot (and with it the sink) is dropped with no lock held.
        drop(self.take_watch(|slot| slot.target.is(stream)));
        self.outbox.settle(stream);
    }
}

#[cfg(test)]
mod model;
