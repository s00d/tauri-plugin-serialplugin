//! Unified serial API — `Box<dyn serialport::SerialPort>` on desktop and Android.

use crate::api::serial_port::{
    configure_at_session as configure_at_session_shared, run_exchange_queued,
};
use crate::at::session::{AtCommandOptions, AtPhase, AtSessionConfig, SendSmsPduOptions};
use crate::cmux::{mux_path, parse_mux_path, CmuxSession};
use crate::error::Error;
use crate::events::{ExchangeOptions, SerialEvent, WatchOptions};
#[cfg(mobile)]
use crate::log_error;
use crate::port::tx_queue::PortTxQueue;
use crate::state::{
    ClearBuffer, ConnectedPort, ConnectedPortHandle, DataBits, FlowControl, Parity, PortState,
    SerialportInfo, StopBits,
};
use crate::{log_debug, log_info};
use serialport::SerialPort as SerialPortTrait;
use std::collections::HashMap;
use std::io::Write;
#[cfg(mobile)]
use std::marker::PhantomData;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
#[cfg(desktop)]
use tauri::plugin::PluginHandle;
#[cfg(mobile)]
use tauri::Runtime;
#[cfg(desktop)]
use tauri::{AppHandle, Runtime};

/// Platform seam: enumerate available ports.
fn platform_available_ports(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    #[cfg(desktop)]
    {
        crate::port::list::enumerate_available_ports(single_port_per_device)
    }
    #[cfg(mobile)]
    {
        let json = crate::android::fd_bridge::call_enumerate_json()?;
        log_info!(
            "[SerialEnumerate] raw json ({} bytes): {}",
            json.len(),
            json
        );
        #[derive(serde::Deserialize)]
        struct AvailablePortsResponse {
            #[serde(default)]
            ports: HashMap<String, crate::android::enumerate::DeviceEntry>,
        }
        let response: AvailablePortsResponse = serde_json::from_str(&json).map_err(|e| {
            log_error!("[SerialEnumerate] Invalid enumerate JSON: {e}; body={json}");
            Error::new(format!("Invalid enumerate JSON: {e}"))
        })?;
        let filtered =
            crate::android::enumerate::expand_devices(response.ports, single_port_per_device)?;
        let keys: Vec<&str> = filtered.keys().map(|s| s.as_str()).collect();
        log_info!(
            "[SerialEnumerate] parsed {} port(s) single={} keys={:?}",
            filtered.len(),
            single_port_per_device,
            keys
        );
        Ok(filtered)
    }
}

/// Platform seam: open physical port as `Box<dyn serialport::SerialPort>`.
#[allow(clippy::too_many_arguments)]
fn platform_open(
    path: &str,
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
    timeout: Duration,
) -> Result<(String, Box<dyn SerialPortTrait>), Error> {
    #[cfg(desktop)]
    {
        let port = serialport::new(path, baud_rate)
            .data_bits(data_bits.into())
            .flow_control(flow_control.into())
            .parity(parity.into())
            .stop_bits(stop_bits.into())
            .timeout(timeout)
            .open()
            .map_err(|e| Error::String(format!("Failed to open serial port: {}", e)))?;
        Ok((path.to_string(), port))
    }
    #[cfg(mobile)]
    {
        let _ = timeout;
        crate::android::driver_host::global_host().open(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
        )
    }
}

/// Default read/write timeout (ms) when opening a port.
const DEFAULT_PORT_TIMEOUT_MS: u64 = 1000;
const PORT_LOCK_TIMEOUT_MS: u64 = 250;
const PORT_IO_TIMEOUT_MS: u64 = 100;

/// Acquire the port mutex with a deadline (hub reader uses try_lock; avoid blocking forever).
fn with_port_try_lock<T, F>(
    port: &Arc<Mutex<Box<dyn serialport::SerialPort>>>,
    lock_timeout: Duration,
    f: F,
) -> Result<T, Error>
where
    F: FnOnce(&mut Box<dyn serialport::SerialPort>) -> Result<T, Error>,
{
    let deadline = Instant::now() + lock_timeout;
    loop {
        match port.try_lock() {
            Ok(mut guard) => {
                let _ = guard.set_timeout(Duration::from_millis(PORT_IO_TIMEOUT_MS));
                return f(&mut guard);
            }
            Err(_) if Instant::now() >= deadline => {
                return Err(Error::String(format!(
                    "serial port lock timeout after {} ms",
                    lock_timeout.as_millis()
                )));
            }
            Err(_) => thread::sleep(Duration::from_millis(1)),
        }
    }
}

/// Write the full buffer, retrying until all bytes are sent or an error occurs.
fn write_all_port(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &[u8],
    operation: &str,
) -> Result<usize, Error> {
    port.write_all(buf)
        .map_err(|e| Error::String(format!("Failed to {}: {}", operation, e)))?;
    Ok(buf.len())
}

/// Tear down resources held by a [`SerialportInfo`] (RX hub + port).
fn finish_serialport_info(path: &str, info: SerialportInfo) -> Result<(), Error> {
    #[cfg(not(mobile))]
    let _ = path;
    match info.state {
        PortState::Connected(cp) => {
            if let Ok(mut hub_guard) = cp.rx_hub.lock() {
                if let Some(hub) = hub_guard.take() {
                    #[cfg(mobile)]
                    crate::android::registry::global_registry().unregister(path);
                    match Arc::try_unwrap(hub) {
                        Ok(hub) => hub.stop(),
                        Err(hub) => hub.request_stop(),
                    }
                }
            }
            #[cfg(mobile)]
            {
                let _ = crate::android::driver_host::global_host().close(Some(path));
            }
            Ok(())
        }
        PortState::Opening | PortState::Closed => Ok(()),
    }
}

fn register_mobile_hub(path: &str, cp: &ConnectedPortHandle, hub: Arc<crate::hub::PortRxHub>) {
    #[cfg(mobile)]
    {
        let hub_handle: Arc<dyn crate::hub::RxHubHandle> = hub;
        crate::android::registry::global_registry().register(path, hub_handle, cp.clone());
    }
    #[cfg(not(mobile))]
    {
        let _ = (path, cp, hub);
    }
}

fn ensure_rx_hub_running(cp: &ConnectedPortHandle, path: &str) -> Result<(), Error> {
    let mut guard = cp
        .rx_hub
        .lock()
        .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
    let needs_start = match guard.as_ref() {
        None => true,
        Some(hub) => hub.is_finished(),
    };
    if needs_start {
        let hub = Arc::new(crate::hub::PortRxHub::start(
            cp.port.clone(),
            path.to_string(),
        ));
        register_mobile_hub(path, cp, hub.clone());
        *guard = Some(hub);
    }
    Ok(())
}

fn ensure_rx_hub(cp: &ConnectedPortHandle, path: &str) -> Result<(), Error> {
    ensure_rx_hub_running(cp, path)
}

fn ensure_rx_hub_on_physical<R: Runtime>(
    serial: &SerialPort<R>,
    physical_path: &str,
) -> Result<(), Error> {
    let cp = serial.resolve_connected_port(physical_path)?;
    ensure_rx_hub(&cp, physical_path)
}

/// Access to the serial port APIs (desktop + Android).
///
/// Lock order: `serialports` before `virtual_ports`; never hold a map lock across
/// device I/O, thread spawn, or `JoinHandle` join.
pub struct SerialPort<R: Runtime> {
    #[cfg(desktop)]
    #[allow(dead_code)]
    pub(crate) app: AppHandle<R>,
    #[cfg(mobile)]
    _runtime: PhantomData<fn() -> R>,
    pub(crate) serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
    pub(crate) virtual_ports: Arc<Mutex<HashMap<String, crate::state::VirtualPortRef>>>,
}

impl<R: Runtime> Clone for SerialPort<R> {
    fn clone(&self) -> Self {
        Self {
            #[cfg(desktop)]
            app: self.app.clone(),
            #[cfg(mobile)]
            _runtime: PhantomData::<fn() -> R>,
            serialports: Arc::clone(&self.serialports),
            virtual_ports: Arc::clone(&self.virtual_ports),
        }
    }
}

impl<R: Runtime> SerialPort<R> {
    #[cfg(mobile)]
    pub fn new() -> Self {
        Self::default()
    }
}

#[cfg(mobile)]
impl<R: Runtime> Default for SerialPort<R> {
    fn default() -> Self {
        Self {
            _runtime: PhantomData::<fn() -> R>,
            serialports: Arc::new(Mutex::new(HashMap::new())),
            virtual_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<R: Runtime> SerialPort<R> {
    #[cfg(mobile)]
    pub fn setup_teardown(&self) {
        let serialports = self.serialports.clone();
        let virtual_ports = self.virtual_ports.clone();
        crate::android::registry::set_rust_state_fail(Box::new(move |path, _reason| {
            if let Err(e) = Self::rust_fail_port_state(&serialports, &virtual_ports, path) {
                crate::log_warn!("rust_fail_port_state {}: {}", path, e);
            }
        }));
        crate::port::list_monitor::set_android_enumerator(move |single| {
            platform_available_ports(single)
        });
        crate::android::registry::init_registry();
    }

    #[cfg(mobile)]
    fn rust_fail_port_state(
        ports: &Arc<Mutex<HashMap<String, SerialportInfo>>>,
        virtual_ports: &Arc<Mutex<HashMap<String, crate::state::VirtualPortRef>>>,
        path: &str,
    ) -> Result<(), Error> {
        if let Ok(mut v) = virtual_ports.lock() {
            if let Some(vp) = v.remove(path) {
                vp.tx_queue.cancel_all();
                vp.tx_queue.clear_halt();
            }
        }
        let mut map = ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
        map.remove(path);
        Ok(())
    }

    #[cfg(desktop)]
    #[allow(dead_code)]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            serialports: Arc::new(Mutex::new(HashMap::new())),
            virtual_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    #[cfg(desktop)]
    #[allow(dead_code)]
    pub fn from_plugin_handle(plugin_handle: PluginHandle<R>) -> Self {
        Self {
            app: plugin_handle.app().clone(),
            serialports: Arc::new(Mutex::new(HashMap::new())),
            virtual_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Get serial port list
    pub fn available_ports(
        &self,
        single_port_per_device: bool,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        platform_available_ports(single_port_per_device)
    }

    /// Subscribe to available-port attach/detach events via a Tauri channel.
    pub fn watch_ports(
        &self,
        options: crate::events::WatchPortsOptions,
        channel: tauri::ipc::Channel<crate::events::PortListEvent>,
    ) -> Result<u32, Error> {
        let channel_id = channel.id();
        crate::port::list_monitor::subscribe(channel_id, channel, options)?;
        Ok(channel_id)
    }

    /// Stop a port-list watch session.
    pub fn unwatch_ports(&self, channel_id: u32) -> Result<(), Error> {
        crate::port::list_monitor::unsubscribe(channel_id);
        Ok(())
    }

    /// Get a list of managed serial ports.
    pub fn managed_ports(&self) -> Result<Vec<String>, Error> {
        // Lock the Mutex to safely access the data inside `self.serialports`.
        let ports = self
            .serialports
            .lock()
            .map_err(|_| Error::String("Failed to lock serialports mutex".to_string()))?;

        let mut port_list: Vec<String> = ports.keys().cloned().collect();
        if let Ok(virtuals) = self.virtual_ports.lock() {
            port_list.extend(virtuals.keys().cloned());
        }
        port_list.sort();
        Ok(port_list)
    }

    /// Cancel an in-flight poll [`read`] (does not stop an active [`watch`]).
    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        let cp = self.resolve_connected_port(&path)?;
        if let Ok(guard) = cp.rx_hub.lock() {
            if let Some(hub) = guard.as_ref() {
                hub.shared().cancel_pending_read();
            }
        }
        Ok(())
    }

    /// Close the specified serial port
    pub fn close(&self, path: String) -> Result<(), Error> {
        log_debug!("close {}", path);

        for channel_id in crate::port::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }

        if let Ok(mut virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.remove(&path) {
                vp.tx_queue.cancel_all();
                let physical = vp.physical_path.clone();
                let dlci = vp.dlci;
                drop(virtuals);
                if let Ok(cp) = self.resolve_connected_port(&physical) {
                    if let Some(session) = crate::sync_util::lock_or_recover(&cp.mux).clone() {
                        session.unregister_dlci(dlci);
                    }
                }
                return Ok(());
            }
        }

        let removed = match self.serialports.lock() {
            Ok(mut serialports) => serialports.remove(&path),
            Err(error) => {
                return Err(Error::String(format!("Failed to acquire lock: {}", error)));
            }
        };

        if let Some(port_info) = removed {
            log_debug!("stop {}", path);
            finish_serialport_info(&path, port_info)?;
            log_debug!("end {}", path);
            Ok(())
        } else {
            Err(Error::String(format!("Serial port {} is not open!", path)))
        }
    }

    /// Close all open serial ports
    pub fn close_all(&self) -> Result<(), Error> {
        let paths = self.managed_ports()?;
        let mut errors = vec![];

        for path in paths {
            if let Err(e) = self.close(path) {
                errors.push(e.to_string());
            }
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(Error::String(errors.join(", ")))
        }
    }

    /// Force close a serial port
    pub fn force_close(&self, path: String) -> Result<(), Error> {
        for channel_id in crate::port::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }

        match self.serialports.lock() {
            Ok(mut map) => {
                if let Some(serial) = map.remove(&path) {
                    finish_serialport_info(&path, serial)?;
                }
                let _ = self.virtual_ports.lock().map(|mut v| v.remove(&path));
                Ok(())
            }
            Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn open(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
    ) -> Result<String, Error> {
        let stale = {
            let mut serialports = self
                .serialports
                .lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;
            if matches!(
                serialports.get(&path).map(|i| &i.state),
                Some(PortState::Opening)
            ) {
                return Err(Error::String(format!(
                    "open already in progress for {}",
                    path
                )));
            }
            let stale = serialports.remove(&path);
            serialports.insert(
                path.clone(),
                SerialportInfo {
                    state: PortState::Opening,
                },
            );
            stale
        };

        if let Some(existing) = stale {
            log_info!("Replacing existing port {}", path);
            finish_serialport_info(&path, existing)?;
        }

        let timeout_ms = timeout.unwrap_or(DEFAULT_PORT_TIMEOUT_MS);
        let port_result = platform_open(
            &path,
            baud_rate,
            data_bits.unwrap_or(DataBits::Eight),
            flow_control.unwrap_or(FlowControl::None),
            parity.unwrap_or(Parity::None),
            stop_bits.unwrap_or(StopBits::One),
            Duration::from_millis(timeout_ms),
        );

        let mut serialports = self
            .serialports
            .lock()
            .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

        match port_result {
            Ok((session_path, port)) => {
                if session_path != path {
                    serialports.remove(&path);
                    if !serialports.contains_key(&session_path) {
                        serialports.insert(
                            session_path.clone(),
                            SerialportInfo {
                                state: PortState::Opening,
                            },
                        );
                    }
                }

                let cp = ConnectedPort::new(port);
                ensure_rx_hub(&cp.handle(), &session_path)?;
                let entry = serialports.get_mut(&session_path).ok_or_else(|| {
                    Error::String(format!("Port '{}' disappeared during open", session_path))
                })?;
                if !matches!(entry.state, PortState::Opening) {
                    return Err(Error::String(format!(
                        "Port '{}' state changed during open",
                        session_path
                    )));
                }
                entry.state = PortState::Connected(cp);
                Ok(session_path)
            }
            Err(e) => {
                if matches!(
                    serialports.get(&path).map(|i| &i.state),
                    Some(PortState::Opening)
                ) {
                    serialports.remove(&path);
                }
                Err(e)
            }
        }
    }

    /// Stream port data through a Tauri IPC channel (v3). Subscribes to the port RX hub.
    pub fn watch(
        &self,
        path: String,
        options: WatchOptions,
        channel: Channel<SerialEvent>,
    ) -> Result<u32, Error> {
        let channel_id = channel.id();
        crate::port::watch_registry::register(channel_id, path.clone())?;
        log_debug!("Starting watch on port: {} (channel {})", path, channel_id);

        let batch_timeout = options
            .serial_data_flush_interval_ms
            .or(options.timeout)
            .unwrap_or(DEFAULT_PORT_TIMEOUT_MS);
        let read_size = options.size.unwrap_or(1024);

        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path).cloned() {
                drop(virtuals);
                let session = {
                    let cp = self.resolve_connected_port(&vp.physical_path)?;
                    let mux = cp
                        .mux
                        .lock()
                        .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                        .clone()
                        .ok_or_else(|| Error::String("CMUX not enabled".into()))?;
                    mux
                };
                ensure_rx_hub_on_physical(self, &vp.physical_path)?;
                session.set_watch(vp.dlci, channel, batch_timeout);
                return Ok(channel_id);
            }
        }

        if let Err(e) = self.with_connected_port(path.clone(), |cp| {
            if cp.virtual_dlci.is_some() {
                return Err(Error::String("legacy virtual ConnectedPort".into()));
            }
            if cp
                .rx_hub
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|h| h.shared().has_watch()))
                .unwrap_or(false)
            {
                if let Ok(guard) = cp.rx_hub.lock() {
                    if let Some(hub) = guard.as_ref() {
                        hub.detach_watch();
                    }
                }
            }
            ensure_rx_hub(&cp.handle(), &path)?;
            let guard = cp
                .rx_hub
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            let hub = guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing after start".into()))?;
            hub.attach_watch(channel, batch_timeout, read_size);
            Ok(())
        }) {
            crate::port::watch_registry::unregister(channel_id);
            return Err(e);
        }

        Ok(channel_id)
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        let path = crate::port::watch_registry::unregister(channel_id)
            .ok_or_else(|| Error::new(format!("No active watch for channel {}", channel_id)))?;
        log_debug!("Stopping watch on port: {} (channel {})", path, channel_id);
        self.stop_watch_thread(&path)
    }

    fn stop_watch_thread(&self, path: &str) -> Result<(), Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(path).cloned() {
                drop(virtuals);
                if let Ok(cp) = self.resolve_connected_port(&vp.physical_path) {
                    if let Some(session) = crate::sync_util::lock_or_recover(&cp.mux).clone() {
                        session.clear_watch(vp.dlci);
                    }
                }
                return Ok(());
            }
        }
        self.with_connected_port(path.to_string(), |cp| {
            if let Some(dlci) = cp.virtual_dlci {
                if let Some(session) = crate::sync_util::lock_or_recover(&cp.mux).clone() {
                    session.clear_watch(dlci);
                }
                return Ok(());
            }
            if let Ok(guard) = cp.rx_hub.lock() {
                if let Some(hub) = guard.as_ref() {
                    hub.detach_watch();
                }
            }
            Ok(())
        })
    }

    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        let bytes = self.read_via_hub(path, timeout, size, false)?;
        Ok(String::from_utf8_lossy(&bytes).to_string())
    }

    pub fn read_binary(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<Vec<u8>, Error> {
        self.read_via_hub(path, timeout, size, true)
    }

    fn read_via_hub(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
        fill: bool,
    ) -> Result<Vec<u8>, Error> {
        let cp = self.resolve_connected_port(&path)?;
        ensure_rx_hub_running(&cp, &path)?;
        let hub_shared = {
            let guard = cp
                .rx_hub
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing".into()))?
                .shared()
        };
        hub_shared
            .read_request(size.unwrap_or(1024), timeout.unwrap_or(1000), fill)
            .map_err(Error::String)
    }

    /// Write data to the serial port
    pub fn write(&self, path: String, value: String) -> Result<usize, Error> {
        self.get_serialport(path.clone(), |port| {
            write_all_port(port, value.as_bytes(), "write data")
        })
    }

    /// Write binary data to the serial port
    pub fn write_binary(&self, path: String, value: Vec<u8>) -> Result<usize, Error> {
        self.get_serialport(path.clone(), |port| {
            write_all_port(port, &value, "write binary data")
        })
    }

    /// Set the baud rate
    pub fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.set_baud_rate(baud_rate)
                .map_err(|e| Error::String(format!("Failed to set baud rate: {}", e)))
        })
    }

    /// Set the data bits
    pub fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.set_data_bits(data_bits.into()).map_err(Error::from)
        })
    }

    /// Set the flow control
    pub fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.set_flow_control(flow_control.into())
                .map_err(Error::from)
        })
    }

    /// Set the parity
    pub fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.set_parity(parity.into()).map_err(Error::from)
        })
    }

    /// Set the stop bits
    pub fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.set_stop_bits(stop_bits.into()).map_err(Error::from)
        })
    }

    /// Set the timeout
    pub fn set_timeout(&self, path: String, timeout: Duration) -> Result<(), Error> {
        self.get_serialport(path, |port| port.set_timeout(timeout).map_err(Error::from))
    }

    /// Set the RTS (Request To Send) control signal
    pub fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.write_request_to_send(level).map_err(Error::from)
        })
    }

    /// Set the DTR (Data Terminal Ready) control signal
    pub fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.write_data_terminal_ready(level).map_err(Error::from)
        })
    }

    /// Read the CTS (Clear To Send) control signal state
    pub fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port| port.read_clear_to_send().map_err(Error::from))
    }

    /// Read the DSR (Data Set Ready) control signal state
    pub fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port| port.read_data_set_ready().map_err(Error::from))
    }

    /// Read the RI (Ring Indicator) control signal state
    pub fn read_ring_indicator(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port| port.read_ring_indicator().map_err(Error::from))
    }

    /// Read the CD (Carrier Detect) control signal state
    pub fn read_carrier_detect(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port| port.read_carrier_detect().map_err(Error::from))
    }

    /// Get the number of bytes available to read
    pub fn bytes_to_read(&self, path: String) -> Result<u32, Error> {
        let hub_buffered = if let Ok(cp) = self.resolve_connected_port(&path) {
            cp.rx_hub
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|hub| hub.shared().buffered_len() as u32))
                .unwrap_or(0)
        } else {
            0
        };
        let port_buffered = if let Ok(cp) = self.resolve_connected_port(&path) {
            cp.port
                .try_lock()
                .ok()
                .and_then(|port| port.bytes_to_read().ok())
                .unwrap_or(0)
        } else {
            0
        };
        Ok(hub_buffered.saturating_add(port_buffered))
    }

    /// Get the number of bytes waiting to be written
    pub fn bytes_to_write(&self, path: String) -> Result<u32, Error> {
        self.get_serialport(path, |port| port.bytes_to_write().map_err(Error::from))
    }

    /// Clear input/output buffers
    pub fn clear_buffer(&self, path: String, buffer_to_clear: ClearBuffer) -> Result<(), Error> {
        self.get_serialport(path, |port| {
            port.clear(buffer_to_clear.into()).map_err(Error::from)
        })
    }

    /// Start break signal transmission
    pub fn set_break(&self, path: String) -> Result<(), Error> {
        self.get_serialport(path, |port| port.set_break().map_err(Error::from))
    }

    /// Stop break signal transmission
    pub fn clear_break(&self, path: String) -> Result<(), Error> {
        self.get_serialport(path, |port| port.clear_break().map_err(Error::from))
    }

    /// Write payload and read until terminators, idle silence, or timeout (AT-style exchange).
    pub fn exchange(
        &self,
        path: String,
        value: String,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value.into_bytes(), options)
    }

    /// Binary variant of [`Self::exchange`].
    pub fn exchange_binary(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value, options)
    }

    fn get_tx_queue(&self, path: &str) -> Result<Arc<PortTxQueue>, Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(path) {
                return Ok(vp.tx_queue.clone());
            }
        }
        let ports = self
            .serialports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
        let info = ports
            .get(path)
            .ok_or_else(|| Error::String(format!("Port '{}' not found", path)))?;
        match &info.state {
            PortState::Connected(cp) => Ok(cp.tx_queue.clone()),
            other => Err(Error::String(other.not_connected_reason())),
        }
    }

    fn exchange_bytes(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        run_exchange_queued(
            path,
            options,
            |p| self.get_tx_queue(p),
            |physical_path,
             dlci,
             virtual_path,
             payload,
             options|
             -> Result<crate::at::parse::ExchangeResponse, Error> {
                self.exchange_bytes_mux_direct(physical_path, dlci, virtual_path, payload, options)
            },
            |path, payload, options| -> Result<crate::at::parse::ExchangeResponse, Error> {
                self.exchange_bytes_direct(path, payload, options)
            },
            payload,
        )
    }

    fn exchange_bytes_direct(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();
        let resolved_timeout = options.resolve().timeout_ms;

        let cp = self.resolve_connected_port(&path)?;
        ensure_rx_hub(&cp, &path)?;

        struct DesktopExchangeIo<'a> {
            port: &'a Arc<Mutex<Box<dyn serialport::SerialPort>>>,
            timeout_ms: u64,
        }
        impl crate::exchange::io::ExchangeIo for DesktopExchangeIo<'_> {
            fn purge_rx(&self) -> Result<(), Error> {
                with_port_try_lock(
                    self.port,
                    Duration::from_millis(self.timeout_ms.min(5000)),
                    |port| port.clear(ClearBuffer::Input.into()).map_err(Error::from),
                )
            }
            fn write_payload(&self, payload: &[u8]) -> Result<(), Error> {
                with_port_try_lock(
                    self.port,
                    Duration::from_millis(self.timeout_ms.min(5000)),
                    |port| write_all_port(port, payload, "exchange write").map(|_| ()),
                )
            }
        }

        let hub = {
            let guard = crate::sync_util::lock_or_recover(&cp.rx_hub);
            guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing".into()))?
                .clone()
        };

        crate::exchange::run::run_physical_exchange(
            hub.as_ref(),
            &DesktopExchangeIo {
                port: &cp.port,
                timeout_ms: resolved_timeout,
            },
            &command,
            &user_solicited,
            payload,
            options,
            cp.exchange_cancel.clone(),
        )
    }

    fn exchange_bytes_mux_direct(
        &self,
        physical_path: String,
        _dlci: u8,
        virtual_path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let vp = self
            .virtual_ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .get(&virtual_path)
            .cloned()
            .ok_or_else(|| Error::String(format!("Virtual port '{}' not open", virtual_path)))?;
        self.exchange_bytes_mux_via_ref(physical_path, &vp, payload, options)
    }

    fn exchange_bytes_mux_via_ref(
        &self,
        physical_path: String,
        vp: &crate::state::VirtualPortRef,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        let dlci = vp.dlci;
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();

        let session = {
            let ports = self
                .serialports
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            let info = ports.get(&physical_path).ok_or_else(|| {
                Error::String(format!("Physical port '{}' not found", physical_path))
            })?;
            match &info.state {
                PortState::Connected(cp) => cp
                    .mux
                    .lock()
                    .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                    .clone()
                    .ok_or_else(|| Error::String("CMUX not enabled on physical port".into()))?,
                other => return Err(Error::String(other.not_connected_reason())),
            }
        };

        ensure_rx_hub_on_physical(self, &physical_path)?;

        crate::exchange::run::run_mux_exchange(
            &session,
            dlci,
            &command,
            &user_solicited,
            payload,
            options,
            vp.exchange_cancel.clone(),
        )
    }

    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        if let Some((physical, dlci)) = parse_mux_path(path.as_str()) {
            return self.exchange_bytes_mux_direct(
                physical.to_string(),
                dlci,
                path,
                payload,
                options,
            );
        }
        self.exchange_bytes_direct(path, payload, options)
    }

    pub fn at(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at::parse::AtCommandResult, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_at(self, &tx_queue, path, command, options)
    }

    /// Multi-phase AT flow (e.g. CMGS) — phases run atomically without interleaving other jobs.
    pub fn at_phases(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_at_phases(self, &tx_queue, path, phases)
    }

    /// Built-in CMGS recipe: `AT+CMGS=n` → `>` → PDU + Ctrl+Z → final line.
    pub fn send_sms_pdu(
        &self,
        path: String,
        length: u32,
        pdu: Vec<u8>,
        options: Option<SendSmsPduOptions>,
    ) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        crate::at::commands::queue_send_sms_pdu(self, &tx_queue, path, length, pdu, options)
    }

    /// Configure AT session defaults for native `at` jobs on this port.
    pub fn configure_at_session(
        &self,
        path: String,
        session: AtSessionConfig,
    ) -> Result<(), Error> {
        let tx_queue = self
            .resolve_connected_port(&path)
            .map(|cp| cp.tx_queue.clone())
            .ok();
        configure_at_session_shared(&self.virtual_ports, tx_queue, &path, session)
    }

    /// Enter GSM 07.10 CMUX mode (`AT+CMUX=…` then attach deframer to the RX hub).
    pub fn enable_mux(&self, path: String, command: String, timeout_ms: u64) -> Result<(), Error> {
        self.exchange(
            path.clone(),
            format!("{command}\r"),
            ExchangeOptions {
                timeout_ms: Some(timeout_ms),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
                ..Default::default()
            },
        )?;

        let cp = self.resolve_connected_port(&path)?;
        if cp.virtual_dlci.is_some() {
            return Err(Error::String("enable_mux on virtual port".into()));
        }
        if crate::sync_util::lock_or_recover(&cp.mux).is_some() {
            return Err(Error::String("CMUX already enabled".into()));
        }
        cp.port
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .clear(ClearBuffer::Input.into())
            .map_err(Error::from)?;
        let session = CmuxSession::new(
            path.clone(),
            Arc::new(crate::cmux::SerialPortIo(cp.port.clone())),
        );
        ensure_rx_hub(&cp, &path)?;
        if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
            hub.attach_cmux(session.clone());
        }
        *crate::sync_util::lock_or_recover(&cp.mux) = Some(session);
        Ok(())
    }

    /// Register a virtual DLCI channel on an CMUX-enabled physical port.
    pub fn open_mux_channel(&self, physical_path: String, dlci: u8) -> Result<String, Error> {
        let virtual_path = mux_path(&physical_path, dlci);
        let session = {
            let ports = self
                .serialports
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            let info = ports
                .get(&physical_path)
                .ok_or_else(|| Error::String(format!("Port '{}' not found", physical_path)))?;
            match &info.state {
                PortState::Connected(cp) => cp
                    .mux
                    .lock()
                    .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                    .clone()
                    .ok_or_else(|| Error::String("CMUX not enabled".into())),
                other => Err(Error::String(other.not_connected_reason())),
            }
        }?;

        if self
            .virtual_ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .contains_key(&virtual_path)
        {
            return Ok(virtual_path);
        }

        session.register_dlci(dlci, virtual_path.clone());
        self.virtual_ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .insert(
                virtual_path.clone(),
                crate::state::VirtualPortRef {
                    physical_path,
                    dlci,
                    exchange_cancel: Arc::new(std::sync::atomic::AtomicBool::new(false)),
                    tx_queue: Arc::new(PortTxQueue::new()),
                },
            );
        Ok(virtual_path)
    }

    /// Tear down CMUX: close virtual channels and detach deframer.
    pub fn disable_mux(&self, path: String) -> Result<(), Error> {
        let virtual_paths: Vec<String> = {
            let mut paths: Vec<String> = self
                .virtual_ports
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                .iter()
                .filter(|(_p, vp)| vp.physical_path == path)
                .map(|(p, _)| p.clone())
                .collect();
            if paths.is_empty() {
                let ports = self.serialports.lock().unwrap();
                paths = ports
                    .keys()
                    .filter(|p| {
                        parse_mux_path(p).map(|(base, _)| base == path.as_str()) == Some(true)
                    })
                    .cloned()
                    .collect();
            }
            paths
        };
        for vp in virtual_paths {
            let _ = self.close(vp);
        }
        self.with_connected_port(path.clone(), |cp| {
            if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
                hub.detach_cmux();
            }
            *crate::sync_util::lock_or_recover(&cp.mux) = None;
            Ok(())
        })
    }

    /// Cancel an in-flight exchange and reject queued waiters on `path`.
    pub fn cancel_exchange(&self, path: String) -> Result<(), Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path).cloned() {
                drop(virtuals);
                let session = {
                    let ports = self
                        .serialports
                        .lock()
                        .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
                    let info = ports.get(&vp.physical_path).ok_or_else(|| {
                        Error::String(format!("Physical port '{}' not found", vp.physical_path))
                    })?;
                    match &info.state {
                        PortState::Connected(cp) => cp
                            .mux
                            .lock()
                            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                            .clone(),
                        other => return Err(Error::String(other.not_connected_reason())),
                    }
                };
                if let Some(session) = session {
                    crate::exchange::cancel::cancel_virtual_exchange(
                        &vp.exchange_cancel,
                        &vp.tx_queue,
                        &session,
                        vp.dlci,
                    );
                } else {
                    vp.exchange_cancel.store(true, Ordering::SeqCst);
                    vp.tx_queue.cancel_all();
                    vp.tx_queue.clear_halt();
                }
                return Ok(());
            }
        }
        let cp = self.resolve_connected_port(&path)?;
        crate::exchange::cancel::cancel_physical_exchange(
            &cp.exchange_cancel,
            &cp.tx_queue,
            || {
                if let Ok(guard) = cp.rx_hub.lock() {
                    if let Some(hub) = guard.as_ref() {
                        hub.cancel_active_exchange();
                    }
                }
            },
        );
        Ok(())
    }

    /// Resolve a connected port's shared handles (brief global lock only).
    fn resolve_connected_port(&self, path: &str) -> Result<ConnectedPortHandle, Error> {
        let ports = self
            .serialports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
        let info = ports
            .get(path)
            .ok_or_else(|| Error::String(format!("Port '{}' not found", path)))?;
        match &info.state {
            PortState::Connected(cp) => Ok(cp.handle()),
            other => Err(Error::String(other.not_connected_reason())),
        }
    }

    /// Run `f` only when the port is [`PortState::Connected`] (holds global lock for `f`).
    fn with_connected_port<T, F>(&self, path: String, f: F) -> Result<T, Error>
    where
        F: FnOnce(&mut ConnectedPort) -> Result<T, Error>,
    {
        let mut ports = self
            .serialports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;

        let info = ports
            .get_mut(&path)
            .ok_or_else(|| Error::String(format!("Port '{}' not found", path)))?;

        match &mut info.state {
            PortState::Connected(cp) => f(cp),
            other => Err(Error::String(other.not_connected_reason())),
        }
    }

    fn get_serialport<T, F>(&self, path: String, f: F) -> Result<T, Error>
    where
        F: FnOnce(&mut Box<dyn serialport::SerialPort>) -> Result<T, Error>,
    {
        let cp = self.resolve_connected_port(&path)?;
        with_port_try_lock(&cp.port, Duration::from_millis(PORT_LOCK_TIMEOUT_MS), f)
    }
}

impl<R: Runtime> crate::at::commands::ExchangeRunner for SerialPort<R> {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        SerialPort::run_exchange_unqueued(self, path, payload, options)
    }
}
