use crate::at_session::{
    check_expect_ok, normalize_at_command, AtCommandOptions, AtPhase, AtPhaseWrite,
    AtSessionConfig, SendSmsPduOptions,
};
use crate::cmux::{mux_path, parse_mux_path, CmuxSession};
use crate::error::Error;
use crate::events::{ExchangeOptions, SerialEvent, WatchOptions};
use crate::port_tx_queue::PortTxQueue;
use crate::state::{
    ClearBuffer, ConnectedPort, ConnectedPortHandle, DataBits, FlowControl, Parity, PortState,
    SerialportInfo, StopBits,
};
use crate::{log_debug, log_info};
use serialport::{
    DataBits as SerialDataBits, FlowControl as SerialFlowControl, Parity as SerialParity,
    StopBits as SerialStopBits,
};
use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;
use tauri::plugin::PluginHandle;
use tauri::{AppHandle, Runtime};

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
fn finish_serialport_info(info: SerialportInfo) -> Result<(), Error> {
    match info.state {
        PortState::Connected(cp) => {
            if let Ok(mut hub_guard) = cp.rx_hub.lock() {
                if let Some(hub) = hub_guard.take() {
                    hub.stop();
                }
            }
            Ok(())
        }
        PortState::Opening | PortState::Closed => Ok(()),
    }
}

fn ensure_rx_hub(cp: &ConnectedPortHandle, path: &str) -> Result<(), Error> {
    let mut guard = cp
        .rx_hub
        .lock()
        .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
    if guard.is_none() {
        *guard = Some(crate::port_rx_hub::PortRxHub::start(
            cp.port.clone(),
            path.to_string(),
        ));
    }
    Ok(())
}

fn ensure_rx_hub_on_physical<R: Runtime>(
    serial: &SerialPort<R>,
    physical_path: &str,
) -> Result<(), Error> {
    let cp = serial.resolve_connected_port(physical_path)?;
    ensure_rx_hub(&cp, physical_path)
}

/// Access to the serial port APIs on desktop platforms.
pub struct SerialPort<R: Runtime> {
    #[allow(dead_code)]
    pub(crate) app: AppHandle<R>,
    pub(crate) serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
    pub(crate) virtual_ports: Arc<Mutex<HashMap<String, crate::state::VirtualPortRef>>>,
}

impl<R: Runtime> Clone for SerialPort<R> {
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            serialports: Arc::clone(&self.serialports),
            virtual_ports: Arc::clone(&self.virtual_ports),
        }
    }
}

impl<R: Runtime> SerialPort<R> {
    #[allow(dead_code)]
    pub fn new(app: AppHandle<R>) -> Self {
        Self {
            app,
            serialports: Arc::new(Mutex::new(HashMap::new())),
            virtual_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

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
        crate::port_list::enumerate_available_ports(single_port_per_device)
    }

    /// Subscribe to available-port attach/detach events via a Tauri channel.
    pub fn watch_ports(
        &self,
        options: crate::events::WatchPortsOptions,
        channel: tauri::ipc::Channel<crate::events::PortListEvent>,
    ) -> Result<u32, Error> {
        let channel_id = channel.id();
        crate::port_list_monitor::subscribe(channel_id, channel, options)?;
        Ok(channel_id)
    }

    /// Stop a port-list watch session.
    pub fn unwatch_ports(&self, channel_id: u32) -> Result<(), Error> {
        crate::port_list_monitor::unsubscribe(channel_id);
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

    /// Cancel an in-flight poll [`read`] or active [`watch`].
    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        let cp = self.resolve_connected_port(&path)?;
        if let Ok(guard) = cp.rx_hub.lock() {
            if let Some(hub) = guard.as_ref() {
                hub.detach_watch();
            }
        }
        Ok(())
    }

    /// Close the specified serial port
    pub fn close(&self, path: String) -> Result<(), Error> {
        log_debug!("close {}", path);

        for channel_id in crate::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }

        if let Ok(mut virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.remove(&path) {
                vp.tx_queue.cancel_all();
                let _ = self.with_connected_port(vp.physical_path.clone(), |cp| {
                    if let Some(session) = cp.mux.lock().unwrap().clone() {
                        session.unregister_dlci(vp.dlci);
                    }
                    Ok(())
                });
                return Ok(());
            }
        }

        match self.serialports.lock() {
            Ok(mut serialports) => {
                if let Some(port_info) = serialports.remove(&path) {
                    log_debug!("stop {}", path);
                    finish_serialport_info(port_info)?;

                    log_debug!("end {}", path);

                    Ok(())
                } else {
                    Err(Error::String(format!("Serial port {} is not open!", &path)))
                }
            }
            Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
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
        for channel_id in crate::watch_registry::paths_for_port(&path) {
            let _ = self.unwatch(channel_id);
        }

        match self.serialports.lock() {
            Ok(mut map) => {
                if let Some(serial) = map.remove(&path) {
                    finish_serialport_info(serial)?;
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
    ) -> Result<(), Error> {
        if self.managed_ports()?.contains(&path) {
            let _ = self.close(path.clone());
        }

        let mut serialports = self
            .serialports
            .lock()
            .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;

        // Close existing port entry before opening a new one
        if let Some(existing) = serialports.remove(&path) {
            log_info!("Replacing existing port {}", path);
            finish_serialport_info(existing)?;
        }

        serialports.insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Opening,
            },
        );

        // Open new port (mutex held — concurrent access to this path sees Opening)
        let port_result = serialport::new(path.clone(), baud_rate)
            .data_bits(data_bits.map(Into::into).unwrap_or(SerialDataBits::Eight))
            .flow_control(
                flow_control
                    .map(Into::into)
                    .unwrap_or(SerialFlowControl::None),
            )
            .parity(parity.map(Into::into).unwrap_or(SerialParity::None))
            .stop_bits(stop_bits.map(Into::into).unwrap_or(SerialStopBits::One))
            .timeout(Duration::from_millis(
                timeout.unwrap_or(DEFAULT_PORT_TIMEOUT_MS),
            ))
            .open();

        match port_result {
            Ok(port) => {
                let entry = serialports.get_mut(&path).ok_or_else(|| {
                    Error::String(format!("Port '{}' disappeared during open", path))
                })?;
                entry.state = PortState::Connected(ConnectedPort::new(port));
                Ok(())
            }
            Err(e) => {
                serialports.remove(&path);
                Err(Error::String(format!("Failed to open serial port: {}", e)))
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
        crate::watch_registry::register(channel_id, path.clone())?;
        log_debug!("Starting watch on port: {} (channel {})", path, channel_id);

        let batch_timeout = options
            .serial_data_flush_interval_ms
            .or(options.timeout)
            .unwrap_or(DEFAULT_PORT_TIMEOUT_MS);
        let read_size = options.size.unwrap_or(1024);

        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path) {
                let session = self.with_connected_port(vp.physical_path.clone(), |cp| {
                    cp.mux
                        .lock()
                        .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
                        .clone()
                        .ok_or_else(|| Error::String("CMUX not enabled".into()))
                })?;
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
            crate::watch_registry::unregister(channel_id);
            return Err(e);
        }

        Ok(channel_id)
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        let path = crate::watch_registry::unregister(channel_id)
            .ok_or_else(|| Error::new(format!("No active watch for channel {}", channel_id)))?;
        log_debug!("Stopping watch on port: {} (channel {})", path, channel_id);
        self.stop_watch_thread(&path)
    }

    fn stop_watch_thread(&self, path: &str) -> Result<(), Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(path) {
                if let Ok(Some(session)) = self
                    .with_connected_port(vp.physical_path.clone(), |cp| {
                        Ok(cp.mux.lock().unwrap().clone())
                    })
                {
                    session.clear_watch(vp.dlci);
                }
                return Ok(());
            }
        }
        self.with_connected_port(path.to_string(), |cp| {
            if let Some(dlci) = cp.virtual_dlci {
                if let Some(session) = cp.mux.lock().unwrap().clone() {
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

    /// Read data from the serial port
    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        self.with_connected_port(path.clone(), |cp| {
            let has_watch = cp
                .rx_hub
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|h| h.shared().has_watch()))
                .unwrap_or(false);
            if has_watch {
                return Err(Error::String(
                    "Cannot read while watch is active; use watch or exchange".to_string(),
                ));
            }
            Ok(())
        })?;
        self.get_serialport(path, |port| {
            let timeout = timeout.unwrap_or(1000);

            let mut buffer = vec![0; size.unwrap_or(1024)];
            port.set_timeout(Duration::from_millis(timeout))
                .map_err(|e| Error::String(format!("Failed to set timeout: {}", e)))?;

            match port.read(&mut buffer) {
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                    Ok(data)
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Err(Error::String(format!(
                    "no data received within {} ms",
                    timeout
                ))),
                Err(e) => Err(Error::String(format!("Failed to read data: {}", e))),
            }
        })
    }

    pub fn read_binary(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<Vec<u8>, Error> {
        self.get_serialport(path.clone(), |port| {
            let target_size = size.unwrap_or(1024);
            let timeout = timeout.unwrap_or(1000);
            let mut buffer = Vec::with_capacity(target_size);
            let start = std::time::Instant::now();

            while buffer.len() < target_size && start.elapsed() < Duration::from_millis(timeout) {
                let mut temp_buf = vec![0; target_size - buffer.len()];
                match port.read(&mut temp_buf) {
                    Ok(n) if n > 0 => {
                        buffer.extend_from_slice(&temp_buf[..n]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                        if buffer.is_empty() {
                            return Err(Error::String(format!(
                                "no data received within {} ms",
                                timeout
                            )));
                        } else {
                            break;
                        }
                    }
                    Err(e) => return Err(Error::String(format!("Failed to read data: {}", e))),
                    _ => break,
                }
            }

            Ok(buffer)
        })
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
        self.get_serialport(path, |port| port.bytes_to_read().map_err(Error::from))
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
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        self.exchange_bytes(path, value.into_bytes(), options)
    }

    /// Binary variant of [`Self::exchange`].
    pub fn exchange_binary(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
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
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        if let Some((physical, dlci)) = parse_mux_path(path.as_str()) {
            let physical_path = physical.to_string();
            let tx_queue = self.get_tx_queue(path.as_str())?;
            return tx_queue.run_serial(|| {
                self.exchange_bytes_mux_direct(physical_path, dlci, path, payload, options)
            });
        }
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.exchange_bytes_direct(path, payload, options))
    }

    fn exchange_bytes_direct(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();
        let mut opts = options;
        if opts.command.is_none() {
            opts.command = Some(command.clone());
        }
        let resolved = opts.resolve();
        let cp = self.resolve_connected_port(&path)?;
        cp.exchange_cancel.store(false, Ordering::SeqCst);
        let cancel = cp.exchange_cancel.clone();

        let result = (|| {
            ensure_rx_hub(&cp, &path)?;

            let hub_shared = {
                let hub_guard = cp
                    .rx_hub
                    .lock()
                    .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
                hub_guard
                    .as_ref()
                    .ok_or_else(|| Error::String("RX hub missing".into()))?
                    .shared()
            };

            use crate::events::RxPrepareMode;
            match resolved.rx_prepare {
                RxPrepareMode::Purge => {
                    let _yield_guard = crate::port_rx_hub::PortIoYieldGuard::new(hub_shared.clone());
                    with_port_try_lock(
                        &cp.port,
                        Duration::from_millis(resolved.timeout_ms.min(5000)),
                        |port| port.clear(ClearBuffer::Input.into()).map_err(Error::from),
                    )?;
                }
                RxPrepareMode::Drain => {
                    hub_shared
                        .drain(
                            resolved.drain_idle_ms,
                            resolved.drain_max_ms,
                            cancel.clone(),
                            resolved.solicited_prefixes.clone(),
                        )
                        .map_err(Error::String)?;
                }
                RxPrepareMode::None => {}
            }

            {
                let _yield_guard = crate::port_rx_hub::PortIoYieldGuard::new(hub_shared);
                with_port_try_lock(
                    &cp.port,
                    Duration::from_millis(resolved.timeout_ms.min(5000)),
                    |port| write_all_port(port, &payload, "exchange write"),
                )?;
            }

            let waiter = crate::port_rx_hub::ExchangeWaiter::new(resolved.clone(), cancel.clone());
            {
                let hub_guard = cp
                    .rx_hub
                    .lock()
                    .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
                hub_guard
                    .as_ref()
                    .ok_or_else(|| Error::String("RX hub missing".into()))?
                    .set_exchange_waiter(waiter.clone());
            }
            let wait_result = waiter.wait(resolved.timeout_ms);
            {
                let hub_guard = cp
                    .rx_hub
                    .lock()
                    .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
                if let Some(h) = hub_guard.as_ref() {
                    h.clear_exchange_waiter();
                }
            }
            let (raw, matched) = wait_result.map_err(Error::String)?;
            Ok(crate::exchange_read::ReadUntilOutcome { raw, matched })
        })();

        result.map(|outcome| {
            crate::at_parse::ExchangeResponse::from_raw(
                outcome.raw,
                outcome.matched,
                &command,
                &user_solicited,
                resolved.result_format,
            )
        })
    }

    fn exchange_bytes_mux_direct(
        &self,
        physical_path: String,
        _dlci: u8,
        virtual_path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
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
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
        let dlci = vp.dlci;
        let command = options
            .command
            .clone()
            .unwrap_or_else(|| String::from_utf8_lossy(&payload).into_owned());
        let user_solicited = options.solicited_prefixes.clone().unwrap_or_default();
        let mut opts = options;
        if opts.command.is_none() {
            opts.command = Some(command.clone());
        }
        let resolved = opts.resolve();

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

        vp.exchange_cancel.store(false, Ordering::SeqCst);
        let cancel = vp.exchange_cancel.clone();
        ensure_rx_hub_on_physical(self, &physical_path)?;

        let waiter = crate::port_rx_hub::ExchangeWaiter::new(resolved.clone(), cancel.clone());
        session.set_exchange_waiter(dlci, waiter.clone());
        session.send_uih(dlci, &payload).map_err(Error::String)?;

        let wait_result = waiter.wait(resolved.timeout_ms);
        session.clear_exchange_waiter(dlci);
        let (raw, matched) = wait_result.map_err(Error::String)?;
        Ok(crate::at_parse::ExchangeResponse::from_raw(
            raw,
            matched,
            &command,
            &user_solicited,
            resolved.result_format,
        ))
    }

    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at_parse::ExchangeResponse, Error> {
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

    /// Send one AT command through the native transaction queue with session defaults.
    pub fn at(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at_parse::AtCommandResult, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| {
            let session = tx_queue.at_session();
            let append_cr = options
                .as_ref()
                .and_then(|o| o.append_cr)
                .unwrap_or_else(|| session.append_cr());
            let payload = normalize_at_command(&command, append_cr);
            let exchange_opts = session.merge_exchange(&command, options.as_ref());
            let response = self.run_exchange_unqueued(path, payload.into_bytes(), exchange_opts)?;
            check_expect_ok(
                &session,
                response.status,
                &String::from_utf8_lossy(&response.raw),
            )?;
            Ok(crate::at_parse::AtCommandResult::from_exchange(
                command, response,
            ))
        })
    }

    /// Multi-phase AT flow (e.g. CMGS) — phases run atomically without interleaving other jobs.
    pub fn at_phases(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        tx_queue.run_serial(|| self.at_phases_direct(path, phases))
    }

    fn at_phases_direct(
        &self,
        path: String,
        phases: Vec<AtPhase>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        let session = tx_queue.at_session();
        let mut results = Vec::with_capacity(phases.len());
        for (i, phase) in phases.iter().enumerate() {
            let label = phase.command.clone().unwrap_or_else(|| match &phase.write {
                AtPhaseWrite::Text(s) => s.clone(),
                AtPhaseWrite::Binary(b) => format!("<binary {} bytes>", b.len()),
            });
            let rx_prepare = if i == 0 {
                None
            } else {
                Some(crate::events::RxPrepareMode::None)
            };
            let mut exchange_opts = session.merge_phase(phase, &label);
            if let Some(rp) = rx_prepare {
                exchange_opts.rx_prepare = Some(rp);
            }
            let payload = match &phase.write {
                AtPhaseWrite::Text(s) => {
                    let append_cr = session.append_cr();
                    normalize_at_command(s, append_cr).into_bytes()
                }
                AtPhaseWrite::Binary(b) => b.clone(),
            };
            let response = self.run_exchange_unqueued(path.clone(), payload, exchange_opts)?;
            check_expect_ok(
                &session,
                response.status,
                &String::from_utf8_lossy(&response.raw),
            )?;
            results.push(crate::at_parse::AtCommandResult::from_exchange(
                label, response,
            ));
        }
        Ok(results)
    }

    /// Built-in CMGS recipe: `AT+CMGS=n` → `>` → PDU + Ctrl+Z → final line.
    pub fn send_sms_pdu(
        &self,
        path: String,
        length: u32,
        pdu: Vec<u8>,
        options: Option<SendSmsPduOptions>,
    ) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
        let tx_queue = self.get_tx_queue(&path)?;
        let session = tx_queue.at_session();
        let timeout_ms = options
            .as_ref()
            .and_then(|o| o.timeout_ms)
            .or(session.default_timeout_ms);
        let result_format = options
            .as_ref()
            .and_then(|o| o.result_format)
            .or(session.result_format);
        let cmd = format!("AT+CMGS={length}");
        let mut payload = pdu;
        payload.push(0x1a);
        let phases = vec![
            AtPhase {
                write: AtPhaseWrite::Text(cmd.clone()),
                completion_mode: Some(crate::events::ExchangeCompletionMode::AtIntermediate),
                result_format,
                timeout_ms,
                command: Some(cmd),
                rx_prepare: None,
            },
            AtPhase {
                write: AtPhaseWrite::Binary(payload),
                completion_mode: Some(crate::events::ExchangeCompletionMode::AtFinalLine),
                result_format,
                timeout_ms,
                command: Some(String::new()),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
            },
        ];
        tx_queue.run_serial(|| self.at_phases_direct(path, phases))
    }

    /// Configure AT session defaults for native `at` jobs on this port.
    pub fn configure_at_session(
        &self,
        path: String,
        session: AtSessionConfig,
    ) -> Result<(), Error> {
        if let Ok(mut virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get_mut(&path) {
                vp.tx_queue.configure_at_session(session);
                return Ok(());
            }
        }
        let cp = self.resolve_connected_port(&path)?;
        cp.tx_queue.configure_at_session(session);
        Ok(())
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

        self.with_connected_port(path.clone(), |cp| {
            if cp.virtual_dlci.is_some() {
                return Err(Error::String("enable_mux on virtual port".into()));
            }
            if cp.mux.lock().unwrap().is_some() {
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
            ensure_rx_hub(&cp.handle(), &path)?;
            if let Some(hub) = cp.rx_hub.lock().unwrap().as_ref() {
                hub.attach_cmux(session.clone());
            }
            *cp.mux.lock().unwrap() = Some(session);
            Ok(())
        })
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
            if let Some(hub) = cp.rx_hub.lock().unwrap().as_ref() {
                hub.detach_cmux();
            }
            *cp.mux.lock().unwrap() = None;
            Ok(())
        })
    }

    /// Cancel an in-flight exchange and reject queued waiters on `path`.
    pub fn cancel_exchange(&self, path: String) -> Result<(), Error> {
        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path) {
                vp.exchange_cancel.store(true, Ordering::SeqCst);
                vp.tx_queue.cancel_all();
                return Ok(());
            }
        }
        let cp = self.resolve_connected_port(&path)?;
        cp.exchange_cancel.store(true, Ordering::SeqCst);
        cp.tx_queue.cancel_all();
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
        let _yield_guard = cp.rx_hub.lock().ok().and_then(|guard| {
            guard
                .as_ref()
                .map(|hub| crate::port_rx_hub::PortIoYieldGuard::new(hub.shared()))
        });
        with_port_try_lock(
            &cp.port,
            Duration::from_millis(PORT_LOCK_TIMEOUT_MS),
            f,
        )
    }
}

#[cfg(test)]
mod disconnect_tests {
    fn is_benign_read_error(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        )
    }

    fn is_disconnect_read_error(err: &std::io::Error) -> bool {
        matches!(
            err.kind(),
            std::io::ErrorKind::BrokenPipe
                | std::io::ErrorKind::NotConnected
                | std::io::ErrorKind::ConnectionAborted
                | std::io::ErrorKind::ConnectionReset
                | std::io::ErrorKind::UnexpectedEof
        )
    }

    #[test]
    fn read_timeout_is_benign_not_disconnect() {
        let err = std::io::Error::new(std::io::ErrorKind::TimedOut, "timeout");
        assert!(is_benign_read_error(&err));
        assert!(!is_disconnect_read_error(&err));
    }

    #[test]
    fn broken_pipe_is_disconnect() {
        let err = std::io::Error::new(std::io::ErrorKind::BrokenPipe, "hang-up");
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
