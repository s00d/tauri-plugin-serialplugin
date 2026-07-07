//! Desktop poll-model RX hub.

mod io_errors {
    /// Read timeout / would-block — hub may retry.
    pub(super) fn is_benign_read_error(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        )
    }

    /// Fatal disconnect — fail waiters and end the hub loop.
    pub(super) fn is_disconnect_read_error(err: &std::io::Error) -> bool {
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

        #[test]
        fn read_timeout_is_benign_not_disconnect() {
            let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout");
            assert!(is_benign_read_error(&err));
            assert!(!is_disconnect_read_error(&err));
        }

        #[test]
        fn broken_pipe_is_disconnect() {
            let err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "broken");
            assert!(!is_benign_read_error(&err));
            assert!(is_disconnect_read_error(&err));
        }

        #[test]
        fn permission_denied_is_neither_benign_nor_disconnect() {
            let err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
            assert!(!is_benign_read_error(&err));
            assert!(!is_disconnect_read_error(&err));
        }
    }
}

use io_errors::{is_benign_read_error, is_disconnect_read_error};

use crate::cmux::CmuxSession;
use crate::events::SerialEvent;
use crate::hub::shared::{
    finish_drain, route_drain_chunk, ExchangeWaiter, HubRoutingState, RxHubShared,
};
use serialport::SerialPort;
use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

const POLL_READ_TIMEOUT_MS: u64 = 10;

fn poll_read_port(
    port: &Arc<Mutex<Box<dyn SerialPort>>>,
    buf: &mut [u8],
    stop_rx: &Receiver<()>,
) -> std::io::Result<usize> {
    loop {
        if matches!(stop_rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Interrupted,
                "rx hub stopped",
            ));
        }
        let mut p = match port.try_lock() {
            Ok(g) => g,
            Err(_) => {
                thread::sleep(Duration::from_millis(1));
                continue;
            }
        };
        let _ = p.set_timeout(Duration::from_millis(POLL_READ_TIMEOUT_MS));
        match p.read(buf) {
            Ok(n) => return Ok(n),
            Err(e) if is_benign_read_error(&e) => {
                drop(p);
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => return Err(e),
        }
    }
}
/// Single RX consumer on the main serial fd (desktop).
pub struct PortRxHub {
    shared: Arc<RxHubShared>,
    stop_tx: Sender<()>,
    thread: Option<JoinHandle<()>>,
}

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

    pub fn is_finished(&self) -> bool {
        self.thread
            .as_ref()
            .map(JoinHandle::is_finished)
            .unwrap_or(true)
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

    pub fn cancel_active_exchange(&self) {
        self.shared.cancel_active_exchange();
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
        self.shared
            .drain(idle_ms, max_ms, cancel, solicited_prefixes)
    }

    pub fn request_stop(&self) {
        let _ = self.stop_tx.send(());
    }

    pub fn stop(mut self) {
        let _ = self.stop_tx.send(());
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl crate::hub::handle::RxHubHandle for PortRxHub {
    fn shared(&self) -> Arc<RxHubShared> {
        self.shared()
    }
    fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>) {
        self.set_exchange_waiter(waiter);
    }
    fn clear_exchange_waiter(&self) {
        self.clear_exchange_waiter();
    }
    fn cancel_active_exchange(&self) {
        self.cancel_active_exchange();
    }
    fn attach_watch(&self, channel: Channel<SerialEvent>, batch_timeout_ms: u64, read_size: usize) {
        self.attach_watch(channel, batch_timeout_ms, read_size);
    }
    fn detach_watch(&self) {
        self.detach_watch();
    }
    fn attach_cmux(&self, session: Arc<CmuxSession>) {
        self.attach_cmux(session);
    }
    fn detach_cmux(&self) {
        self.detach_cmux();
    }
}

fn hub_loop(
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    path: String,
    shared: Arc<RxHubShared>,
    stop_rx: Receiver<()>,
) {
    let mut routing = HubRoutingState::new(path.clone());
    let mut batch_timeout_ms = 1000u64;
    let mut read_size = 1024usize;
    let mut last_error_emit = Instant::now() - Duration::from_secs(1);

    loop {
        if matches!(stop_rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)) {
            shared.fail_all_waiters("port closed");
            shared.flush_watch_now(&mut routing);
            shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            break;
        }

        if crate::sync_util::lock_or_recover(&shared.drain).is_some() {
            let early_finish = {
                let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
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
            let n = match poll_read_port(&port, &mut buf, &stop_rx) {
                Ok(n) => n,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => break,
                Err(e) if is_benign_read_error(&e) => 0,
                Err(e) => {
                    finish_drain(&shared, Err(format!("drain read failed: {}", e)));
                    continue;
                }
            };

            if n > 0 {
                route_drain_chunk(&shared, &path, &buf[..n]);
            } else {
                let finish = {
                    let mut guard = crate::sync_util::lock_or_recover(&shared.drain);
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

        if let Some(watch) = crate::sync_util::lock_or_recover(&shared.watch).as_ref() {
            batch_timeout_ms = watch.batch_timeout_ms;
            read_size = watch.read_size;
        }

        let mut buf = vec![0u8; read_size];
        let read_result = poll_read_port(&port, &mut buf, &stop_rx);

        match read_result {
            Ok(n) if n > 0 => {
                shared.feed_bytes(&buf[..n], &mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            }
            Ok(_) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => break,
            Err(e) if is_benign_read_error(&e) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                shared.flush_watch_now(&mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
                if is_disconnect_read_error(&e) {
                    shared.fail_all_waiters(&format!("Serial port disconnected: {}", e));
                    let channel = crate::sync_util::lock_or_recover(&shared.watch)
                        .as_ref()
                        .map(|watch| watch.channel.clone());
                    if let Some(channel) = channel {
                        let _ = channel.send(SerialEvent::Disconnect {
                            path: path.clone(),
                            reason: format!("Serial port disconnected: {}", e),
                        });
                    }
                    break;
                }
                if last_error_emit.elapsed() >= Duration::from_secs(1) {
                    last_error_emit = Instant::now();
                    let channel = crate::sync_util::lock_or_recover(&shared.watch)
                        .as_ref()
                        .map(|watch| watch.channel.clone());
                    if let Some(channel) = channel {
                        let _ = channel.send(SerialEvent::Error {
                            path: path.clone(),
                            message: format!("Serial read error: {}", e),
                        });
                    }
                }
                thread::sleep(Duration::from_millis(50));
            }
        }

        shared.tick(&path, &mut routing);
        shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
        let _ = batch_timeout_ms;
    }
}
