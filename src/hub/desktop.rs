//! Desktop poll-model RX hub.

mod io_errors {
    /// Read timeout / would-block — hub may retry.
    pub(super) fn is_benign_read_error(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::TimedOut
                | std::io::ErrorKind::WouldBlock
                | std::io::ErrorKind::Interrupted
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

        /// Windows reports an unplugged adapter as PermissionDenied, never BrokenPipe.
        #[cfg(not(target_os = "android"))]
        #[test]
        fn persistent_read_error_ends_in_disconnect_event() {
            use crate::events::SerialEvent;
            use crate::mock_serial::MockSerialPort;
            use std::sync::{Arc, Mutex};
            use std::time::Duration;
            use tauri::ipc::{Channel, InvokeResponseBody};

            let mut mock = MockSerialPort::new();
            mock.sticky_read_error = Some(std::io::ErrorKind::PermissionDenied);
            let port: Arc<Mutex<Box<dyn serialport::SerialPort>>> =
                Arc::new(Mutex::new(Box::new(mock)));
            let (tx, rx) = std::sync::mpsc::channel();
            let tx = Mutex::new(tx);
            let channel = Channel::<SerialEvent>::new(move |body| {
                if let InvokeResponseBody::Json(json) = body {
                    if json.contains("disconnect") {
                        let _ = tx.lock().unwrap().send(());
                    }
                }
                Ok(())
            });
            let hub = super::super::PortRxHub::start(port, "unplugged".into());
            hub.attach_watch(channel, 10, 64, false);
            rx.recv_timeout(Duration::from_secs(3))
                .expect("no Disconnect after persistent read errors");
            hub.stop();
        }

        #[test]
        fn eintr_is_benign() {
            let err = std::io::Error::new(std::io::ErrorKind::Interrupted, "EINTR");
            assert!(is_benign_read_error(&err));
        }

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
    finish_drain, tick_read_slot, try_complete_drain, ExchangeWaiter, HubRoutingState, RxHubShared,
};
use serialport::SerialPort;
use std::io::Read;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

const POLL_READ_TIMEOUT_MS: u64 = 10;

/// `Ok(None)` means the hub was asked to stop.
fn poll_read_port(
    port: &Arc<Mutex<Box<dyn SerialPort>>>,
    buf: &mut [u8],
    stop_rx: &Receiver<()>,
) -> std::io::Result<Option<usize>> {
    loop {
        if matches!(stop_rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)) {
            return Ok(None);
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
            Ok(n) => return Ok(Some(n)),
            Err(e) if is_benign_read_error(&e) => {
                drop(p);
                // Yield so hub_loop can complete drain idle/deadline and flush watch batches.
                return Ok(Some(0));
            }
            Err(e) => return Err(e),
        }
    }
}

/// Non-benign read errors in a row before the port is treated as gone. Windows reports
/// an unplugged adapter as PermissionDenied / Other, Unix sometimes as EIO, never as
/// BrokenPipe, so a persistent error is the only portable unplug signal.
const MAX_CONSECUTIVE_READ_ERRORS: u32 = 5;

fn shutdown_hub_state(shared: &RxHubShared, routing: &mut HubRoutingState) {
    shared.fail_all_waiters("port closed");
    shared.flush_watch_now(routing);
    shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
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
        route_urc: bool,
    ) {
        self.shared
            .attach_watch(channel, batch_timeout_ms, read_size, route_urc);
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
    fn attach_watch(
        &self,
        channel: Channel<SerialEvent>,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
    ) {
        self.attach_watch(channel, batch_timeout_ms, read_size, route_urc);
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
    fn shutdown_hub(&self) {
        self.request_stop();
    }
}

fn hub_loop(
    port: Arc<Mutex<Box<dyn SerialPort>>>,
    path: String,
    shared: Arc<RxHubShared>,
    stop_rx: Receiver<()>,
) {
    let mut routing = HubRoutingState::new(path.clone());
    let mut read_size = 1024usize;
    let mut last_error_emit = Instant::now() - Duration::from_secs(1);
    let mut consecutive_errors = 0u32;

    loop {
        if matches!(stop_rx.try_recv(), Ok(_) | Err(TryRecvError::Disconnected)) {
            shutdown_hub_state(&shared, &mut routing);
            break;
        }

        if crate::sync_util::lock_or_recover(&shared.drain).is_some() {
            try_complete_drain(&shared);
            if crate::sync_util::lock_or_recover(&shared.drain).is_none() {
                continue;
            }

            let mut buf = vec![0u8; 1024];
            let n = match poll_read_port(&port, &mut buf, &stop_rx) {
                Ok(Some(n)) => n,
                Ok(None) => {
                    shutdown_hub_state(&shared, &mut routing);
                    break;
                }
                Err(e) => {
                    finish_drain(&shared, Err(format!("drain read failed: {}", e)));
                    continue;
                }
            };

            if n > 0 {
                // feed_bytes retries drain then falls through to exchange/watch/idle
                // if reclaim won during poll_read_port — never drop the chunk.
                shared.feed_bytes(&buf[..n], &mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            }
            try_complete_drain(&shared);
            tick_read_slot(&shared);
            continue;
        }

        if let Some(watch) = crate::sync_util::lock_or_recover(&shared.watch).as_ref() {
            read_size = watch.read_size;
        }

        let mut buf = vec![0u8; read_size];
        let read_result = poll_read_port(&port, &mut buf, &stop_rx);

        if read_result.is_ok() {
            consecutive_errors = 0;
        }
        match read_result {
            Ok(None) => {
                shutdown_hub_state(&shared, &mut routing);
                break;
            }
            Ok(Some(n)) if n > 0 => {
                shared.feed_bytes(&buf[..n], &mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
            }
            Ok(Some(_)) => {
                thread::sleep(Duration::from_millis(1));
            }
            Err(e) => {
                consecutive_errors += 1;
                shared.flush_watch_now(&mut routing);
                shared.dispatch_pending_events(std::mem::take(&mut routing.pending_events));
                if is_disconnect_read_error(&e) || consecutive_errors >= MAX_CONSECUTIVE_READ_ERRORS
                {
                    let reason = format!("Serial port disconnected: {}", e);
                    shared.fail_all_waiters(&reason);
                    let channel = crate::sync_util::lock_or_recover(&shared.watch)
                        .as_ref()
                        .map(|watch| watch.channel.clone());
                    if let Some(channel) = channel {
                        let _ = channel.send(SerialEvent::Disconnect {
                            path: path.clone(),
                            reason: reason.clone(),
                        });
                    }
                    // Android: clear mobile registry + USB adapter (harness expects
                    // inject_bulk_read_error → on_usb_error → registry drop).
                    #[cfg(target_os = "android")]
                    {
                        crate::android::registry::on_usb_error(&path, &reason);
                        let _ = crate::android::driver_host::global_host().close(Some(&path));
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
    }
}
