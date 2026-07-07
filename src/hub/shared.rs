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
    /// Poll read chunk size for the desktop hub thread only.
    #[cfg(desktop)]
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
        #[cfg_attr(not(desktop), allow(unused_variables))] read_size: usize,
    ) {
        crate::sync_util::lock_or_recover(&self.idle).clear();
        *crate::sync_util::lock_or_recover(&self.watch) = Some(WatchSlot {
            channel,
            batch_timeout_ms,
            #[cfg(desktop)]
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
        if crate::sync_util::lock_or_recover(&self.drain).is_some() {
            route_drain_chunk(self, &path, chunk);
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
        if crate::sync_util::lock_or_recover(&self.drain).is_some() {
            let early = {
                let mut guard = crate::sync_util::lock_or_recover(&self.drain);
                let Some(drain) = guard.as_mut() else {
                    return;
                };
                if drain.cancel.load(Ordering::SeqCst) {
                    Some(Err("exchange cancelled".into()))
                } else if Instant::now() >= drain.deadline {
                    Some(Ok(std::mem::take(&mut drain.buffer)))
                } else if let Some(last) = drain.last_byte_at {
                    if last.elapsed() >= Duration::from_millis(drain.idle_ms) {
                        Some(Ok(std::mem::take(&mut drain.buffer)))
                    } else {
                        None
                    }
                } else if drain.started_at.elapsed() >= Duration::from_millis(drain.idle_ms) {
                    Some(Ok(Vec::new()))
                } else {
                    None
                }
            };
            if let Some(result) = early {
                finish_drain(self, result);
            }
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
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).take() {
                    if slot.buffer.is_empty() {
                        return Err(format!("no data received within {} ms", timeout_ms));
                    }
                    return Ok(slot.buffer);
                }
                return Err(format!("no data received within {} ms", timeout_ms));
            }
            let (g, timeout_result) = cvar
                .wait_timeout(guard, remaining)
                .map_err(|e| e.to_string())?;
            guard = g;
            if guard.is_none() && timeout_result.timed_out() && Instant::now() >= deadline {
                if let Some(slot) = crate::sync_util::lock_or_recover(&self.read_slot).take() {
                    if slot.buffer.is_empty() {
                        return Err(format!("no data received within {} ms", timeout_ms));
                    }
                    return Ok(slot.buffer);
                }
                return Err(format!("no data received within {} ms", timeout_ms));
            }
        }
        guard.take().unwrap()
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
                *crate::sync_util::lock_or_recover(&self.drain) = None;
                return Err("drain timed out waiting for hub".into());
            }
            let (g, _) = cvar
                .wait_timeout(guard, remaining)
                .map_err(|e| e.to_string())?;
            guard = g;
        }
        guard.take().unwrap()
    }
}
pub(crate) fn route_drain_chunk(shared: &RxHubShared, path: &str, chunk: &[u8]) {
    let prefixes = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
        let Some(drain) = guard.as_mut() else {
            return;
        };
        drain.buffer.extend_from_slice(chunk);
        drain.last_byte_at = Some(Instant::now());
        drain.solicited_prefixes.clone()
    };
    emit_drain_urc_with_prefixes(shared, path, chunk, &prefixes);
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
    let finish = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let Some(slot) = guard.as_mut() else {
            return;
        };
        let remaining = slot.max_bytes.saturating_sub(slot.buffer.len());
        if remaining == 0 {
            Some(Ok(std::mem::take(&mut slot.buffer)))
        } else {
            let take = chunk.len().min(remaining);
            slot.buffer.extend_from_slice(&chunk[..take]);
            if !slot.fill || slot.buffer.len() >= slot.max_bytes {
                Some(Ok(std::mem::take(&mut slot.buffer)))
            } else {
                None
            }
        }
    };
    if let Some(result) = finish {
        finish_read_slot(shared, result);
    }
}

pub(crate) fn tick_read_slot(shared: &RxHubShared) {
    let early = {
        let mut guard = crate::sync_util::lock_or_recover(&shared.read_slot);
        let Some(slot) = guard.as_mut() else {
            return;
        };
        if Instant::now() >= slot.deadline {
            if slot.buffer.is_empty() {
                Some(Err(format!(
                    "no data received within {} ms",
                    slot.timeout_ms
                )))
            } else {
                Some(Ok(std::mem::take(&mut slot.buffer)))
            }
        } else {
            None
        }
    };
    if let Some(result) = early {
        finish_read_slot(shared, result);
    }
}

pub(crate) fn finish_read_slot(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    if let Some(slot) = crate::sync_util::lock_or_recover(&shared.read_slot).take() {
        let (lock, cvar) = &*slot.done;
        *crate::sync_util::lock_or_recover(lock) = Some(result);
        cvar.notify_all();
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
    if let Some(drain) = crate::sync_util::lock_or_recover(&shared.drain).take() {
        let (lock, cvar) = &*drain.done;
        *crate::sync_util::lock_or_recover(lock) = Some(result);
        cvar.notify_all();
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
}
