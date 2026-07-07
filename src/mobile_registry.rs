//! Global registry for JNI feedRx / USB error callbacks (path → hub + port handles).

use crate::mobile_rx_hub::MobileRxHub;
use crate::port_list_monitor;
use crate::state::MobileConnectedPortHandle;
use crate::watch_registry;
use crate::{log_error, log_warn};
use std::collections::HashMap;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, OnceLock};

type RustStateFailFn = Box<dyn Fn(&str, &str) + Send + Sync>;
static RUST_STATE_FAIL: OnceLock<RustStateFailFn> = OnceLock::new();

/// Called from [`SerialPort::setup_teardown`] to drop Rust port map entries after USB failure.
pub fn set_rust_state_fail(f: RustStateFailFn) {
    let _ = RUST_STATE_FAIL.set(f);
}

struct PortEntry {
    hub: Arc<MobileRxHub>,
    handle: MobileConnectedPortHandle,
}

pub struct MobilePortRegistry {
    ports: Mutex<HashMap<String, PortEntry>>,
}

impl MobilePortRegistry {
    fn new() -> Self {
        Self {
            ports: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(&self, hub: Arc<MobileRxHub>, handle: MobileConnectedPortHandle) {
        let path = handle.path.clone();
        crate::sync_util::lock_or_recover(&self.ports).insert(path, PortEntry { hub, handle });
    }

    pub fn unregister(&self, path: &str) {
        crate::sync_util::lock_or_recover(&self.ports).remove(path);
    }

    pub fn feed_rx(&self, path: &str, chunk: &[u8]) {
        if let Some(entry) = crate::sync_util::lock_or_recover(&self.ports).get(path) {
            entry.hub.feed(chunk);
        }
    }

    pub fn fail_port(&self, path: &str, reason: &str) {
        log_error!("fail_port path={} reason={}", path, reason);
        let (hub, handle) = {
            let ports = crate::sync_util::lock_or_recover(&self.ports);
            let Some(entry) = ports.get(path) else {
                log_warn!("fail_port: path {} not in registry", path);
                return;
            };
            (entry.hub.clone(), entry.handle.clone())
        };

        handle.exchange_cancel.store(true, Ordering::SeqCst);
        hub.shared().fail_all_waiters(reason);
        handle.tx_queue.cancel_all();
        handle.tx_queue.clear_halt();
        hub.shared().emit_disconnect(path, reason);

        for channel_id in watch_registry::paths_for_port(path) {
            watch_registry::unregister(channel_id);
        }

        hub.shutdown();
        self.unregister(path);

        if let Some(f) = RUST_STATE_FAIL.get() {
            f(path, reason);
        }
    }

    pub fn handle_for(&self, path: &str) -> Option<MobileConnectedPortHandle> {
        crate::sync_util::lock_or_recover(&self.ports)
            .get(path)
            .map(|e| e.handle.clone())
    }

    pub fn close_all(&self) {
        let entries: Vec<(String, Arc<MobileRxHub>)> =
            crate::sync_util::lock_or_recover(&self.ports)
                .iter()
                .map(|(path, entry)| (path.clone(), entry.hub.clone()))
                .collect();
        for (path, hub) in entries {
            hub.shutdown();
            self.unregister(&path);
        }
    }
}

static REGISTRY: OnceLock<Arc<MobilePortRegistry>> = OnceLock::new();

pub fn init_registry() -> Arc<MobilePortRegistry> {
    REGISTRY
        .get_or_init(|| Arc::new(MobilePortRegistry::new()))
        .clone()
}

pub fn global_registry() -> Arc<MobilePortRegistry> {
    init_registry()
}

pub fn feed_rx(path: &str, chunk: &[u8]) {
    global_registry().feed_rx(path, chunk);
}

/// JNI / Kotlin: Rust-side teardown only. Kotlin USB close runs on main thread separately.
pub fn on_usb_error(path: &str, reason: &str) {
    global_registry().fail_port(path, reason);
}

pub fn on_port_list_change() {
    port_list_monitor::request_refresh();
}

pub fn on_app_destroy() {
    global_registry().close_all();
}

#[cfg(all(test, target_os = "android"))]
mod fail_port_tests {
    use super::*;
    use crate::mobile_rx_hub::MobileRxHub;
    use crate::port_rx_hub::ExchangeWaiter;
    use crate::port_tx_queue::PortTxQueue;
    use crate::state::MobileConnectedPortHandle;
    use crate::{AtResultFormat, ExchangeCompletionMode, ResolvedExchangeOptions, RxPrepareMode};
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn test_handle(path: &str) -> MobileConnectedPortHandle {
        MobileConnectedPortHandle {
            path: path.to_string(),
            rx_hub: Arc::new(Mutex::new(None)),
            mux: Arc::new(Mutex::new(None)),
            exchange_cancel: Arc::new(AtomicBool::new(false)),
            tx_queue: Arc::new(PortTxQueue::new()),
            listening: Arc::new(AtomicBool::new(false)),
        }
    }

    #[test]
    fn fail_port_releases_exchange_waiter_quickly() {
        let registry = MobilePortRegistry::new();
        let hub = Arc::new(MobileRxHub::new("dev".into()));
        let handle = test_handle("dev");
        registry.register(hub.clone(), handle.clone());

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
        hub.shared().set_exchange_waiter(waiter.clone());

        let gate = Arc::new(AtomicBool::new(false));
        let gate_bg = gate.clone();
        let q = handle.tx_queue.clone();
        let t = std::thread::spawn(move || {
            gate_bg.store(true, std::sync::atomic::Ordering::SeqCst);
            q.run_serial(|| {
                std::thread::sleep(Duration::from_secs(30));
                Ok(())
            })
        });
        while !gate.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(2));
        }

        let start = Instant::now();
        registry.fail_port("dev", "usb error");
        let wait_result = waiter.wait(100);
        assert!(wait_result.is_err());
        assert!(start.elapsed() < Duration::from_millis(50));
        let join = t.join().unwrap();
        assert!(join.is_err());
    }

    #[test]
    fn fail_port_clear_halt_allows_next_tx_job() {
        let registry = MobilePortRegistry::new();
        let hub = Arc::new(MobileRxHub::new("dev2".into()));
        let handle = test_handle("dev2");
        registry.register(hub, handle.clone());
        registry.fail_port("dev2", "reset");
        let result = handle.tx_queue.run_serial(|| Ok(7));
        assert_eq!(result.unwrap(), 7);
    }
}
