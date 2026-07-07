//! Unified RX dispatcher: one reader per port routes bytes to watch vs exchange.

use crate::at_parse::{
    at_final_line_complete, at_intermediate_line_complete, classify_final_line, is_likely_urc,
    ExchangeDemux, ExchangeMatch,
};
use crate::cmux::CmuxSession;
use crate::events::{ExchangeCompletionMode, SerialEvent};
use crate::exchange_read::{default_terminators, matches_terminators, ResolvedExchangeOptions};
#[cfg(desktop)]
use serialport::SerialPort;
#[cfg(desktop)]
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(desktop)]
use std::sync::atomic::AtomicU32;
#[cfg(desktop)]
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex};
#[cfg(desktop)]
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

#[cfg(desktop)]
const POLL_READ_TIMEOUT_MS: u64 = 100;

type ExchangeDone = Arc<(
    Mutex<Option<Result<(Vec<u8>, ExchangeMatch), String>>>,
    Condvar,
)>;
type DrainDone = Arc<(Mutex<Option<Result<Vec<u8>, String>>>, Condvar)>;

/// Read from the port without starving writers: retry until the port mutex is free.
#[cfg(desktop)]
fn poll_read_port(
    port: &Arc<Mutex<Box<dyn SerialPort>>>,
    buf: &mut [u8],
) -> std::io::Result<usize> {
    loop {
        let mut p = match port.try_lock() {
            Ok(g) => g,
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        let _ = p.set_timeout(Duration::from_millis(POLL_READ_TIMEOUT_MS));
        return p.read(buf);
    }
}

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
        let mut buffer = self.buffer.lock().unwrap();
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
        let mut guard = lock.lock().unwrap();
        let deadline = Instant::now() + Duration::from_millis(timeout_ms);
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(format!("exchange timed out after {} ms", timeout_ms));
            }
            let (g, timeout) = cvar
                .wait_timeout(guard, remaining)
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
        let mut guard = lock.lock().unwrap();
        *guard = Some(result);
        cvar.notify_all();
    }

    /// Fail an in-flight exchange immediately (e.g. USB error teardown).
    pub fn fail_with_reason(&self, reason: String) {
        self.finish(Err(reason));
    }
}

fn check_exchange_complete(buf: &[u8], options: &ResolvedExchangeOptions) -> Option<ExchangeMatch> {
    match options.completion_mode {
        crate::events::ExchangeCompletionMode::AtFinalLine => {
            at_final_line_complete(buf, options.result_format)
        }
        crate::events::ExchangeCompletionMode::AtIntermediate => {
            at_intermediate_line_complete(buf, options.result_format)
        }
        ExchangeCompletionMode::Substring => {
            if matches_terminators(buf, &options.terminators) {
                Some(ExchangeMatch::Substring {
                    term: String::from_utf8_lossy(default_terminators()[0].as_slice()).into_owned(),
                })
            } else {
                None
            }
        }
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

struct WatchSlot {
    channel: Channel<SerialEvent>,
    batch_timeout_ms: u64,
    /// Poll read chunk size for the desktop hub thread only.
    #[cfg(desktop)]
    read_size: usize,
}

struct DrainSlot {
    idle_ms: u64,
    cancel: Arc<AtomicBool>,
    buffer: Vec<u8>,
    last_byte_at: Option<Instant>,
    started_at: Instant,
    deadline: Instant,
    solicited_prefixes: Vec<String>,
    done: DrainDone,
}

/// Shared hub state between the RX thread and API handlers.
pub struct RxHubShared {
    exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    watch: Mutex<Option<WatchSlot>>,
    drain: Mutex<Option<DrainSlot>>,
    cmux: Mutex<Option<Arc<CmuxSession>>>,
    /// When > 0, the desktop hub reader yields so exchange/write can take the port mutex.
    #[cfg(desktop)]
    port_io_yield: AtomicU32,
}

impl RxHubShared {
    pub fn new() -> Self {
        Self {
            exchange_waiter: Mutex::new(None),
            watch: Mutex::new(None),
            drain: Mutex::new(None),
            cmux: Mutex::new(None),
            #[cfg(desktop)]
            port_io_yield: AtomicU32::new(0),
        }
    }

    pub fn attach_watch(
        &self,
        channel: Channel<SerialEvent>,
        batch_timeout_ms: u64,
        #[cfg_attr(not(desktop), allow(unused_variables))] read_size: usize,
    ) {
        *self.watch.lock().unwrap() = Some(WatchSlot {
            channel,
            batch_timeout_ms,
            #[cfg(desktop)]
            read_size,
        });
    }

    pub fn detach_watch(&self) {
        *self.watch.lock().unwrap() = None;
    }

    pub fn attach_cmux(&self, session: Arc<CmuxSession>) {
        *self.cmux.lock().unwrap() = Some(session);
    }

    pub fn detach_cmux(&self) {
        *self.cmux.lock().unwrap() = None;
    }

    pub fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>) {
        *self.exchange_waiter.lock().unwrap() = Some(waiter);
    }

    pub fn clear_exchange_waiter(&self) {
        *self.exchange_waiter.lock().unwrap() = None;
    }

    /// Push-model entry: route incoming bytes (Android JNI / tests).
    pub fn feed_bytes(&self, chunk: &[u8], state: &mut HubRoutingState) {
        if chunk.is_empty() {
            return;
        }
        let path = state.path.clone();
        if self.drain.lock().unwrap().is_some() {
            route_drain_chunk(self, &path, chunk);
            return;
        }
        route_incoming_chunk(self, &path, chunk, state);
    }

    /// Idle timers for push model: drain completion + watch batch flush.
    pub fn tick(&self, path: &str, state: &mut HubRoutingState) {
        if self.drain.lock().unwrap().is_some() {
            let early = {
                let mut guard = self.drain.lock().unwrap();
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
            flush_watch_data(self, path, &mut state.combined_buffer, &mut state.pending_events);
        }
    }

    /// Immediately fail exchange waiters and active drain (USB error teardown).
    pub fn fail_all_waiters(&self, reason: &str) {
        if let Some(waiter) = self.exchange_waiter.lock().unwrap().take() {
            waiter.fail_with_reason(reason.to_string());
        }
        finish_drain(self, Err(reason.to_string()));
    }

    pub fn pending_watch_bytes(&self, state: &HubRoutingState) -> usize {
        state.combined_buffer.len()
    }

    pub fn flush_watch_now(&self, state: &mut HubRoutingState) {
        flush_watch_data(self, &state.path, &mut state.combined_buffer, &mut state.pending_events);
    }

    pub fn emit_disconnect(&self, path: &str, reason: &str) {
        if let Some(watch) = self.watch.lock().unwrap().as_ref() {
            let _ = watch.channel.send(SerialEvent::Disconnect {
                path: path.to_string(),
                reason: reason.to_string(),
            });
        }
    }

    /// Ask the desktop hub reader to back off while another thread needs the port mutex.
    #[cfg(desktop)]
    pub fn begin_port_io(&self) {
        self.port_io_yield.fetch_add(1, Ordering::SeqCst);
    }

    #[cfg(desktop)]
    pub fn end_port_io(&self) {
        self.port_io_yield.fetch_sub(1, Ordering::SeqCst);
    }

    #[cfg(desktop)]
    fn should_yield_port_io(&self) -> bool {
        self.port_io_yield.load(Ordering::SeqCst) > 0
    }

    pub fn has_watch(&self) -> bool {
        self.watch.lock().map(|g| g.is_some()).unwrap_or(false)
    }

    /// Deliver events queued while holding the routing mutex (avoids channel.send under lock).
    pub fn dispatch_pending_events(&self, events: Vec<SerialEvent>) {
        if events.is_empty() {
            return;
        }
        if let Some(watch) = self.watch.lock().unwrap().as_ref() {
            for ev in events {
                crate::watch_registry::send_event(&watch.channel, ev);
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
            *self.drain.lock().unwrap() = Some(DrainSlot {
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
        let mut guard = lock.lock().unwrap();
        let deadline = Instant::now() + Duration::from_millis(max_ms + 500);
        while guard.is_none() {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                *self.drain.lock().unwrap() = None;
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

/// Clears [`RxHubShared::begin_port_io`] on drop (desktop poll hub only).
#[cfg(desktop)]
pub struct PortIoYieldGuard {
    shared: Arc<RxHubShared>,
}

#[cfg(desktop)]
impl PortIoYieldGuard {
    pub fn new(shared: Arc<RxHubShared>) -> Self {
        shared.begin_port_io();
        Self { shared }
    }
}

#[cfg(desktop)]
impl Drop for PortIoYieldGuard {
    fn drop(&mut self) {
        self.shared.end_port_io();
    }
}

/// Single RX consumer on the main serial fd (desktop).
#[cfg(desktop)]
pub struct PortRxHub {
    shared: Arc<RxHubShared>,
    stop_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(desktop)]
impl PortRxHub {
    pub fn start(port: Arc<Mutex<Box<dyn SerialPort>>>, path: String) -> Self {
        let shared = Arc::new(RxHubShared::new());
        let (stop_tx, stop_rx) = mpsc::channel();
        let shared_clone = shared.clone();
        let thread = thread::spawn(move || hub_loop(port, path, shared_clone, stop_rx));
        Self {
            shared,
            stop_tx,
            thread: Some(thread),
        }
    }

    pub fn shared(&self) -> Arc<RxHubShared> {
        self.shared.clone()
    }

    pub fn attach_watch(
        &self,
        channel: Channel<SerialEvent>,
        batch_timeout_ms: u64,
        read_size: usize,
    ) {
        self.shared
            .attach_watch(channel, batch_timeout_ms, read_size);
    }

    pub fn detach_watch(&self) {
        self.shared.detach_watch();
    }

    pub fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>) {
        self.shared.set_exchange_waiter(waiter);
    }

    pub fn clear_exchange_waiter(&self) {
        self.shared.clear_exchange_waiter();
    }

    pub fn attach_cmux(&self, session: Arc<CmuxSession>) {
        self.shared.attach_cmux(session);
    }

    pub fn detach_cmux(&self) {
        self.shared.detach_cmux();
    }

    /// Soft-drain via the hub thread (single reader); URC lines are emitted on the watch channel.
    pub fn drain(
        &self,
        idle_ms: u64,
        max_ms: u64,
        cancel: Arc<AtomicBool>,
        solicited_prefixes: Vec<String>,
    ) -> Result<Vec<u8>, String> {
        self.shared.drain(idle_ms, max_ms, cancel, solicited_prefixes)
    }

    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

#[cfg(desktop)]
fn hub_loop(
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    path: String,
    shared: Arc<RxHubShared>,
    stop_rx: Receiver<()>,
) {
    let mut routing = HubRoutingState::new(path.clone());
    let mut batch_timeout_ms = 1000u64;
    let mut read_size = 1024usize;
    let mut consecutive_errors = 0u32;

    loop {
        if matches!(stop_rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)) {
            shared.flush_watch_now(&mut routing);
            shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            break;
        }

        if shared.drain.lock().unwrap().is_some() {
            let early_finish = {
                let mut guard = shared.drain.lock().unwrap();
                let Some(drain) = guard.as_mut() else {
                    continue;
                };
                if drain.cancel.load(Ordering::SeqCst) {
                    Some(Err("exchange cancelled".into()))
                } else if Instant::now() >= drain.deadline {
                    Some(Ok(std::mem::take(&mut drain.buffer)))
                } else {
                    None
                }
            };
            if let Some(result) = early_finish {
                finish_drain(&shared, result);
                continue;
            }

            let mut buf = vec![0u8; 1024];
            let n = match poll_read_port(&port, &mut buf) {
                Ok(n) => n,
                Err(e) if is_benign(&e) => 0,
                Err(e) => {
                    finish_drain(&shared, Err(format!("drain read failed: {}", e)));
                    continue;
                }
            };

            if n > 0 {
                route_drain_chunk(&shared, &path, &buf[..n]);
            } else {
                let finish = {
                    let mut guard = shared.drain.lock().unwrap();
                    let Some(drain) = guard.as_mut() else {
                        continue;
                    };
                    if drain.last_byte_at.is_none() {
                        Some(Ok(Vec::new()))
                    } else if drain
                        .last_byte_at
                        .is_some_and(|t| t.elapsed() >= Duration::from_millis(drain.idle_ms))
                    {
                        Some(Ok(std::mem::take(&mut drain.buffer)))
                    } else {
                        None
                    }
                };
                if let Some(result) = finish {
                    finish_drain(&shared, result);
                }
            }
            continue;
        }

        if let Some(watch) = shared.watch.lock().unwrap().as_ref() {
            batch_timeout_ms = watch.batch_timeout_ms;
            read_size = watch.read_size;
        }

        if shared.should_yield_port_io() {
            thread::sleep(Duration::from_millis(1));
            continue;
        }

        let mut buf = vec![0u8; read_size];
        let read_result = poll_read_port(&port, &mut buf);

        match read_result {
            Ok(n) if n > 0 => {
                consecutive_errors = 0;
                shared.feed_bytes(&buf[..n], &mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            }
            Ok(_) => {}
            Err(e) if is_benign(&e) => {}
            Err(e) => {
                shared.flush_watch_now(&mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
                if is_disconnect(&e) {
                    if let Some(watch) = shared.watch.lock().unwrap().as_ref() {
                        let _ = watch.channel.send(SerialEvent::Disconnect {
                            path: path.clone(),
                            reason: format!("Serial port disconnected: {}", e),
                        });
                    }
                    break;
                }
                consecutive_errors += 1;
                if let Some(watch) = shared.watch.lock().unwrap().as_ref() {
                    let _ = watch.channel.send(SerialEvent::Error {
                        path: path.clone(),
                        message: format!("Serial read error: {}", e),
                    });
                }
                if consecutive_errors >= 3 {
                    break;
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        shared.tick(&path, &mut routing);
        shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
        let _ = batch_timeout_ms;
    }
}

fn route_drain_chunk(shared: &RxHubShared, path: &str, chunk: &[u8]) {
    let prefixes = {
        let mut guard = shared.drain.lock().unwrap();
        let Some(drain) = guard.as_mut() else {
            return;
        };
        drain.buffer.extend_from_slice(chunk);
        drain.last_byte_at = Some(Instant::now());
        drain.solicited_prefixes.clone()
    };
    emit_drain_urc_with_prefixes(shared, path, chunk, &prefixes);
}

fn route_incoming_chunk(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    state: &mut HubRoutingState,
) {
    if let Some(session) = shared.cmux.lock().unwrap().clone() {
        session.feed_physical_rx(chunk);
        return;
    }
    if let Some(waiter) = shared.exchange_waiter.lock().unwrap().clone() {
        if state.exchange_demux.is_none() {
            let cmd = waiter.options.command.clone().unwrap_or_default();
            state.exchange_demux =
                Some(ExchangeDemux::new(&cmd, &waiter.options.solicited_prefixes));
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
    } else {
        state.exchange_demux = None;
        let prefixes: Vec<String> = shared
            .watch
            .lock()
            .unwrap()
            .as_ref()
            .map(|_| Vec::new())
            .unwrap_or_default();
        for action in state.line_router.route_streaming(chunk, &prefixes) {
            match action {
                RxRouteAction::UrcLine(line) => {
                    if shared.has_watch() {
                        state.pending_events.push(SerialEvent::Urc {
                            path: path.to_string(),
                            line,
                        });
                    }
                }
                RxRouteAction::StreamData(bytes) => {
                    state.combined_buffer.extend_from_slice(&bytes);
                }
            }
        }
    }
}

fn finish_drain(shared: &RxHubShared, result: Result<Vec<u8>, String>) {
    if let Some(drain) = shared.drain.lock().unwrap().take() {
        let (lock, cvar) = &*drain.done;
        *lock.lock().unwrap() = Some(result);
        cvar.notify_all();
    }
}

fn emit_drain_urc_with_prefixes(
    shared: &RxHubShared,
    path: &str,
    chunk: &[u8],
    prefixes: &[String],
) {
    let lines = crate::at_parse::split_lines(&String::from_utf8_lossy(chunk));
    if let Some(watch) = shared.watch.lock().unwrap().as_ref() {
        for line in lines {
            if is_likely_urc(&line, prefixes) {
                emit_urc(&watch.channel, path, &line);
            }
        }
    }
}

fn flush_watch_data(shared: &RxHubShared, path: &str, combined_buffer: &mut Vec<u8>, pending: &mut Vec<SerialEvent>) {
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

#[cfg(desktop)]
fn is_benign(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    )
}

#[cfg(desktop)]
fn is_disconnect(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::BrokenPipe
            | std::io::ErrorKind::NotConnected
            | std::io::ErrorKind::ConnectionAborted
            | std::io::ErrorKind::ConnectionReset
            | std::io::ErrorKind::UnexpectedEof
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, RxPrepareMode};

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
}
