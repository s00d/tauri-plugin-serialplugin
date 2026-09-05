//! Shared RX hub state (desktop poll + Android push).

use crate::at::parse::{classify_final_line, is_likely_urc, ExchangeDemux, ExchangeMatch};
use crate::cmux::CmuxSession;
use crate::events::SerialEvent;
use crate::exchange::completion::check_exchange_complete;
use crate::exchange::options::ResolvedExchangeOptions;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

pub(crate) const IDLE_BUFFER_CAP: usize = 64 * 1024;

/// Bounded grace window `recover_or_timeout()` waits for a `done` result
/// after losing the `read_slot` reclaim race. Deliberately independent of
/// the read's own `timeout_ms`/deadline — by the time this runs, that
/// deadline has already elapsed, so there is no "remaining" budget left to
/// derive a value from. This is a second, much shorter window covering
/// only the race itself: the hub thread has already taken the slot and is
/// in the middle of posting its result, which takes microseconds under
/// normal scheduling (no I/O in that path). Not a full elimination of the
/// race under pathological scheduler starvation, but any finite bound has
/// that same limit.
const READ_SLOT_RACE_RECOVERY_WINDOW: Duration = Duration::from_millis(50);

type ExchangeDone = Arc<(
    Mutex<Option<Result<(Vec<u8>, ExchangeMatch), String>>>,
    Condvar,
)>;
type DrainDone = Arc<(Mutex<Option<Result<Vec<u8>, String>>>, Condvar)>;
type ReadDone = Arc<(Mutex<Option<Result<Vec<u8>, String>>>, Condvar)>;

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

/// Line-oriented router for streaming when no exchange is active.
#[derive(Debug, Default)]
pub struct LineRouter {
    partial: String,
}

impl LineRouter {
    pub fn route_streaming(
        &mut self,
        chunk: &[u8],
        solicited_prefixes: &[String],
    ) -> Vec<RxRouteAction> {
        let text = String::from_utf8_lossy(chunk);
        self.partial.push_str(&text);
        let mut actions = Vec::new();
        while let Some(pos) = self.partial.find('\n') {
            let line = self.partial[..pos]
                .trim()
                .trim_end_matches('\r')
                .to_string();
            self.partial.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if is_likely_urc(&line, solicited_prefixes) && classify_final_line(&line).is_none() {
                actions.push(RxRouteAction::UrcLine(line));
            } else {
                actions.push(RxRouteAction::StreamData(line.into_bytes()));
            }
        }
        if !self.partial.is_empty() {
            actions.push(RxRouteAction::StreamData(self.partial.as_bytes().to_vec()));
            self.partial.clear();
        }
        actions
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
    pub exchange_demux: Option<ExchangeDemux>,
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
}

pub(crate) struct DrainSlot {
    pub(crate) idle_ms: u64,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) buffer: Vec<u8>,
    pub(crate) last_byte_at: Option<Instant>,
    pub(crate) started_at: Instant,
    pub(crate) deadline: Instant,
    pub(crate) solicited_prefixes: Vec<String>,
    pub(crate) done: DrainDone,
}

pub(crate) struct ReadSlot {
    pub(crate) max_bytes: usize,
    pub(crate) fill: bool,
    pub(crate) timeout_ms: u64,
    pub(crate) buffer: Vec<u8>,
    pub(crate) deadline: Instant,
    pub(crate) done: ReadDone,
}

/// Shared hub state between the RX thread and API handlers.
pub struct RxHubShared {
    pub(crate) exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    pub(crate) watch: Mutex<Option<WatchSlot>>,
    pub(crate) drain: Mutex<Option<DrainSlot>>,
    pub(crate) read_slot: Mutex<Option<ReadSlot>>,
    pub(crate) idle: Mutex<Vec<u8>>,
    pub(crate) cmux: Mutex<Option<Arc<CmuxSession>>>,
}

impl Default for RxHubShared {
    fn default() -> Self {
        Self::new()
    }
}

impl RxHubShared {
    pub fn new() -> Self {
        Self {
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
    ) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        *crate::sync_util::lock_or_recover(&self.watch) = Some(WatchSlot {
            channel,
            batch_timeout_ms,
            read_size,
        });
    }

    pub fn detach_watch(&self) {
        *crate::sync_util::lock_or_recover(&self.watch) = None;
    }

    pub fn attach_cmux(&self, session: Arc<CmuxSession>) {
        *crate::sync_util::lock_or_recover(&self.cmux) = Some(session);
    }

    pub fn detach_cmux(&self) {
        *crate::sync_util::lock_or_recover(&self.cmux) = None;
    }

    pub fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>) {
        *crate::sync_util::lock_or_recover(&self.exchange_waiter) = Some(waiter);
    }

    pub fn clear_exchange_waiter(&self) {
        *crate::sync_util::lock_or_recover(&self.exchange_waiter) = None;
    }

    /// Wake an in-flight exchange waiter when [`cancel_exchange`] is invoked.
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
        // Try drain without a separate is_some() check — that races with reclaim
        // and route_drain_chunk used to drop bytes when the slot disappeared.
        if route_drain_chunk(self, &path, chunk) {
            return;
        }
        if let Some(waiter) = crate::sync_util::lock_or_recover(&self.exchange_waiter).clone() {
            route_exchange_chunk(self, &path, chunk, state, waiter);
            return;
        }
        if crate::sync_util::lock_or_recover(&self.read_slot).is_some() {
            route_read_slot_chunk(self, chunk);
            return;
        }
        if self.has_watch() {
            route_watch_chunk(&path, chunk, state);
            return;
        }
        push_idle(self, chunk);
    }

    /// Idle timers for push model: drain completion + watch batch flush + read deadlines.
    pub fn tick(&self, path: &str, state: &mut HubRoutingState) {
        tick_read_slot(self);
        let completed = {
            let mut guard = crate::sync_util::lock_or_recover(&self.drain);
            #[derive(Clone, Copy)]
            enum Kind {
                Cancel,
                Buffer,
                Empty,
            }
            let kind = match guard.as_ref() {
                None => None,
                Some(d) if d.cancel.load(Ordering::SeqCst) => Some(Kind::Cancel),
                Some(d) if Instant::now() >= d.deadline => Some(Kind::Buffer),
                Some(d)
                    if d.last_byte_at
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(d.idle_ms)) =>
                {
                    Some(Kind::Buffer)
                }
                Some(d)
                    if d.last_byte_at.is_none()
                        && d.started_at.elapsed() >= Duration::from_millis(d.idle_ms) =>
                {
                    Some(Kind::Empty)
                }
                _ => None,
            };
            kind.map(|k| {
                let d = guard.take().unwrap();
                let result = match k {
                    Kind::Cancel => Err("exchange cancelled".into()),
                    Kind::Buffer => Ok(d.buffer),
                    Kind::Empty => Ok(Vec::new()),
                };
                (d.done, result)
            })
        };
        if let Some((done, result)) = completed {
            wake_done(&done, result);
        }

        let batch_timeout_ms = self
            .watch
            .lock()
            .unwrap()
            .as_ref()
            .map(|w| w.batch_timeout_ms)
            .unwrap_or(1000);
        if state.flush_at.elapsed() >= Duration::from_millis(batch_timeout_ms) {
            state.flush_at = Instant::now();
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
        let read_len = self
            .read_slot
            .lock()
            .unwrap()
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
        if crate::sync_util::lock_or_recover(&self.read_slot).is_some() {
            return Err("read already in progress".into());
        }

        let max_bytes = max_bytes.max(1);
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);

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

        let done = Arc::new((Mutex::new(None), Condvar::new()));
        {
            *crate::sync_util::lock_or_recover(&self.read_slot) = Some(ReadSlot {
                max_bytes,
                fill,
                timeout_ms,
                buffer: initial,
                deadline,
                done: done.clone(),
            });
        }

        let (lock, cvar) = &*done;
        let mut guard = crate::sync_util::lock_or_recover(lock);
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Deadlock fix: drop the `done` mutex guard before locking
                // `read_slot` — finish_read_slot() locks the same two mutexes
                // in the opposite order (read_slot then done), so holding both
                // here at once is an ABBA lock inversion.
                drop(guard);
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).take() {
                    if slot.buffer.is_empty() {
                        return Err(format!("no data received within {} ms", timeout_ms));
                    }
                    return Ok(slot.buffer);
                }
                return Self::recover_or_timeout(lock, cvar, timeout_ms);
            }
            let (g, timeout_result) = cvar
                .wait_timeout(guard, remaining)
                .map_err(|e| e.to_string())?;
            guard = g;
            if guard.is_none() && timeout_result.timed_out() && Instant::now() >= deadline {
                // Same ABBA fix as above.
                drop(guard);
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).take() {
                    if slot.buffer.is_empty() {
                        return Err(format!("no data received within {} ms", timeout_ms));
                    }
                    return Ok(slot.buffer);
                }
                return Self::recover_or_timeout(lock, cvar, timeout_ms);
            }
        }
        guard.take().unwrap()
    }

    /// Re-check `done` after losing the `read_slot` reclaim race on a
    /// timeout path: dropping the `done` guard to avoid the ABBA deadlock
    /// with `finish_read_slot()` opens a window where the hub thread can
    /// claim the slot and post its result to `done` before we re-lock
    /// `read_slot` and find it already empty. Wait briefly for that result
    /// instead of discarding it as a spurious timeout.
    fn recover_or_timeout(
        lock: &Mutex<Option<Result<Vec<u8>, String>>>,
        cvar: &Condvar,
        timeout_ms: u64,
    ) -> Result<Vec<u8>, String> {
        Self::recover_or_timeout_within(lock, cvar, timeout_ms, READ_SLOT_RACE_RECOVERY_WINDOW)
    }

    // `window` is a parameter (not always `READ_SLOT_RACE_RECOVERY_WINDOW`)
    // so tests can use a much wider margin than production's 50ms — a slow
    // CI runner preempting a spawned thread past a tight window would
    // otherwise make timing-based tests flaky independent of whether the
    // production code is correct.
    fn recover_or_timeout_within(
        lock: &Mutex<Option<Result<Vec<u8>, String>>>,
        cvar: &Condvar,
        timeout_ms: u64,
        window: Duration,
    ) -> Result<Vec<u8>, String> {
        let mut guard = crate::sync_util::lock_or_recover(lock);
        let deadline = Instant::now() + window;
        // Loop, not a single wait: `Condvar::wait_timeout` can return on a
        // spurious wakeup with `guard` still `None` (no OS/libc condvar
        // guarantees against this) — a single wait would then treat that
        // spurious wakeup as "recovery window over, no result", discarding
        // a real result that shows up a moment later, the same class of
        // bug this function exists to fix.
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            let (g, _) = cvar
                .wait_timeout(guard, remaining)
                .unwrap_or_else(|e| e.into_inner());
            guard = g;
        }
        match guard.take() {
            Some(result) => result,
            None => Err(format!("no data received within {} ms", timeout_ms)),
        }
    }

    pub fn pending_watch_bytes(&self, state: &HubRoutingState) -> usize {
        state.combined_buffer.len()
    }

    pub fn flush_watch_now(&self, state: &mut HubRoutingState) {
        flush_watch_data(
            self,
            &state.path,
            &mut state.combined_buffer,
            &mut state.pending_events,
        );
    }

    pub fn emit_disconnect(&self, path: &str, reason: &str) {
        let channel = crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .map(|watch| watch.channel.clone());
        if let Some(channel) = channel {
            let _ = channel.send(SerialEvent::Disconnect {
                path: path.to_string(),
                reason: reason.to_string(),
            });
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
        let done = Arc::new((Mutex::new(None), Condvar::new()));
        {
            *crate::sync_util::lock_or_recover(&self.drain) = Some(DrainSlot {
                idle_ms,
                cancel,
                buffer: Vec::new(),
                last_byte_at: None,
                started_at: Instant::now(),
                deadline: Instant::now() + Duration::from_millis(max_ms),
                solicited_prefixes,
                done: done.clone(),
            });
        }
        let (lock, cvar) = &*done;
        let mut guard = crate::sync_util::lock_or_recover(lock);
        let deadline = Instant::now() + Duration::from_millis(max_ms + 500);
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Same ABBA deadlock fix as read_request(): drop `done` before
                // locking `drain` — finish_drain() takes them in the opposite
                // order.
                drop(guard);
                // Same reclaim as read_request: take leftover slot, else recover posted done.
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.drain).take() {
                    if slot.buffer.is_empty() {
                        return Err("drain timed out waiting for hub".into());
                    }
                    return Ok(slot.buffer);
                }
                return match Self::recover_or_timeout(lock, cvar, 0) {
                    Ok(buf) => Ok(buf),
                    Err(e) if e.contains("no data received") => {
                        Err("drain timed out waiting for hub".into())
                    }
                    Err(e) => Err(e),
                };
            }
            let (g, _) = cvar
                .wait_timeout(guard, remaining)
                .map_err(|e| e.to_string())?;
            guard = g;
        }
        guard.take().unwrap()
    }
}
/// Append RX into the active drain slot. Returns `false` if there is no slot
/// (caller must route elsewhere — never silently drop the chunk).
pub(crate) fn route_drain_chunk(shared: &RxHubShared, path: &str, chunk: &[u8]) -> bool {
    let prefixes = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
        let Some(drain) = guard.as_mut() else {
            return false;
        };
        drain.buffer.extend_from_slice(chunk);
        drain.last_byte_at = Some(Instant::now());
        drain.solicited_prefixes.clone()
    };
    emit_drain_urc_with_prefixes(shared, path, chunk, &prefixes);
    true
}

pub(crate) fn route_exchange_chunk(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    state: &mut HubRoutingState,
    waiter: Arc<ExchangeWaiter>,
) {
    if state.exchange_demux.is_none() {
        let cmd = waiter.options.command.clone().unwrap_or_default();
        state.exchange_demux = Some(ExchangeDemux::new(&cmd, &waiter.options.solicited_prefixes));
    }
    if let Some(demux) = state.exchange_demux.as_mut() {
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

pub(crate) fn route_watch_chunk(path: &str, chunk: &[u8], state: &mut HubRoutingState) {
    state.exchange_demux = None;
    for action in state.line_router.route_streaming(chunk, &[]) {
        match action {
            RxRouteAction::UrcLine(line) => {
                state.pending_events.push(SerialEvent::Urc {
                    path: path.to_string(),
                    line,
                });
            }
            RxRouteAction::StreamData(bytes) => {
                state.combined_buffer.extend_from_slice(&bytes);
            }
        }
    }
}

pub(crate) fn route_read_slot_chunk(shared: &RxHubShared, chunk: &[u8]) {
    let completed = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let ready = {
            let Some(slot) = guard.as_mut() else {
                return;
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
            Some((slot.done, Ok(slot.buffer)))
        }
    };
    if let Some((done, result)) = completed {
        wake_done(&done, result);
    }
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
        Some((slot.done, result))
    };
    if let Some((done, result)) = completed {
        wake_done(&done, result);
    }
}

pub(crate) fn wake_done(done: &ReadDone, result: Result<Vec<u8>, String>) {
    let (lock, cvar) = &**done;
    *crate::sync_util::lock_or_recover(lock) = Some(result);
    cvar.notify_all();
}

pub(crate) fn finish_read_slot(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    // Take slot first so the read_slot guard drops before locking done (ABBA).
    let slot = crate::sync_util::lock_or_recover(&shared.read_slot).take();
    if let Some(slot) = slot {
        wake_done(&slot.done, result);
    }
}

pub(crate) fn push_idle(shared: &RxHubShared, chunk: &[u8]) {
    let mut idle = crate::sync_util::lock_or_recover(&shared.idle);
    idle.extend_from_slice(chunk);
    if idle.len() > IDLE_BUFFER_CAP {
        let excess = idle.len() - IDLE_BUFFER_CAP;
        idle.drain(..excess);
    }
}

pub(crate) fn finish_drain(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    let drain = crate::sync_util::lock_or_recover(&shared.drain).take();
    if let Some(drain) = drain {
        wake_done(&drain.done, result);
    }
}

pub(crate) fn emit_drain_urc_with_prefixes(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    prefixes: &[String],
) {
    let lines = crate::at::parse::split_lines(&String::from_utf8_lossy(chunk));
    let channel = shared
        .watch
        .lock()
        .unwrap()
        .as_ref()
        .map(|watch| watch.channel.clone());
    if let Some(channel) = channel {
        for line in lines {
            if is_likely_urc(&line, prefixes) {
                emit_urc(&channel, path, &line);
            }
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
    use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
    use std::thread;

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
        use tauri::ipc::Channel;
        let shared = Arc::new(RxHubShared::new());
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024);
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
        use tauri::ipc::Channel;
        let shared = Arc::new(RxHubShared::new());
        shared.feed_bytes(b"stale", &mut HubRoutingState::new("p".into()));
        assert!(!crate::sync_util::lock_or_recover(&shared.idle).is_empty());
        let channel = Channel::<SerialEvent>::new(|_| Ok(()));
        shared.attach_watch(channel, 100, 1024);
        assert!(crate::sync_util::lock_or_recover(&shared.idle).is_empty());
    }

    // Regression coverage for RxHubShared::recover_or_timeout(). Exercises
    // it directly (it's a private assoc fn, visible to this nested test
    // module via `use super::*`), independent of the full ABBA-race timing
    // in read_request() itself. Uses recover_or_timeout_within() with a
    // wide window (well beyond each test's own posting delay) rather than
    // the production 50ms constant, so a slow/preempted CI runner can't
    // turn a correct implementation into a flaky test.

    #[test]
    fn recover_or_timeout_returns_result_posted_during_recovery_window() {
        let done: ReadDone = Arc::new((Mutex::new(None), Condvar::new()));
        let done_bg = done.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            let (lock, cvar) = &*done_bg;
            *crate::sync_util::lock_or_recover(lock) = Some(Ok(b"late".to_vec()));
            cvar.notify_all();
        });
        let (lock, cvar) = &*done;
        let result = RxHubShared::recover_or_timeout_within(lock, cvar, 5, Duration::from_secs(2));
        assert_eq!(result.expect("recovered result"), b"late");
    }

    #[test]
    fn recover_or_timeout_survives_spurious_wakeups() {
        // Fires several notify_all()s with `done` still None before the
        // real result lands — simulates the condvar spurious-wakeup case.
        // A single-`wait_timeout` version returns a false timeout on the
        // first one; the fixed loop keeps waiting out the recovery window.
        let done: ReadDone = Arc::new((Mutex::new(None), Condvar::new()));
        let done_bg = done.clone();
        thread::spawn(move || {
            for _ in 0..3 {
                thread::sleep(Duration::from_millis(5));
                done_bg.1.notify_all();
            }
            thread::sleep(Duration::from_millis(5));
            let (lock, cvar) = &*done_bg;
            *crate::sync_util::lock_or_recover(lock) = Some(Ok(b"after-spurious".to_vec()));
            cvar.notify_all();
        });
        let (lock, cvar) = &*done;
        let result = RxHubShared::recover_or_timeout_within(lock, cvar, 5, Duration::from_secs(2));
        assert_eq!(result.expect("recovered result"), b"after-spurious");
    }

    #[test]
    fn recover_or_timeout_falls_back_to_timeout_error_when_nothing_arrives() {
        let done: ReadDone = Arc::new((Mutex::new(None), Condvar::new()));
        let start = Instant::now();
        let result = RxHubShared::recover_or_timeout(&done.0, &done.1, 5);
        let err = result.expect_err("no result ever posted");
        assert!(err.contains("no data received"), "unexpected: {err}");
        // Bounded by READ_SLOT_RACE_RECOVERY_WINDOW (50ms), not a hang.
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    // --- Post-#38: completing a slot takes it whole (no emptied-Some window) ---

    fn mock_done() -> ReadDone {
        Arc::new((Mutex::new(None), Condvar::new()))
    }

    #[test]
    fn route_read_slot_chunk_completing_removes_slot_before_post() {
        let shared = RxHubShared::new();
        let done = mock_done();
        *crate::sync_util::lock_or_recover(&shared.read_slot) = Some(ReadSlot {
            max_bytes: 3,
            fill: false,
            timeout_ms: 100,
            buffer: Vec::new(),
            deadline: Instant::now() + Duration::from_secs(5),
            done: done.clone(),
        });
        route_read_slot_chunk(&shared, b"xyz");
        assert!(crate::sync_util::lock_or_recover(&shared.read_slot).is_none());
        let guard = crate::sync_util::lock_or_recover(&done.0);
        assert_eq!(guard.as_ref().unwrap().as_ref().unwrap(), b"xyz");
    }

    #[test]
    fn tick_read_slot_deadline_takes_whole_slot() {
        let shared = RxHubShared::new();
        let done = mock_done();
        *crate::sync_util::lock_or_recover(&shared.read_slot) = Some(ReadSlot {
            max_bytes: 64,
            fill: false,
            timeout_ms: 50,
            buffer: b"late".to_vec(),
            deadline: Instant::now() - Duration::from_millis(1),
            done: done.clone(),
        });
        tick_read_slot(&shared);
        assert!(crate::sync_util::lock_or_recover(&shared.read_slot).is_none());
        let guard = crate::sync_util::lock_or_recover(&done.0);
        assert_eq!(guard.as_ref().unwrap().as_ref().unwrap(), b"late");
    }

    #[test]
    fn tick_drain_complete_takes_whole_slot() {
        let shared = RxHubShared::new();
        let done: DrainDone = Arc::new((Mutex::new(None), Condvar::new()));
        *crate::sync_util::lock_or_recover(&shared.drain) = Some(DrainSlot {
            idle_ms: 1,
            cancel: Arc::new(AtomicBool::new(false)),
            buffer: b"buf".to_vec(),
            last_byte_at: Some(Instant::now() - Duration::from_millis(50)),
            started_at: Instant::now() - Duration::from_millis(100),
            deadline: Instant::now() + Duration::from_secs(5),
            solicited_prefixes: vec![],
            done: done.clone(),
        });
        let mut routing = HubRoutingState::new("p".into());
        shared.tick("p", &mut routing);
        assert!(crate::sync_util::lock_or_recover(&shared.drain).is_none());
        let guard = crate::sync_util::lock_or_recover(&done.0);
        assert_eq!(guard.as_ref().unwrap().as_ref().unwrap(), b"buf");
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

    #[test]
    fn route_drain_chunk_returns_false_when_no_slot() {
        let shared = RxHubShared::new();
        assert!(!route_drain_chunk(&shared, "p", b"x"));
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
    fn drain_timeout_recovers_result_already_posted_to_done() {
        // finish_drain posts before the wait loop's hard deadline; reclaim
        // must return that Ok rather than a synthetic timeout.
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
        // max_ms=0 → wait loop hard deadline is ~500ms; inject bytes into the
        // live slot without completing via tick so reclaim's take() path runs.
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
}
