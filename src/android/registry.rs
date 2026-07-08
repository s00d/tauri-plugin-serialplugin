//! Global registry for USB failure callbacks (path → hub + port handles).
//! RX bytes reach the hub via Rust reader polling, not a legacy Kotlin push path.

use crate::hub::RxHubHandle;
use crate::port::list_monitor;
use crate::port::watch_registry;
use crate::state::ConnectedPortHandle;
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
    hub: Arc<dyn RxHubHandle>,
    handle: ConnectedPortHandle,
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

    pub fn register(&self, path: &str, hub: Arc<dyn RxHubHandle>, handle: ConnectedPortHandle) {
        crate::sync_util::lock_or_recover(&self.ports)
            .insert(path.to_string(), PortEntry { hub, handle });
    }

    pub fn unregister(&self, path: &str) {
        crate::sync_util::lock_or_recover(&self.ports).remove(path);
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

        hub.shutdown_hub();
        self.unregister(path);

        if let Some(f) = RUST_STATE_FAIL.get() {
            f(path, reason);
        }
    }

    pub fn handle_for(&self, path: &str) -> Option<ConnectedPortHandle> {
        crate::sync_util::lock_or_recover(&self.ports)
            .get(path)
            .map(|e| e.handle.clone())
    }

    pub fn close_all(&self) {
        let entries: Vec<(String, Arc<dyn RxHubHandle>)> =
            crate::sync_util::lock_or_recover(&self.ports)
                .iter()
                .map(|(path, entry)| (path.clone(), entry.hub.clone()))
                .collect();
        for (path, hub) in entries {
            hub.shutdown_hub();
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

/// Hub teardown after USB/driver failure.
pub fn on_usb_error(path: &str, reason: &str) {
    global_registry().fail_port(path, reason);
}

pub fn on_port_list_change() {
    list_monitor::request_refresh();
}

pub fn on_device_detached(device_name: &str) {
    let _ = device_name;
    list_monitor::request_refresh();
}

pub fn on_app_destroy() {
    global_registry().close_all();
    let _ = crate::android::driver_host::global_host().close(None);
}

/// Debug-only helpers for Kotlin ↔ Rust JNI integration tests.
#[cfg(all(debug_assertions, target_os = "android"))]
pub mod test_harness {
    use super::*;
    use crate::hub::PortRxHub;
    use crate::mock_serial::MockSerialPort;
    use crate::state::ConnectedPort;
    use serialport::SerialPort;
    use std::sync::Arc;

    pub fn reset() {
        let _ = crate::android::driver_host::global_host().close(None);
        global_registry().close_all();
    }

    pub fn register_port(path: &str) {
        let port: Box<dyn SerialPort> = Box::new(MockSerialPort::new());
        let cp = ConnectedPort::new(port);
        let hub = Arc::new(PortRxHub::start(cp.port.clone(), path.to_string()));
        *cp.rx_hub.lock().unwrap() = Some(hub.clone());
        let hub_handle: Arc<dyn RxHubHandle> = hub;
        global_registry().register(path, hub_handle, cp.handle());
    }

    fn hub_for(path: &str) -> Option<Arc<dyn RxHubHandle>> {
        let registry = global_registry();
        let ports = crate::sync_util::lock_or_recover(&registry.ports);
        ports.get(path).map(|e| e.hub.clone())
    }

    pub fn hub_buffered_len(path: &str) -> i64 {
        hub_for(path)
            .map(|hub| hub.shared().buffered_len() as i64)
            .unwrap_or(-1)
    }

    pub fn hub_take_idle(path: &str) -> Vec<u8> {
        hub_for(path)
            .map(|hub| hub.shared().take_idle_bytes())
            .unwrap_or_default()
    }

    pub fn registry_has_port(path: &str) -> bool {
        global_registry().handle_for(path).is_some()
    }

    pub fn invoke_write(path: &str, data: &[u8]) -> Result<usize, crate::error::Error> {
        crate::android::driver_host::global_host().write(path, data)
    }

    #[cfg(feature = "android-test-harness")]
    fn cdc_dual_iface_fake() -> android_usb_serial::FakeTransport {
        use android_usb_serial::transport::{EndpointInfo, InterfaceInfo};
        let fake = android_usb_serial::FakeTransport::cdc_single_iface();
        fake.set_interfaces(vec![
            InterfaceInfo {
                id: 0,
                class: 2,
                subclass: 2,
                protocol: 0,
            },
            InterfaceInfo {
                id: 1,
                class: 10,
                subclass: 0,
                protocol: 0,
            },
        ]);
        fake.configure_endpoints(&[(
            1,
            vec![
                EndpointInfo {
                    address: 0x81,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
                EndpointInfo {
                    address: 0x02,
                    attributes: 2,
                    max_packet_size: 64,
                    interval: 0,
                },
            ],
        )]);
        fake
    }

    #[cfg(feature = "android-test-harness")]
    pub fn open_fake_port(device_name: &str) -> Result<String, crate::error::Error> {
        use crate::state::{DataBits, FlowControl, Parity, StopBits};
        use std::sync::Arc;

        let fake = Arc::new(cdc_dual_iface_fake());
        crate::android::driver_host::global_host().inject_fake_device(device_name, fake);
        let (session, _port) = crate::android::driver_host::global_host().open(
            device_name,
            115_200,
            DataBits::Eight,
            FlowControl::None,
            Parity::None,
            StopBits::One,
        )?;
        register_port(&session);
        Ok(session)
    }

    #[cfg(feature = "android-test-harness")]
    pub fn fake_inject_rx(device_name: &str, data: &[u8]) -> bool {
        crate::android::driver_host::global_host()
            .fake_transport(device_name)
            .map(|f| {
                f.push_rx(data);
                true
            })
            .unwrap_or(false)
    }

    #[cfg(feature = "android-test-harness")]
    pub fn fake_take_tx(device_name: &str) -> Vec<u8> {
        crate::android::driver_host::global_host()
            .fake_transport(device_name)
            .map(|f| f.take_tx())
            .unwrap_or_default()
    }

    #[cfg(feature = "android-test-harness")]
    pub fn fake_inject_error(device_name: &str, reason: &str) -> bool {
        crate::android::driver_host::global_host()
            .fake_transport(device_name)
            .map(|f| {
                f.inject_bulk_read_error(reason);
                true
            })
            .unwrap_or(false)
    }
}

#[cfg(all(test, target_os = "android"))]
mod fail_port_tests {
    use super::*;
    use crate::hub::ExchangeWaiter;
    use crate::hub::PortRxHub;
    use crate::mock_serial::MockSerialPort;
    use crate::port::tx_queue::PortTxQueue;
    use crate::state::{ConnectedPort, ConnectedPortHandle};
    use crate::{AtResultFormat, ExchangeCompletionMode, ResolvedExchangeOptions, RxPrepareMode};
    use serialport::SerialPort;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    fn test_handle(path: &str, port: Arc<Mutex<Box<dyn SerialPort>>>) -> ConnectedPortHandle {
        ConnectedPortHandle {
            port,
            rx_hub: Arc::new(Mutex::new(None)),
            mux: Arc::new(Mutex::new(None)),
            virtual_dlci: None,
            physical_path: None,
            exchange_cancel: Arc::new(AtomicBool::new(false)),
            tx_queue: Arc::new(PortTxQueue::new()),
        }
    }

    #[test]
    fn fail_port_releases_exchange_waiter_quickly() {
        let registry = MobilePortRegistry::new();
        let port: Arc<Mutex<Box<dyn SerialPort>>> =
            Arc::new(Mutex::new(Box::new(MockSerialPort::new())));
        let hub = Arc::new(PortRxHub::start(port.clone(), "dev".into()));
        let handle = test_handle("dev", port);
        registry.register("dev", hub.clone(), handle.clone());

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
        let port: Arc<Mutex<Box<dyn SerialPort>>> =
            Arc::new(Mutex::new(Box::new(MockSerialPort::new())));
        let hub = Arc::new(PortRxHub::start(port.clone(), "dev2".into()));
        let handle = test_handle("dev2", port);
        registry.register("dev2", hub, handle.clone());
        registry.fail_port("dev2", "reset");
        let result = handle.tx_queue.run_serial(|| Ok(7));
        assert_eq!(result.unwrap(), 7);
    }
}

#[cfg(test)]
#[cfg(mobile)]
mod hub_cancel_tests {
    use super::*;
    use crate::hub::ExchangeWaiter;
    use crate::hub::PortRxHub;
    use crate::mock_serial::MockSerialPort;
    use crate::{AtResultFormat, ExchangeCompletionMode, ResolvedExchangeOptions, RxPrepareMode};
    use serialport::SerialPort;
    use std::sync::atomic::AtomicBool;
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    #[test]
    fn cancel_active_exchange_wakes_waiter_quickly() {
        let port: Arc<Mutex<Box<dyn SerialPort>>> =
            Arc::new(Mutex::new(Box::new(MockSerialPort::new())));
        let hub = Arc::new(PortRxHub::start(port, "dev-cancel".into()));
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
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        };
        let waiter = ExchangeWaiter::new(options, cancel.clone());
        hub.set_exchange_waiter(waiter.clone());

        cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        hub.cancel_active_exchange();

        let start = Instant::now();
        let result = waiter.wait(5000);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("cancel"), "expected cancel error, got {err}");
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "cancel took too long: {:?}",
            start.elapsed()
        );
    }
}
