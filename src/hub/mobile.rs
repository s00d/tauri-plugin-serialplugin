//! Push-model RX hub for Android (bytes fed from Kotlin/JNI instead of poll loop).

use crate::cmux::CmuxSession;
use crate::events::SerialEvent;
use crate::hub::shared::{HubRoutingState, RxHubShared};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use tauri::ipc::Channel;

const MOBILE_HUB_TICK_MS: u64 = 20;

/// RX hub driven by [`RxHubShared::feed_bytes`] (Android SIOM → JNI).
pub struct MobileRxHub {
    pub path: String,
    shared: Arc<RxHubShared>,
    routing: Arc<Mutex<HubRoutingState>>,
    stop: Arc<AtomicBool>,
    timer: Mutex<Option<JoinHandle<()>>>,
}

impl MobileRxHub {
    pub fn new(path: String) -> Self {
        let shared = Arc::new(RxHubShared::new());
        let routing = Arc::new(Mutex::new(HubRoutingState::new(path.clone())));
        let stop = Arc::new(AtomicBool::new(false));
        let timer = Mutex::new(Some(spawn_tick_thread(
            shared.clone(),
            path.clone(),
            routing.clone(),
            stop.clone(),
        )));
        Self {
            path,
            shared,
            routing,
            stop,
            timer,
        }
    }

    pub fn shared(&self) -> Arc<RxHubShared> {
        self.shared.clone()
    }

    pub fn feed(&self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        let pending = {
            let mut routing = crate::sync_util::lock_or_recover(&self.routing);
            self.shared.feed_bytes(chunk, &mut routing);
            std::mem::take(&mut routing.pending_events)
        };
        self.shared.dispatch_pending_events(pending);
    }

    pub fn pending_watch_bytes(&self) -> usize {
        let routing = crate::sync_util::lock_or_recover(&self.routing);
        self.shared.pending_watch_bytes(&routing)
    }

    pub fn buffered_len(&self) -> usize {
        let routing = crate::sync_util::lock_or_recover(&self.routing);
        self.shared.buffered_len() + self.shared.pending_watch_bytes(&routing)
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

    pub fn attach_cmux(&self, session: Arc<CmuxSession>) {
        self.shared.attach_cmux(session);
    }

    pub fn detach_cmux(&self) {
        self.shared.detach_cmux();
    }

    pub fn set_exchange_waiter(&self, waiter: Arc<crate::hub::ExchangeWaiter>) {
        self.shared.set_exchange_waiter(waiter);
    }

    pub fn clear_exchange_waiter(&self) {
        self.shared.clear_exchange_waiter();
    }

    pub fn cancel_active_exchange(&self) {
        self.shared.cancel_active_exchange();
    }

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

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(h) = crate::sync_util::lock_or_recover(&self.timer).take() {
            let _ = h.join();
        }
        let pending = {
            let mut routing = crate::sync_util::lock_or_recover(&self.routing);
            self.shared.flush_watch_now(&mut routing);
            std::mem::take(&mut routing.pending_events)
        };
        self.shared.dispatch_pending_events(pending);
    }
}

impl crate::hub::handle::RxHubHandle for MobileRxHub {
    fn shared(&self) -> Arc<RxHubShared> {
        self.shared()
    }
    fn set_exchange_waiter(&self, waiter: Arc<crate::hub::ExchangeWaiter>) {
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
    fn feed_rx(&self, chunk: &[u8]) {
        self.feed(chunk);
    }
    fn shutdown_hub(&self) {
        self.shutdown();
    }
}

fn spawn_tick_thread(
    shared: Arc<RxHubShared>,
    path: String,
    routing: Arc<Mutex<HubRoutingState>>,
    stop: Arc<AtomicBool>,
) -> JoinHandle<()> {
    thread::spawn(move || {
        while !stop.load(Ordering::SeqCst) {
            thread::sleep(Duration::from_millis(MOBILE_HUB_TICK_MS));
            if stop.load(Ordering::SeqCst) {
                break;
            }
            let mut guard = crate::sync_util::lock_or_recover(&routing);
            shared.tick(&path, &mut guard);
            let pending = std::mem::take(&mut guard.pending_events);
            drop(guard);
            shared.dispatch_pending_events(pending);
        }
    })
}
