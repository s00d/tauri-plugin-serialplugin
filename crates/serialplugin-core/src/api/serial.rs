//! Unified serial API — `Box<dyn serialport::SerialPort>` on desktop and Android.
#[cfg(target_os = "ios")]
use crate::serialport;

use crate::api::serial_port::{
    configure_at_session as configure_at_session_shared, run_exchange_queued,
};
use crate::at::session::{AtCommandOptions, AtPhase, AtSessionConfig, SendSmsPduOptions};
use crate::cmux::{mux_path, parse_mux_path, CmuxSession};
use crate::error::{CoreError, Error, ErrorKind};
use crate::events::{
    CoreEvent, CoreSink, EventSink, ExchangeOptions, PortListSink, PortStatus, SerialEvent,
    SubscriptionId, WatchOptions,
};
use crate::hub::delivery::CoreQueue;
#[cfg(target_os = "android")]
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
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

type PortArc = Arc<Mutex<Box<dyn SerialPortTrait>>>;

/// Ids handed out by [`SerialManager::watch`] / [`SerialManager::watch_ports`].
/// The Tauri plugin passes its IPC channel ids through the `*_with_id` variants instead;
/// both share the manager's registry, so an id already taken there is skipped (and
/// registering a duplicate id is rejected outright).
fn next_watch_id(taken: impl Fn(u32) -> bool) -> u32 {
    static NEXT: AtomicU32 = AtomicU32::new(1);
    loop {
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if !taken(id) {
            return id;
        }
    }
}

/// Ids of `watch_core` streams and `subscribe_core` subscriptions.
fn next_subscription_id() -> u64 {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// Platform seam: enumerate available ports.
fn platform_available_ports(
    single_port_per_device: bool,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    #[cfg(not(target_os = "android"))]
    {
        crate::port::list::enumerate_available_ports(single_port_per_device)
    }
    #[cfg(target_os = "android")]
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
    lines: crate::state::ModemLinesOnOpen,
) -> Result<(String, Box<dyn SerialPortTrait>), CoreError> {
    if path.starts_with("ble://") {
        #[cfg(feature = "ble")]
        {
            #[cfg(not(target_os = "android"))]
            use crate::port::ble::BleSerialPort as Ble;
            #[cfg(target_os = "android")]
            use crate::port::ble_android::AndroidBleSerialPort as Ble;
            let port = Ble::connect(
                path,
                baud_rate,
                data_bits.into(),
                flow_control.into(),
                parity.into(),
                stop_bits.into(),
                timeout,
            )
            .map_err(|e| CoreError::io(format!("Failed to open {path}"), e))?;
            return Ok((path.to_string(), Box::new(port)));
        }
        #[cfg(not(feature = "ble"))]
        return Err(CoreError::new(
            ErrorKind::Other,
            Error::String(format!(
                "{path}: BLE needs the plugin's `ble` cargo feature"
            )),
        ));
    }
    if crate::port::tcp::tcp_address(path).is_some() {
        let port = crate::port::tcp::TcpSerialPort::connect(
            path,
            baud_rate,
            data_bits.into(),
            flow_control.into(),
            parity.into(),
            stop_bits.into(),
            timeout,
        )
        .map_err(|e| CoreError::io(format!("Failed to open {path}"), e))?;
        return Ok((path.to_string(), Box::new(port)));
    }
    // iOS has no local serial devices; serialport's open/enumerate code would also
    // pull in IOKit, which iOS apps cannot link.
    #[cfg(target_os = "ios")]
    {
        let _ = (
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
            lines,
        );
        Err(CoreError::new(
            ErrorKind::NotFound,
            Error::String(format!(
                "{path}: iOS has no local serial ports; use a tcp://host:port bridge"
            )),
        ))
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let mut builder = serialport::new(path, baud_rate)
            .data_bits(data_bits.into())
            .flow_control(flow_control.into())
            .parity(parity.into())
            .stop_bits(stop_bits.into())
            .timeout(timeout);
        if let Some(dtr) = lines.dtr {
            builder = builder.dtr_on_open(dtr);
        }
        let mut port = builder.open().map_err(|e| {
            let err = CoreError::serialport("Failed to open serial port".into(), e);
            // Listed but not openable: in use (Windows says "Access is denied", in the
            // system language; serialport reports that and flock failures as NoDevice).
            let listed = || {
                serialport::available_ports().is_ok_and(|l| l.iter().any(|p| p.port_name == path))
            };
            if err.kind() == ErrorKind::NotFound && listed() {
                err.with_kind(ErrorKind::Busy)
            } else {
                err
            }
        })?;
        if let Some(rts) = lines.rts {
            port.write_request_to_send(rts)
                .map_err(|e| CoreError::serialport("Failed to set RTS on open".into(), e))?;
        }
        Ok((path.to_string(), port))
    }
    #[cfg(target_os = "android")]
    {
        let _ = timeout;
        crate::android::driver_host::global_host()
            .open(
                path,
                baud_rate,
                data_bits,
                flow_control,
                parity,
                stop_bits,
                lines,
            )
            .map_err(CoreError::from)
    }
}

/// A second handle for writes and control calls, so they never wait for the RX hub's
/// read (and the hub's poll timeout never touches the write timeout). `timeout` is
/// the port timeout from `open` / `set_timeout`.
///
/// Local serial ports on Unix only. TCP, BLE and Android keep one handle (no real
/// clone there), and so does Windows: its duplicated handles share one COMMTIMEOUTS,
/// so the hub's short read timeout would cut writes short.
fn split_writer(
    path: &str,
    port: &dyn SerialPortTrait,
    timeout: Duration,
) -> Option<Box<dyn SerialPortTrait>> {
    if !cfg!(all(
        unix,
        not(any(target_os = "android", target_os = "ios"))
    )) || path.starts_with("ble://")
        || crate::port::tcp::tcp_address(path).is_some()
    {
        return None;
    }
    let mut writer = port.try_clone().ok()?;
    writer.set_timeout(timeout).ok()?;
    Some(writer)
}

/// Default read/write timeout (ms) when opening a port.
const DEFAULT_PORT_TIMEOUT_MS: u64 = 1000;
const PORT_LOCK_TIMEOUT_MS: u64 = 250;
const PORT_IO_TIMEOUT_MS: u64 = 100;
/// Pause between the `AT+CMUX` OK and the first frame (see `enable_mux`). The M26
/// needs well under 20 ms; 100 ms leaves room for slower modems.
const CMUX_SWITCH_SETTLE: Duration = Duration::from_millis(100);
/// Watch batch window when the caller sets none: short enough for interactive
/// terminals, long enough to coalesce a burst into one event.
const DEFAULT_WATCH_FLUSH_MS: u64 = 20;

/// Acquire the port mutex with a deadline (hub reader uses try_lock; avoid blocking forever).
/// `io_timeout`: set the shared 100 ms I/O timeout first (single-handle ports only).
fn lock_with_deadline<T, E, F>(
    port: &PortArc,
    lock_timeout: Duration,
    io_timeout: bool,
    f: F,
) -> Result<T, E>
where
    E: From<Error>,
    F: FnOnce(&mut Box<dyn serialport::SerialPort>) -> Result<T, E>,
{
    let deadline = Instant::now() + lock_timeout;
    loop {
        match port.try_lock() {
            Ok(mut guard) => {
                if io_timeout {
                    let _ = guard.set_timeout(Duration::from_millis(PORT_IO_TIMEOUT_MS));
                }
                return f(&mut guard);
            }
            Err(_) if Instant::now() >= deadline => {
                return Err(Error::String(format!(
                    "serial port lock timeout after {} ms",
                    lock_timeout.as_millis()
                ))
                .into());
            }
            Err(_) => thread::sleep(Duration::from_millis(1)),
        }
    }
}

fn with_port_try_lock<T, E, F>(port: &PortArc, lock_timeout: Duration, f: F) -> Result<T, E>
where
    E: From<Error>,
    F: FnOnce(&mut Box<dyn serialport::SerialPort>) -> Result<T, E>,
{
    lock_with_deadline(port, lock_timeout, true, f)
}

/// Time `len` bytes take on the wire at the port's baud rate (11 bits a character).
fn wire_ms(port: &dyn SerialPortTrait, len: usize) -> u64 {
    let baud = u64::from(port.baud_rate().unwrap_or(9600).max(1));
    (len as u64).saturating_mul(11_000) / baud
}

/// Write the full buffer, retrying until all bytes are sent or an error occurs
/// (single-handle ports: the hub shares the handle and its timeout).
fn write_all_port(
    port: &mut Box<dyn serialport::SerialPort>,
    buf: &[u8],
    operation: &str,
) -> Result<usize, CoreError> {
    // The shared 100 ms I/O timeout is too short for big or slow writes (8 KB at
    // 9600 baud needs ~9 s), so give writes room for the wire time plus a margin.
    let wire_ms = wire_ms(port.as_ref(), buf.len());
    let _ = port.set_timeout(Duration::from_millis(wire_ms.saturating_mul(2) + 1000));
    let result = port
        .write_all(buf)
        .map_err(|e| CoreError::io(format!("Failed to {operation}"), e));
    let _ = port.set_timeout(Duration::from_millis(PORT_IO_TIMEOUT_MS));
    result.map(|()| buf.len())
}

/// Write on a split write handle: its own timeout, no lock shared with the hub. A
/// write that needs longer on the wire than the port timeout gets twice its wire
/// time (as 4.x writes did), so a big write is not cut short by a short timeout.
fn write_all_split(port: &PortArc, buf: &[u8], operation: &str) -> Result<usize, CoreError> {
    lock_with_deadline(
        port,
        Duration::from_millis(PORT_LOCK_TIMEOUT_MS),
        false,
        |p| {
            let own = p.timeout();
            let need = Duration::from_millis(wire_ms(p.as_ref(), buf.len()).saturating_mul(2));
            let raise = need > own;
            if raise {
                let _ = p.set_timeout(need);
            }
            let result = p
                .write_all(buf)
                .map(|()| buf.len())
                .map_err(|e| CoreError::io(format!("Failed to {operation}"), e));
            if raise {
                let _ = p.set_timeout(own);
            }
            result
        },
    )
}

/// Tear down resources held by a [`SerialportInfo`] (RX hub + port).
fn finish_serialport_info(path: &str, info: SerialportInfo) -> Result<(), Error> {
    #[cfg(not(target_os = "android"))]
    let _ = path;
    match info.state {
        PortState::Connected(cp) => {
            if let Ok(mut hub_guard) = cp.rx_hub.lock() {
                if let Some(hub) = hub_guard.take() {
                    #[cfg(target_os = "android")]
                    crate::android::registry::global_registry().unregister(path);
                    // The hub flushes what it holds into the watches; no `Disconnect`.
                    hub.shared().mark_closing();
                    match Arc::try_unwrap(hub) {
                        Ok(hub) => hub.stop(),
                        Err(hub) => hub.request_stop(),
                    }
                }
            }
            #[cfg(target_os = "android")]
            {
                let _ = crate::android::driver_host::global_host().close(Some(path));
            }
            Ok(())
        }
        PortState::Opening | PortState::Closed => Ok(()),
    }
}

fn register_mobile_hub(path: &str, cp: &ConnectedPortHandle, hub: Arc<crate::hub::PortRxHub>) {
    #[cfg(target_os = "android")]
    {
        let hub_handle: Arc<dyn crate::hub::RxHubHandle> = hub;
        crate::android::registry::global_registry().register(path, hub_handle, cp.clone());
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = (path, cp, hub);
    }
}

/// Cloned write handle of a port, tied to the reader it was cloned from.
struct Writer {
    reader: Weak<Mutex<Box<dyn SerialPortTrait>>>,
    port: PortArc,
}

/// Per-manager state of the 4.2 core (registries, write handles, subscriptions).
#[derive(Default)]
struct CoreState {
    /// Old `watch` ids → path (one per port).
    watches: Mutex<HashMap<u32, String>>,
    /// `watch_core` ids → path (any number per port).
    core_watches: Mutex<HashMap<u64, String>>,
    writers: Mutex<HashMap<String, Writer>>,
    /// Ports the core released after a disconnect, with the reason (4.x behaviour until
    /// `close`: still listed, a late watch gets `Disconnect`, calls fail with it).
    disconnected: Mutex<HashMap<String, String>>,
    subscribers: Mutex<Vec<(u64, Arc<CoreQueue>)>>,
    /// Bytes per core receiver queue; 0 = [`crate::events::DEFAULT_CORE_QUEUE_BYTES`].
    queue_capacity: AtomicUsize,
}

/// A manager without keeping it alive (the hub threads' disconnect hooks).
struct WeakManager {
    serialports: Weak<Mutex<HashMap<String, SerialportInfo>>>,
    virtual_ports: Weak<Mutex<HashMap<String, crate::state::VirtualPortRef>>>,
    core: Weak<CoreState>,
}

impl WeakManager {
    fn upgrade(&self) -> Option<SerialManager> {
        Some(SerialManager {
            serialports: self.serialports.upgrade()?,
            virtual_ports: self.virtual_ports.upgrade()?,
            core: self.core.upgrade()?,
        })
    }
}

/// Owns every open port (desktop serial, `tcp://`, `ble://`, Android USB, CMUX channels).
///
/// Cheap to clone (all clones share the same ports). Methods block: call them from a
/// plain thread or `tokio::task::spawn_blocking`, not straight from an async task.
///
/// Lock order: `serialports` before `virtual_ports`; never hold a map lock across
/// device I/O, thread spawn, or `JoinHandle` join.
#[derive(Clone, Default)]
pub struct SerialManager {
    #[doc(hidden)]
    pub serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
    #[doc(hidden)]
    pub virtual_ports: Arc<Mutex<HashMap<String, crate::state::VirtualPortRef>>>,
    core: Arc<CoreState>,
}

impl SerialManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Android only: hook USB detach / port-list callbacks from the Kotlin side into this manager.
    #[cfg(target_os = "android")]
    pub fn setup_teardown(&self) {
        let manager = self.downgrade();
        // USB failure: released like a desktop disconnect (state event, the path stays
        // listed until `close`, watches stay registered as on desktop and in 4.x).
        crate::android::registry::set_rust_state_fail(Box::new(move |path, reason| {
            if let Some(m) = manager.upgrade() {
                let removed = crate::sync_util::lock_or_recover(&m.serialports).remove(path);
                if let Some(info) = removed {
                    m.release(path, info, reason);
                }
            }
        }));
        crate::port::list_monitor::set_android_enumerator(move |single| {
            platform_available_ports(single)
        });
        crate::android::registry::init_registry();
    }

    /// Get serial port list
    pub fn available_ports(
        &self,
        single_port_per_device: bool,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        platform_available_ports(single_port_per_device)
    }

    /// Subscribe to available-port attach/detach events. `sink` first gets a
    /// [`Snapshot`](crate::events::PortListEvent::Snapshot), then `added` / `removed`.
    /// Returns the id for [`Self::unwatch_ports`].
    pub fn watch_ports(
        &self,
        options: crate::events::WatchPortsOptions,
        sink: PortListSink,
    ) -> Result<u32, Error> {
        self.watch_ports_with_id(
            next_watch_id(crate::port::list_monitor::is_subscribed),
            options,
            sink,
        )
    }

    /// [`Self::watch_ports`] under a caller-chosen id (the plugin uses its IPC channel id).
    pub fn watch_ports_with_id(
        &self,
        id: u32,
        options: crate::events::WatchPortsOptions,
        sink: PortListSink,
    ) -> Result<u32, Error> {
        crate::port::list_monitor::subscribe_sink(id, sink, options)?;
        Ok(id)
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
        drop(ports);
        if let Ok(virtuals) = self.virtual_ports.lock() {
            port_list.extend(virtuals.keys().cloned());
        }
        port_list.extend(
            crate::sync_util::lock_or_recover(&self.core.disconnected)
                .keys()
                .cloned(),
        );
        port_list.sort();
        port_list.dedup();
        Ok(port_list)
    }

    /// Cancel an in-flight poll [`Self::read`] (does not stop an active [`Self::watch`]).
    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        let cp = self.resolve_connected_port(&path)?;
        if let Ok(guard) = cp.rx_hub.lock() {
            if let Some(hub) = guard.as_ref() {
                hub.shared().cancel_pending_read();
            }
        }
        Ok(())
    }

    /// Close the specified serial port. What the hub still holds (the watch batch, a
    /// parser's remainder) reaches the watches first, in order; no `Disconnect` is
    /// sent. Succeeds once on a port the core already released after a disconnect.
    pub fn close(&self, path: String) -> Result<(), Error> {
        self.close_core(path).map_err(Error::from)
    }

    /// [`Self::close`] with a [`CoreError`].
    pub fn close_core(&self, path: String) -> Result<(), CoreError> {
        log_debug!("close {}", path);

        if crate::sync_util::lock_or_recover(&self.virtual_ports).contains_key(&path) {
            for channel_id in self.watch_ids_for(&path) {
                let _ = self.unwatch(channel_id);
            }
            for id in self.core_watch_ids_for(&path) {
                let _ = self.unwatch_core(SubscriptionId(id));
            }
            let removed = crate::sync_util::lock_or_recover(&self.virtual_ports).remove(&path);
            if let Some(vp) = removed {
                vp.tx_queue.cancel_all();
                if let Ok(cp) = self.resolve_connected_port(&vp.physical_path) {
                    if let Some(session) = crate::sync_util::lock_or_recover(&cp.mux).clone() {
                        session.unregister_dlci(vp.dlci);
                    }
                }
                self.emit_port_state(&path, PortStatus::Closed);
                return Ok(());
            }
        }

        // Physical port: its virtual channels and CMUX session go with it.
        if self.resolve_connected_port(&path).is_ok() {
            let _ = self.disable_mux(path.clone());
        }

        let removed = match self.serialports.lock() {
            Ok(mut serialports) => serialports.remove(&path),
            Err(error) => {
                return Err(Error::String(format!("Failed to acquire lock: {}", error)).into());
            }
        };

        if let Some(port_info) = removed {
            log_debug!("stop {}", path);
            self.drop_writer(&path);
            let finished = finish_serialport_info(&path, port_info);
            self.forget_watches(&path);
            finished?;
            self.emit_port_state(&path, PortStatus::Closed);
            log_debug!("end {}", path);
            Ok(())
        } else {
            self.forget_watches(&path);
            if crate::sync_util::lock_or_recover(&self.core.disconnected)
                .remove(&path)
                .is_some()
            {
                return Ok(());
            }
            Err(CoreError::new(
                ErrorKind::NotFound,
                Error::String(format!("Serial port {} is not open!", path)),
            ))
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
        if crate::sync_util::lock_or_recover(&self.virtual_ports).contains_key(&path) {
            return self.close(path);
        }
        self.close_virtual_channels_of(&path);
        if let Ok(cp) = self.resolve_connected_port(&path) {
            if let Some(session) = crate::sync_util::lock_or_recover(&cp.mux).take() {
                session.close_down();
            }
        }

        // Remove under the lock, join the hub outside it (lock order, see the struct docs).
        let removed = match self.serialports.lock() {
            Ok(mut map) => map.remove(&path),
            Err(error) => return Err(Error::String(format!("Failed to acquire lock: {}", error))),
        };
        self.drop_writer(&path);
        crate::sync_util::lock_or_recover(&self.core.disconnected).remove(&path);
        if let Some(serial) = removed {
            let finished = finish_serialport_info(&path, serial);
            self.forget_watches(&path);
            finished?;
            self.emit_port_state(&path, PortStatus::Closed);
        } else {
            self.forget_watches(&path);
        }
        Ok(())
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
        self.open_with_lines(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
            crate::state::ModemLinesOnOpen::default(),
        )
    }

    /// [`Self::open`] with explicit DTR / RTS levels on open.
    #[allow(clippy::too_many_arguments)]
    pub fn open_with_lines(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
        lines: crate::state::ModemLinesOnOpen,
    ) -> Result<String, Error> {
        self.open_core(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
            lines,
        )
        .map_err(Error::from)
    }

    /// [`Self::open_with_lines`] with a [`CoreError`] (kind `NotFound`, `PermissionDenied`,
    /// `Busy`, … and the platform error as its source). `timeout` (ms, default 1000)
    /// is the port's read / write timeout; [`Self::set_timeout`] changes it later.
    #[allow(clippy::too_many_arguments)]
    pub fn open_core(
        &self,
        path: String,
        baud_rate: u32,
        data_bits: Option<DataBits>,
        flow_control: Option<FlowControl>,
        parity: Option<Parity>,
        stop_bits: Option<StopBits>,
        timeout: Option<u64>,
        lines: crate::state::ModemLinesOnOpen,
    ) -> Result<String, CoreError> {
        // Re-open (e.g. after a webview reload): tear the old session down the same
        // way `close` does, so its watches, CMUX channels and hub go with it.
        let already_open = matches!(
            crate::sync_util::lock_or_recover(&self.serialports)
                .get(&path)
                .map(|i| &i.state),
            Some(PortState::Connected(_))
        );
        let released =
            crate::sync_util::lock_or_recover(&self.core.disconnected).contains_key(&path);
        if already_open || released {
            log_info!("Replacing existing port {}", path);
            let _ = self.close(path.clone());
        }
        let stale = {
            let mut serialports = self
                .serialports
                .lock()
                .map_err(|e| Error::String(format!("Failed to acquire lock: {}", e)))?;
            if matches!(
                serialports.get(&path).map(|i| &i.state),
                Some(PortState::Opening)
            ) {
                return Err(Error::String(format!("open already in progress for {}", path)).into());
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

        let timeout = Duration::from_millis(timeout.unwrap_or(DEFAULT_PORT_TIMEOUT_MS));
        let port_result = platform_open(
            &path,
            baud_rate,
            data_bits.unwrap_or(DataBits::Eight),
            flow_control.unwrap_or(FlowControl::None),
            parity.unwrap_or(Parity::None),
            stop_bits.unwrap_or(StopBits::One),
            timeout,
            lines,
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

                let (cp, hub) = self.connect(&session_path, port, timeout);
                let reserved = matches!(
                    serialports.get(&session_path).map(|i| &i.state),
                    Some(PortState::Opening)
                );
                if let (Ok(()), true) = (&hub, reserved) {
                    if let Some(entry) = serialports.get_mut(&session_path) {
                        entry.state = PortState::Connected(cp);
                    }
                    // Under the map lock: a disconnect hook (it waits for the lock)
                    // reports `Disconnected` after this `Opened`.
                    self.emit_port_state(&session_path, PortStatus::Opened);
                    drop(serialports);
                    return Ok(session_path);
                }
                // Never leave an `Opening` entry or a running hub thread behind.
                if reserved {
                    serialports.remove(&session_path);
                }
                drop(serialports);
                self.drop_writer(&session_path);
                let _ = finish_serialport_info(
                    &session_path,
                    SerialportInfo {
                        state: PortState::Connected(cp),
                    },
                );
                hub?;
                Err(
                    Error::String(format!("Port '{}' state changed during open", session_path))
                        .into(),
                )
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

    /// Manage `port` under `path` as if [`Self::open`] had opened it (tests, custom
    /// transports, a fake device). An open port under `path` is closed first. Its
    /// current `timeout()` is the port timeout.
    ///
    /// On Unix (not TCP / BLE) writes and control calls go to `port.try_clone()` when
    /// it succeeds, reads to `port`: a clone must share the device (like a dup'ed fd),
    /// or `try_clone` must return `Err` to keep one handle for everything.
    pub fn insert_port(
        &self,
        path: String,
        port: Box<dyn SerialPortTrait>,
    ) -> Result<(), CoreError> {
        let known = crate::sync_util::lock_or_recover(&self.serialports).contains_key(&path)
            || crate::sync_util::lock_or_recover(&self.core.disconnected).contains_key(&path);
        if known {
            let _ = self.close(path.clone());
        }
        let timeout = port.timeout();
        // Under the map lock (as `open_core`): a device failing at once is reported by
        // its disconnect hook, which waits for the record.
        let mut serialports = crate::sync_util::lock_or_recover(&self.serialports);
        let (cp, hub) = self.connect(&path, port, timeout);
        if let Err(e) = hub {
            drop(serialports);
            self.drop_writer(&path);
            let _ = finish_serialport_info(
                &path,
                SerialportInfo {
                    state: PortState::Connected(cp),
                },
            );
            return Err(e.into());
        }
        serialports.insert(
            path.clone(),
            SerialportInfo {
                state: PortState::Connected(cp),
            },
        );
        self.emit_port_state(&path, PortStatus::Opened);
        drop(serialports);
        Ok(())
    }

    /// Stream port events into `sink` (one watch per port and manager). Returns the id
    /// for [`Self::unwatch`]. As in 4.x, `sink` runs on the RX side: the port's hub
    /// thread, or the caller of `unwatch` / `close` for the tail they deliver. It never
    /// runs under a manager lock, but a slow sink holds up RX (keep it short, e.g.
    /// forward into a channel), and calling `at()` / `exchange` on the same port from
    /// it waits for the RX it blocks. Nothing reaches it after `close()` returns.
    /// [`Self::watch_core`] has its own delivery thread instead.
    pub fn watch(
        &self,
        path: String,
        options: WatchOptions,
        sink: EventSink,
    ) -> Result<u32, Error> {
        self.watch_framed(path, options, None, sink)
    }

    /// [`Self::watch`] under a caller-chosen id (the plugin uses its IPC channel id).
    pub fn watch_with_id(
        &self,
        channel_id: u32,
        path: String,
        options: WatchOptions,
        sink: EventSink,
    ) -> Result<u32, Error> {
        self.watch_framed_with_id(channel_id, path, options, None, sink)
    }

    /// [`Self::watch`] with a native parser: each complete frame becomes exactly one
    /// [`SerialEvent::Data`](crate::events::SerialEvent::Data) (the batch timer no longer
    /// applies). An invalid config fails before the watch starts. With
    /// [`WatchOptions::route_urc`] the URC split runs first and only non-URC lines reach
    /// the parser. On unwatch, close or disconnect the parser's buffered remainder is
    /// sent as a final `data` event (before `disconnect`), like Node serialport emits
    /// on stream end.
    pub fn watch_framed(
        &self,
        path: String,
        options: WatchOptions,
        parser: Option<serial_codec::framing::FramerConfig>,
        sink: EventSink,
    ) -> Result<u32, Error> {
        let id = next_watch_id(|id| {
            crate::sync_util::lock_or_recover(&self.core.watches).contains_key(&id)
                || crate::port::watch_registry::contains(id)
        });
        self.watch_framed_with_id(id, path, options, parser, sink)
    }

    /// [`Self::watch_framed`] under a caller-chosen id.
    pub fn watch_framed_with_id(
        &self,
        channel_id: u32,
        path: String,
        options: WatchOptions,
        parser: Option<serial_codec::framing::FramerConfig>,
        channel: EventSink,
    ) -> Result<u32, Error> {
        // Validate the parser before anything is registered or attached.
        let mut framer = parser
            .as_ref()
            .map(|p| p.build())
            .transpose()
            .map_err(|e| Error::String(e.to_string()))?;
        self.register_watch(channel_id, &path)?;
        log_debug!("Starting watch on port: {} (channel {})", path, channel_id);

        // Released after a disconnect: the late watch gets that `Disconnect` (4.x).
        if let Some(reason) = self.released_reason(&path) {
            crate::port::watch_registry::send_event_to(
                &channel,
                SerialEvent::Disconnect { path, reason },
            );
            return Ok(channel_id);
        }

        let (batch_timeout, read_size, route_urc) = watch_params(&options);

        if let Ok(virtuals) = self.virtual_ports.lock() {
            if let Some(vp) = virtuals.get(&path).cloned() {
                drop(virtuals);
                let session = self
                    .resolve_connected_port(&vp.physical_path)
                    .and_then(|cp| {
                        crate::sync_util::lock_or_recover(&cp.mux)
                            .clone()
                            .ok_or_else(|| Error::String("CMUX not enabled".into()))
                    });
                let session = match session {
                    Ok(session) => session,
                    Err(e) => {
                        self.unregister_watch(channel_id);
                        return Err(e);
                    }
                };
                if let Err(e) = self.ensure_rx_hub_on_physical(&vp.physical_path) {
                    self.unregister_watch(channel_id);
                    return Err(e);
                }
                session.set_watch_with(vp.dlci, channel, batch_timeout, framer);
                return Ok(channel_id);
            }
        }

        // A replaced watch's remainder is queued under the locks, delivered after.
        let mut replaced = None;
        let attached = self.with_connected_port(path.clone(), |cp| {
            if cp.virtual_dlci.is_some() {
                return Err(Error::String("legacy virtual ConnectedPort".into()));
            }
            if let Some(shared) = cp
                .rx_hub
                .lock()
                .ok()
                .and_then(|g| g.as_ref().map(|h| h.shared()))
                .filter(|s| s.has_watch())
            {
                shared.detach_watch_queued();
                replaced = Some(shared);
            }
            self.ensure_rx_hub(&cp.handle(), &path)?;
            let guard = cp
                .rx_hub
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            let hub = guard
                .as_ref()
                .ok_or_else(|| Error::String("RX hub missing after start".into()))?;
            hub.attach_watch_with(channel, batch_timeout, read_size, route_urc, framer.take());
            Ok(())
        });
        if let Some(shared) = replaced {
            shared.deliver_queued();
        }
        if let Err(e) = attached {
            self.unregister_watch(channel_id);
            return Err(e);
        }

        Ok(channel_id)
    }

    pub fn unwatch(&self, channel_id: u32) -> Result<(), Error> {
        let path = self
            .unregister_watch(channel_id)
            .ok_or_else(|| Error::new(format!("No active watch for channel {}", channel_id)))?;
        log_debug!("Stopping watch on port: {} (channel {})", path, channel_id);
        self.stop_watch_thread(&path)
    }

    /// Detach the watch of `path`. The remainder goes to the sink after every lock
    /// here is released, so a sink calling back into the manager cannot deadlock.
    fn stop_watch_thread(&self, path: &str) -> Result<(), Error> {
        let virtual_port = self
            .virtual_ports
            .lock()
            .ok()
            .and_then(|v| v.get(path).cloned());
        if let Some(vp) = virtual_port {
            if let Ok(cp) = self.resolve_connected_port(&vp.physical_path) {
                let session = crate::sync_util::lock_or_recover(&cp.mux).clone();
                if let Some(session) = session {
                    session.clear_watch(vp.dlci);
                }
            }
            return Ok(());
        }
        enum Detach {
            Mux(Arc<CmuxSession>, u8),
            Hub(Arc<crate::hub::RxHubShared>),
            Nothing,
        }
        // A port the core already released (disconnect) has nothing left to detach.
        let detach = self
            .with_connected_port(path.to_string(), |cp| {
                if let Some(dlci) = cp.virtual_dlci {
                    let session = crate::sync_util::lock_or_recover(&cp.mux).clone();
                    return Ok(session.map_or(Detach::Nothing, |s| Detach::Mux(s, dlci)));
                }
                let shared = cp
                    .rx_hub
                    .lock()
                    .ok()
                    .and_then(|g| g.as_ref().map(|h| h.shared()));
                Ok(shared.map_or(Detach::Nothing, Detach::Hub))
            })
            .unwrap_or(Detach::Nothing);
        match detach {
            Detach::Mux(session, dlci) => session.clear_watch(dlci),
            Detach::Hub(shared) => shared.detach_watch(),
            Detach::Nothing => {}
        }
        Ok(())
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
        self.ensure_rx_hub(&cp, &path)?;
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

    /// Write data to the serial port (or a CMUX channel `path#dlci=N`).
    pub fn write(&self, path: String, value: String) -> Result<usize, Error> {
        self.write_bytes(&path, value.as_bytes(), "write data")
            .map_err(Error::from)
    }

    /// Write binary data to the serial port (or a CMUX channel `path#dlci=N`).
    pub fn write_binary(&self, path: String, value: Vec<u8>) -> Result<usize, Error> {
        self.write_bytes(&path, &value, "write binary data")
            .map_err(Error::from)
    }

    /// [`Self::write_binary`] with a [`CoreError`] (the I/O error as its source).
    pub fn write_core(&self, path: String, data: Vec<u8>) -> Result<usize, CoreError> {
        self.write_bytes(&path, &data, "write binary data")
    }

    fn write_bytes(&self, path: &str, data: &[u8], operation: &str) -> Result<usize, CoreError> {
        let virtual_port = crate::sync_util::lock_or_recover(&self.virtual_ports)
            .get(path)
            .cloned();
        if let Some(vp) = virtual_port {
            let cp = self.resolve_connected_port(&vp.physical_path)?;
            let session = crate::sync_util::lock_or_recover(&cp.mux)
                .clone()
                .ok_or_else(|| Error::String("CMUX not enabled".into()))?;
            return session
                .send_uih(vp.dlci, data)
                .map_err(|e| Error::String(format!("Failed to {operation}: {e}")).into());
        }
        let cp = self.resolve_connected_port(path)?;
        match self.writer_of(path, &cp) {
            Some(writer) => write_all_split(&writer, data, operation),
            None => with_port_try_lock(
                &cp.port,
                Duration::from_millis(PORT_LOCK_TIMEOUT_MS),
                |port| write_all_port(port, data, operation),
            ),
        }
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
        if let Some(reason) = self.released_reason(&path) {
            return Err(Error::String(reason));
        }
        let hub_buffered = if let Ok(cp) = self.resolve_connected_port(&path) {
            cp.rx_hub
                .lock()
                .ok()
                .and_then(|guard| guard.as_ref().map(|hub| hub.shared().buffered_len() as u32))
                .unwrap_or(0)
        } else {
            0
        };
        // Wait for the handle (the hub holds the reader during a poll) instead of
        // reporting 0 whenever it is busy.
        let port_buffered = if let Ok(cp) = self.resolve_connected_port(&path) {
            let port = self.writer_of(&path, &cp).unwrap_or(cp.port);
            lock_with_deadline(
                &port,
                Duration::from_millis(PORT_LOCK_TIMEOUT_MS),
                false,
                |p| p.bytes_to_read().map_err(Error::from),
            )
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
    /// `Input` / `All` also drop what the RX hub holds for no one yet.
    pub fn clear_buffer(&self, path: String, buffer_to_clear: ClearBuffer) -> Result<(), Error> {
        let input = !matches!(buffer_to_clear, ClearBuffer::Output);
        self.get_serialport(path.clone(), |port| {
            port.clear(buffer_to_clear.into()).map_err(Error::from)
        })?;
        if input {
            if let Ok(cp) = self.resolve_connected_port(&path) {
                if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
                    hub.shared().purge_buffers();
                }
            }
        }
        Ok(())
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

    /// [`Self::exchange_binary`] with a [`CoreError`] (`Timeout`, `Cancelled`, …).
    pub fn exchange_core(
        &self,
        path: String,
        value: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, CoreError> {
        self.exchange_bytes(path, value, options)
            .map_err(CoreError::from)
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
        let info = ports.get(path).ok_or_else(|| self.not_open_error(path))?;
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
        self.ensure_rx_hub(&cp, &path)?;

        struct DesktopExchangeIo<'a> {
            port: &'a PortArc,
            writer: Option<PortArc>,
            timeout_ms: u64,
        }
        impl crate::exchange::io::ExchangeIo for DesktopExchangeIo<'_> {
            fn purge_rx(&self) -> Result<(), Error> {
                let (port, io_timeout) = match &self.writer {
                    Some(w) => (w, false),
                    None => (self.port, true),
                };
                lock_with_deadline(
                    port,
                    Duration::from_millis(self.timeout_ms.min(5000)),
                    io_timeout,
                    |port| port.clear(ClearBuffer::Input.into()).map_err(Error::from),
                )
            }
            fn write_payload(&self, payload: &[u8]) -> Result<(), Error> {
                if let Some(w) = &self.writer {
                    return write_all_split(w, payload, "exchange write")
                        .map(|_| ())
                        .map_err(Error::from);
                }
                with_port_try_lock(
                    self.port,
                    Duration::from_millis(self.timeout_ms.min(5000)),
                    |port| {
                        write_all_port(port, payload, "exchange write")
                            .map(|_| ())
                            .map_err(Error::from)
                    },
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
                writer: self.writer_of(&path, &cp),
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

        self.ensure_rx_hub_on_physical(&physical_path)?;

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

    /// [`Self::at`] with a [`CoreError`].
    pub fn at_core(
        &self,
        path: String,
        command: String,
        options: Option<AtCommandOptions>,
    ) -> Result<crate::at::parse::AtCommandResult, CoreError> {
        self.at(path, command, options).map_err(CoreError::from)
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
        let response = self.exchange(
            path.clone(),
            format!("{command}\r"),
            ExchangeOptions {
                timeout_ms: Some(timeout_ms),
                rx_prepare: Some(crate::events::RxPrepareMode::None),
                ..Default::default()
            },
        )?;
        if response.status != crate::at::parse::AtParseStatus::Ok {
            return Err(Error::String(format!(
                "modem rejected {command}: {}",
                String::from_utf8_lossy(&response.raw).trim()
            )));
        }
        // The modem switches to multiplexer mode a few ms after its OK. A frame sent
        // inside that window is lost, and the Quectel M26 then ignores SABM (retries
        // included) for about 2 s: wait the switch out before the first frame.
        thread::sleep(CMUX_SWITCH_SETTLE);

        let cp = self.resolve_connected_port(&path)?;
        if cp.virtual_dlci.is_some() {
            return Err(Error::String("enable_mux on virtual port".into()));
        }
        if crate::sync_util::lock_or_recover(&cp.mux).is_some() {
            return Err(Error::String("CMUX already enabled".into()));
        }
        let io_port = self
            .writer_of(&path, &cp)
            .unwrap_or_else(|| cp.port.clone());
        io_port
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .clear(ClearBuffer::Input.into())
            .map_err(Error::from)?;
        let session = CmuxSession::for_command(
            path.clone(),
            Arc::new(crate::cmux::SerialPortIo(io_port)),
            &command,
        );
        // The modem closed channels (DISC / DM / CLD): their state goes out, and when it
        // left multiplexer mode the session is detached (the port is plain again).
        let manager = self.downgrade();
        let weak = Arc::downgrade(&session);
        let physical = path.clone();
        session.set_on_closed(Box::new(move |paths, reason, whole| {
            let Some(m) = manager.upgrade() else {
                return;
            };
            for p in paths {
                m.emit_port_state(
                    p,
                    PortStatus::Disconnected {
                        kind: ErrorKind::Disconnected,
                        reason: reason.to_string(),
                    },
                );
            }
            if whole {
                m.detach_mux_session(&physical, &weak);
            }
        }));
        self.ensure_rx_hub(&cp, &path)?;
        if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
            hub.attach_cmux(session.clone());
        }
        // Control channel first; without its UA the modem is not multiplexing.
        if let Err(e) = session.open_dlci(0, Duration::from_millis(timeout_ms.max(500))) {
            // The modem said OK to AT+CMUX, so it may be multiplexing with the UA lost:
            // CLD takes it back to AT mode.
            session.close_down();
            if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
                hub.detach_cmux();
            }
            return Err(Error::String(e));
        }
        *crate::sync_util::lock_or_recover(&cp.mux) = Some(session);
        Ok(())
    }

    /// Register a virtual DLCI channel on an CMUX-enabled physical port.
    pub fn open_mux_channel(&self, physical_path: String, dlci: u8) -> Result<String, Error> {
        if !crate::cmux::is_data_dlci(dlci) {
            return Err(Error::String(format!(
                "invalid CMUX DLCI {dlci}: data channels are 1..=63"
            )));
        }
        let virtual_path = mux_path(&physical_path, dlci);
        let session = {
            let ports = self
                .serialports
                .lock()
                .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
            let info = ports
                .get(&physical_path)
                .ok_or_else(|| self.not_open_error(&physical_path))?;
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

        // Already open on *this* session. An entry left from an earlier session (port
        // closed and re-opened) is stale and gets replaced below.
        let known = self
            .virtual_ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .contains_key(&virtual_path);
        if known && session.has_dlci(dlci) {
            return Ok(virtual_path);
        }
        // Closed by the modem or left from an earlier session: its watches are dead.
        if known {
            self.forget_watches(&virtual_path);
        }

        session.register_dlci(dlci, virtual_path.clone());
        if let Err(e) = session.open_dlci(dlci, Duration::from_secs(3)) {
            session.unregister_dlci(dlci);
            return Err(Error::String(e));
        }
        self.virtual_ports
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?
            .insert(virtual_path.clone(), {
                let tx_queue = Arc::new(PortTxQueue::new());
                crate::state::VirtualPortRef {
                    physical_path,
                    dlci,
                    exchange_cancel: tx_queue.cancel_flag(),
                    tx_queue,
                }
            });
        self.emit_port_state(&virtual_path, PortStatus::Opened);
        Ok(virtual_path)
    }

    /// Tear down CMUX: close virtual channels and detach deframer.
    /// The CLD wait (up to T2) runs without the manager's map lock.
    pub fn disable_mux(&self, path: String) -> Result<(), Error> {
        self.close_virtual_channels_of(&path);
        let cp = self.resolve_connected_port(&path)?;
        let session = crate::sync_util::lock_or_recover(&cp.mux).take();
        if let Some(session) = session {
            session.close_down();
        }
        if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
            hub.detach_cmux();
        }
        Ok(())
    }

    /// The modem left multiplexer mode: drop `session` from `physical` (if it is still
    /// that port's session) and its deframer from the hub.
    fn detach_mux_session(&self, physical: &str, session: &Weak<CmuxSession>) {
        let Ok(cp) = self.resolve_connected_port(physical) else {
            return;
        };
        let mut mux = crate::sync_util::lock_or_recover(&cp.mux);
        let current = mux
            .as_ref()
            .zip(session.upgrade())
            .is_some_and(|(a, b)| Arc::ptr_eq(a, &b));
        if !current {
            return;
        }
        mux.take();
        drop(mux);
        if let Some(hub) = crate::sync_util::lock_or_recover(&cp.rx_hub).as_ref() {
            hub.detach_cmux();
        };
    }

    /// Close every CMUX virtual channel that rides on `physical`.
    fn close_virtual_channels_of(&self, physical: &str) {
        let virtual_paths: Vec<String> = crate::sync_util::lock_or_recover(&self.virtual_ports)
            .iter()
            .filter(|(_, vp)| vp.physical_path == physical)
            .map(|(p, _)| p.clone())
            .collect();
        for vp in virtual_paths {
            let _ = self.close(vp);
        }
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
        let info = ports.get(path).ok_or_else(|| self.not_open_error(path))?;
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
            .ok_or_else(|| self.not_open_error(&path))?;

        match &mut info.state {
            PortState::Connected(cp) => f(cp),
            other => Err(Error::String(other.not_connected_reason())),
        }
    }

    /// The error for a path with no record: the disconnect reason of a port the core
    /// released (4.x: calls on a dead port fail with a disconnect error), else
    /// "Port 'X' not found".
    fn not_open_error(&self, path: &str) -> Error {
        Error::String(
            self.released_reason(path)
                .unwrap_or_else(|| format!("Port '{}' not found", path)),
        )
    }

    fn released_reason(&self, path: &str) -> Option<String> {
        crate::sync_util::lock_or_recover(&self.core.disconnected)
            .get(path)
            .cloned()
    }

    /// Settings, lines, buffers and `set_timeout` go to the write handle when the port
    /// has one (no wait for the hub's read, no timeout override), else to the port.
    fn get_serialport<T, F>(&self, path: String, f: F) -> Result<T, Error>
    where
        F: FnOnce(&mut Box<dyn serialport::SerialPort>) -> Result<T, Error>,
    {
        let cp = self.resolve_connected_port(&path)?;
        let lock = Duration::from_millis(PORT_LOCK_TIMEOUT_MS);
        match self.writer_of(&path, &cp) {
            Some(writer) => lock_with_deadline(&writer, lock, false, f),
            None => with_port_try_lock(&cp.port, lock, f),
        }
    }
}

/// Batch window, read size and URC routing of a watch.
fn watch_params(options: &WatchOptions) -> (u64, usize, bool) {
    let batch_timeout = options
        .serial_data_flush_interval_ms
        .or(options.timeout)
        .unwrap_or(DEFAULT_WATCH_FLUSH_MS);
    (
        batch_timeout,
        options.size.unwrap_or(1024),
        options.route_urc,
    )
}

/// The 4.2 core API: [`CoreEvent`] streams, port-state subscriptions, queue limits.
impl SerialManager {
    /// Stream one port's events into `sink` as [`CoreEvent::Event`] (the final
    /// `Disconnect` included). Any number of these may run on a port, beside the old
    /// [`Self::watch`]: each gets its own copy of the stream and its own `parser` /
    /// `route_urc` state, and URCs caught during `at()` / `exchange` reach every one
    /// with `route_urc`. See [`CoreSink`] for threading, the queue and overflow.
    pub fn watch_core(
        &self,
        path: String,
        options: WatchOptions,
        parser: Option<serial_codec::framing::FramerConfig>,
        sink: CoreSink,
    ) -> Result<SubscriptionId, CoreError> {
        let framer = parser
            .as_ref()
            .map(|p| p.build())
            .transpose()
            .map_err(|e| CoreError::new(ErrorKind::Other, Error::String(e.to_string())))?;
        let (batch_timeout, read_size, route_urc) = watch_params(&options);
        let id = next_subscription_id();
        let queue = CoreQueue::spawn(sink, self.core_queue_capacity());
        if let Some(reason) = self.released_reason(&path) {
            queue.push(CoreEvent::Event(SerialEvent::Disconnect { path, reason }));
            queue.close();
            return Ok(SubscriptionId(id));
        }
        let virtual_port = crate::sync_util::lock_or_recover(&self.virtual_ports)
            .get(&path)
            .cloned();
        if let Some(vp) = virtual_port {
            let cp = self.resolve_connected_port(&vp.physical_path)?;
            let session = crate::sync_util::lock_or_recover(&cp.mux)
                .clone()
                .ok_or_else(|| Error::String("CMUX not enabled".into()))?;
            self.ensure_rx_hub(&cp, &vp.physical_path)?;
            if !session.add_core_watch(
                vp.dlci,
                id,
                queue.event_sink(),
                queue.alive_flag(),
                batch_timeout,
                framer,
            ) {
                return Err(Error::String(format!("Port '{}' not found", path)).into());
            }
        } else {
            let cp = self.resolve_connected_port(&path)?;
            self.ensure_rx_hub(&cp, &path)?;
            let shared = crate::sync_util::lock_or_recover(&cp.rx_hub)
                .as_ref()
                .map(|h| h.shared())
                .ok_or_else(|| Error::String("RX hub missing after start".into()))?;
            shared.attach_core_watch(crate::hub::shared::CoreWatchSlot::new(
                id,
                queue.event_sink(),
                queue.alive_flag(),
                batch_timeout,
                read_size,
                route_urc,
                framer,
            ));
        }
        crate::sync_util::lock_or_recover(&self.core.core_watches).insert(id, path);
        Ok(SubscriptionId(id))
    }

    /// Stop a [`Self::watch_core`] stream; its batch and parser remainder arrive last.
    /// `NotFound` also for a stream that already ended (overflow, sink returned
    /// `false`, port gone).
    pub fn unwatch_core(&self, id: SubscriptionId) -> Result<(), CoreError> {
        let not_found = || {
            CoreError::new(
                ErrorKind::NotFound,
                Error::new(format!("No active watch for id {}", id.0)),
            )
        };
        let path = crate::sync_util::lock_or_recover(&self.core.core_watches)
            .remove(&id.0)
            .ok_or_else(not_found)?;
        let virtual_port = crate::sync_util::lock_or_recover(&self.virtual_ports)
            .get(&path)
            .cloned();
        let detached = if let Some(vp) = virtual_port {
            self.resolve_connected_port(&vp.physical_path)
                .ok()
                .and_then(|cp| crate::sync_util::lock_or_recover(&cp.mux).clone())
                .is_some_and(|session| session.remove_core_watch(vp.dlci, id.0))
        } else {
            self.resolve_connected_port(&path)
                .ok()
                .and_then(|cp| {
                    crate::sync_util::lock_or_recover(&cp.rx_hub)
                        .as_ref()
                        .map(|h| h.shared())
                })
                .is_some_and(|shared| shared.detach_core_watch(id.0))
        };
        if detached {
            Ok(())
        } else {
            Err(not_found())
        }
    }

    /// Port states of every port of this manager ([`CoreEvent::PortState`]: opened,
    /// disconnected with its kind and reason, closed); needs no watch. Same queue and
    /// overflow rules as [`Self::watch_core`].
    pub fn subscribe_core(&self, sink: CoreSink) -> SubscriptionId {
        let id = next_subscription_id();
        let queue = CoreQueue::spawn(sink, self.core_queue_capacity());
        crate::sync_util::lock_or_recover(&self.core.subscribers).push((id, queue));
        SubscriptionId(id)
    }

    pub fn unsubscribe_core(&self, id: SubscriptionId) -> Result<(), CoreError> {
        let mut subs = crate::sync_util::lock_or_recover(&self.core.subscribers);
        let i = subs.iter().position(|(s, _)| *s == id.0).ok_or_else(|| {
            CoreError::new(
                ErrorKind::NotFound,
                Error::new(format!("No subscription {}", id.0)),
            )
        })?;
        subs.remove(i).1.close();
        Ok(())
    }

    /// Queue limit (bytes of event data) of core receivers created from now on.
    /// Default [`crate::events::DEFAULT_CORE_QUEUE_BYTES`].
    pub fn set_core_queue_capacity(&self, bytes: usize) {
        self.core.queue_capacity.store(bytes, Ordering::SeqCst);
    }

    fn core_queue_capacity(&self) -> usize {
        match self.core.queue_capacity.load(Ordering::SeqCst) {
            0 => crate::events::DEFAULT_CORE_QUEUE_BYTES,
            n => n,
        }
    }

    fn emit_port_state(&self, path: &str, state: PortStatus) {
        let subs: Vec<Arc<CoreQueue>> = crate::sync_util::lock_or_recover(&self.core.subscribers)
            .iter()
            .map(|(_, q)| q.clone())
            .collect();
        for q in subs {
            q.push(CoreEvent::PortState {
                path: path.to_string(),
                state: state.clone(),
            });
        }
        crate::sync_util::lock_or_recover(&self.core.subscribers).retain(|(_, q)| q.alive());
    }

    fn downgrade(&self) -> WeakManager {
        WeakManager {
            serialports: Arc::downgrade(&self.serialports),
            virtual_ports: Arc::downgrade(&self.virtual_ports),
            core: Arc::downgrade(&self.core),
        }
    }

    // --- old watch registry (per manager; mirrored into `port::watch_registry`)

    fn register_watch(&self, id: u32, path: &str) -> Result<(), Error> {
        let mut watches = crate::sync_util::lock_or_recover(&self.core.watches);
        if watches.contains_key(&id) {
            return Err(Error::new(format!("watch id {} is already in use", id)));
        }
        if watches.values().any(|p| p == path) {
            return Err(Error::new(format!(
                "A watch is already active for port {}",
                path
            )));
        }
        // The 4.x global registry is shared by every manager: an id another manager
        // holds is taken too (else one manager's watch would vanish from it).
        if !crate::port::watch_registry::mirror_insert(id, path) {
            return Err(Error::new(format!("watch id {} is already in use", id)));
        }
        watches.insert(id, path.to_string());
        Ok(())
    }

    fn unregister_watch(&self, id: u32) -> Option<String> {
        let path = crate::sync_util::lock_or_recover(&self.core.watches).remove(&id);
        if path.is_some() {
            crate::port::watch_registry::mirror_remove(id);
        }
        path
    }

    fn watch_ids_for(&self, path: &str) -> Vec<u32> {
        crate::sync_util::lock_or_recover(&self.core.watches)
            .iter()
            .filter(|(_, p)| p.as_str() == path)
            .map(|(id, _)| *id)
            .collect()
    }

    fn core_watch_ids_for(&self, path: &str) -> Vec<u64> {
        crate::sync_util::lock_or_recover(&self.core.core_watches)
            .iter()
            .filter(|(_, p)| p.as_str() == path)
            .map(|(id, _)| *id)
            .collect()
    }

    /// Drop the registry entries of `path` (the port is gone; its hub ended the streams).
    fn forget_watches(&self, path: &str) {
        for id in self.watch_ids_for(path) {
            self.unregister_watch(id);
        }
        crate::sync_util::lock_or_recover(&self.core.core_watches).retain(|_, p| p != path);
    }

    // --- port handles and hub

    /// Wrap a freshly opened port: write handle (where it can be split) and RX hub.
    fn connect(
        &self,
        path: &str,
        port: Box<dyn SerialPortTrait>,
        timeout: Duration,
    ) -> (ConnectedPort, Result<(), Error>) {
        let writer = split_writer(path, port.as_ref(), timeout);
        let cp = ConnectedPort::new(port);
        if let Some(writer) = writer {
            crate::sync_util::lock_or_recover(&self.core.writers).insert(
                path.to_string(),
                Writer {
                    reader: Arc::downgrade(&cp.port),
                    port: Arc::new(Mutex::new(writer)),
                },
            );
        }
        let hub = self.ensure_rx_hub(&cp.handle(), path);
        (cp, hub)
    }

    /// The write handle of `path`, if it was split from this very port.
    fn writer_of(&self, path: &str, cp: &ConnectedPortHandle) -> Option<PortArc> {
        crate::sync_util::lock_or_recover(&self.core.writers)
            .get(path)
            .filter(|w| {
                w.reader
                    .upgrade()
                    .is_some_and(|r| Arc::ptr_eq(&r, &cp.port))
            })
            .map(|w| w.port.clone())
    }

    fn drop_writer(&self, path: &str) {
        crate::sync_util::lock_or_recover(&self.core.writers).remove(path);
    }

    /// Start the RX hub if it is not running (it is restarted after an intentional
    /// stop; after a disconnect the port is gone, so it never restarts on a dead fd).
    fn ensure_rx_hub(&self, cp: &ConnectedPortHandle, path: &str) -> Result<(), Error> {
        let mut guard = cp
            .rx_hub
            .lock()
            .map_err(|e| Error::String(format!("Mutex lock failed: {}", e)))?;
        let needs_start = match guard.as_ref() {
            None => true,
            Some(hub) => hub.is_finished(),
        };
        if needs_start {
            let manager = self.downgrade();
            let reader = Arc::downgrade(&cp.port);
            let owner = path.to_string();
            let hook: crate::hub::desktop::DisconnectHook = Box::new(move |reason| {
                if let Some(m) = manager.upgrade() {
                    m.on_port_disconnected(&owner, &reader, reason);
                }
            });
            let hub = Arc::new(crate::hub::PortRxHub::start_with(
                cp.port.clone(),
                path.to_string(),
                Some(hook),
            ));
            register_mobile_hub(path, cp, hub.clone());
            *guard = Some(hub);
        }
        Ok(())
    }

    fn ensure_rx_hub_on_physical(&self, physical_path: &str) -> Result<(), Error> {
        let cp = self.resolve_connected_port(physical_path)?;
        self.ensure_rx_hub(&cp, physical_path)
    }

    /// On the hub thread, after `Disconnect` went out: release the record if it is
    /// still this reader's (see [`Self::release`]). Never joins the hub (its thread).
    fn on_port_disconnected(
        &self,
        path: &str,
        reader: &Weak<Mutex<Box<dyn SerialPortTrait>>>,
        reason: &str,
    ) {
        let removed = {
            let mut map = crate::sync_util::lock_or_recover(&self.serialports);
            let same = match map.get(path).map(|i| &i.state) {
                Some(PortState::Connected(cp)) => {
                    reader.upgrade().is_some_and(|r| Arc::ptr_eq(&r, &cp.port))
                }
                _ => false,
            };
            if same {
                map.remove(path)
            } else {
                None
            }
        };
        if let Some(info) = removed {
            self.release(path, info, reason);
        }
    }

    /// A port that disconnected (its record already out of the map): drop both handles,
    /// the hub and its CMUX channels, keep a tombstone with `reason` for each (listed,
    /// a late watch gets `Disconnect`, calls fail with it, `close` removes it), and send
    /// `PortState::Disconnected`. Watches stay registered until `close` / `unwatch`.
    fn release(&self, path: &str, info: SerialportInfo, reason: &str) {
        let channels: Vec<String> = {
            let mut virtuals = crate::sync_util::lock_or_recover(&self.virtual_ports);
            let keys: Vec<String> = virtuals
                .iter()
                .filter(|(_, vp)| vp.physical_path == path)
                .map(|(k, _)| k.clone())
                .collect();
            for k in &keys {
                if let Some(vp) = virtuals.remove(k) {
                    vp.tx_queue.cancel_all();
                }
            }
            keys
        };
        self.drop_writer(path);
        {
            let mut released = crate::sync_util::lock_or_recover(&self.core.disconnected);
            for p in channels.iter().map(String::as_str).chain([path]) {
                released.insert(p.to_string(), reason.to_string());
            }
        }
        if let PortState::Connected(cp) = info.state {
            // Dropping the hub handle detaches its (ending) thread; the reader goes with it.
            crate::sync_util::lock_or_recover(&cp.rx_hub).take();
            #[cfg(target_os = "android")]
            crate::android::registry::global_registry().unregister(path);
        }
        log_info!("{} released after disconnect: {}", path, reason);
        // The I/O error kind is in `reason`; an unplug is `Disconnected` on every
        // platform (Windows reports it as PermissionDenied).
        for p in channels.iter().map(String::as_str).chain([path]) {
            self.emit_port_state(
                p,
                PortStatus::Disconnected {
                    kind: ErrorKind::Disconnected,
                    reason: reason.to_string(),
                },
            );
        }
    }
}

impl crate::at::commands::ExchangeRunner for SerialManager {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error> {
        SerialManager::run_exchange_unqueued(self, path, payload, options)
    }
}
