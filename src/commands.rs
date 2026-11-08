//! Tauri command handlers for serial port I/O. See README and docs.rs.

use crate::api::SerialPort;
use crate::error::Error;
use crate::events::{Capabilities, ExchangeOptions, SerialEvent, WatchOptions};
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use std::collections::HashMap;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State};

/// Run port I/O on the blocking pool: plain sync commands run on the main thread
/// in Tauri v2, so a `read` waiting for data would freeze the UI.
async fn blocking<R: Runtime, T: Send + 'static>(
    serial: &State<'_, SerialPort<R>>,
    f: impl FnOnce(SerialPort<R>) -> Result<T, Error> + Send + 'static,
) -> Result<T, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || f(serial))
        .await
        .map_err(|e| Error::String(format!("serial task failed: {e}")))?
}

/// Lists all available serial ports on the system
/// See README for examples.
#[tauri::command]
pub async fn available_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    single_port_per_device: Option<bool>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    blocking(&serial, move |s| {
        s.available_ports(single_port_per_device.unwrap_or(false))
    })
    .await
}

/// Lists all currently managed serial ports
/// See README for examples.
#[tauri::command]
pub fn managed_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<Vec<String>, Error> {
    serial.managed_ports()
}

/// Cancels an in-flight poll `read` on a serial port (does not stop an active `watch`).
/// See README for examples.
#[tauri::command]
pub async fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.cancel_read(path)).await
}

/// Closes a serial port
/// See README for examples.
#[tauri::command]
pub async fn close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.close(path)).await
}

/// Closes all open serial ports
/// See README for examples.
#[tauri::command]
pub async fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.close_all()).await
}

/// Forcefully closes a serial port
/// See README for examples.
#[tauri::command]
pub async fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.force_close(path)).await
}

/// Opens a serial port with the specified configuration
/// See README for examples.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn open<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
    data_bits: Option<DataBits>,
    flow_control: Option<FlowControl>,
    parity: Option<Parity>,
    stop_bits: Option<StopBits>,
    timeout: Option<u64>,
    dtr: Option<bool>,
    rts: Option<bool>,
) -> Result<String, Error> {
    blocking(&serial, move |s| {
        s.open_with_lines(
            path,
            baud_rate,
            data_bits,
            flow_control,
            parity,
            stop_bits,
            timeout,
            crate::state::ModemLinesOnOpen { dtr, rts },
        )
    })
    .await
}

/// Writes string data to a serial port
/// See README for examples.
#[tauri::command]
pub async fn write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
) -> Result<usize, Error> {
    blocking(&serial, move |s| s.write(path, value)).await
}

/// Writes binary data to a serial port
/// See README for examples.
#[tauri::command]
pub async fn write_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
) -> Result<usize, Error> {
    blocking(&serial, move |s| s.write_binary(path, value)).await
}

/// Reads string data from a serial port
/// See README for examples.
#[tauri::command]
pub async fn read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<String, Error> {
    blocking(&serial, move |s| s.read(path, timeout, size)).await
}

/// Reads binary data from a serial port
/// See README for examples.
#[tauri::command]
pub async fn read_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<Vec<u8>, Error> {
    blocking(&serial, move |s| s.read_binary(path, timeout, size)).await
}

/// Returns runtime capabilities (transport, platform, version) without window probing.
/// See README for examples.
#[tauri::command]
pub fn capabilities() -> Result<Capabilities, Error> {
    Ok(Capabilities::current())
}

/// Streams serial port events through a Tauri IPC channel.
/// See README for examples.
#[tauri::command]
pub async fn watch<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    options: WatchOptions,
    channel: Channel<SerialEvent>,
) -> Result<u32, Error> {
    blocking(&serial, move |s| s.watch(path, options, channel)).await
}

/// Stops a watch session by channel id.
/// See README for examples.
#[tauri::command]
pub async fn unwatch<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    channel_id: u32,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.unwatch(channel_id)).await
}

/// Streams available-port attach/detach events through a Tauri IPC channel.
#[tauri::command]
pub async fn watch_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    options: Option<crate::events::WatchPortsOptions>,
    channel: tauri::ipc::Channel<crate::events::PortListEvent>,
) -> Result<u32, Error> {
    blocking(&serial, move |s| {
        s.watch_ports(options.unwrap_or_default(), channel)
    })
    .await
}

/// Stops a port-list watch session by channel id.
#[tauri::command]
pub async fn unwatch_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    channel_id: u32,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.unwatch_ports(channel_id)).await
}

/// Sets the baud rate for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_baud_rate<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_baud_rate(path, baud_rate)).await
}

/// Sets the number of data bits for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_data_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    data_bits: DataBits,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_data_bits(path, data_bits)).await
}

/// Sets the flow control mode for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_flow_control<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    flow_control: FlowControl,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_flow_control(path, flow_control)).await
}

/// Sets the parity checking mode for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_parity<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    parity: Parity,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_parity(path, parity)).await
}

/// Sets the number of stop bits for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_stop_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    stop_bits: StopBits,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_stop_bits(path, stop_bits)).await
}

/// Sets the read timeout for a serial port
/// See README for examples.
#[tauri::command]
pub async fn set_timeout<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: u64,
) -> Result<(), Error> {
    blocking(&serial, move |s| {
        let timeout_duration = Duration::from_millis(timeout);
        s.set_timeout(path, timeout_duration)
    })
    .await
}

/// Sets the RTS (Request To Send) control signal
/// See README for examples.
#[tauri::command]
pub async fn write_request_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.write_request_to_send(path, level)).await
}

/// Sets the DTR (Data Terminal Ready) control signal
/// See README for examples.
#[tauri::command]
pub async fn write_data_terminal_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.write_data_terminal_ready(path, level)).await
}

/// Reads the CTS (Clear To Send) control signal state
/// See README for examples.
#[tauri::command]
pub async fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    blocking(&serial, move |s| s.read_clear_to_send(path)).await
}

/// Reads the DSR (Data Set Ready) control signal state
/// See README for examples.
#[tauri::command]
pub async fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    blocking(&serial, move |s| s.read_data_set_ready(path)).await
}

/// Reads the RI (Ring Indicator) control signal state
/// See README for examples.
#[tauri::command]
pub async fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    blocking(&serial, move |s| s.read_ring_indicator(path)).await
}

/// Reads the CD (Carrier Detect) control signal state
/// See README for examples.
#[tauri::command]
pub async fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    blocking(&serial, move |s| s.read_carrier_detect(path)).await
}

/// Gets the number of bytes available to read from the serial port
/// See README for examples.
#[tauri::command]
pub async fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    blocking(&serial, move |s| s.bytes_to_read(path)).await
}

/// Gets the number of bytes available to write to the serial port
/// See README for examples.
#[tauri::command]
pub async fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    blocking(&serial, move |s| s.bytes_to_write(path)).await
}

/// Clears the specified buffer of the serial port
/// See README for examples.
#[tauri::command]
pub async fn clear_buffer<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    buffer_type: ClearBuffer,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.clear_buffer(path, buffer_type)).await
}

/// Sets the break condition on the serial port
/// See README for examples.
#[tauri::command]
pub async fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.set_break(path)).await
}

/// Clears the break condition on the serial port
/// See README for examples.
#[tauri::command]
pub async fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.clear_break(path)).await
}

/// Sets the global log level for the plugin
/// See README for examples.
#[tauri::command]
pub fn set_log_level<R: Runtime>(
    _app: AppHandle<R>,
    _serial: State<'_, SerialPort<R>>,
    level: crate::state::LogLevel,
) -> Result<(), Error> {
    crate::state::set_log_level(level);
    Ok(())
}

/// Gets the current global log level
/// See README for examples.
#[tauri::command]
pub fn get_log_level<R: Runtime>(
    _app: AppHandle<R>,
    _serial: State<'_, SerialPort<R>>,
) -> Result<crate::state::LogLevel, Error> {
    Ok(crate::state::get_log_level())
}

/// Write and read-until-response (AT-style exchange).
/// See README for examples.
#[tauri::command]
pub async fn exchange<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
    options: Option<ExchangeOptions>,
) -> Result<crate::at::parse::ExchangeResponse, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        serial.exchange(path, value, options.unwrap_or_default())
    })
    .await
    .map_err(|e| Error::String(format!("exchange task failed: {e}")))?
}

/// Cancel an in-flight exchange on a port.
/// See README for examples.
#[tauri::command]
pub async fn cancel_exchange<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.cancel_exchange(path)).await
}

/// Send one AT command with native session defaults and FIFO queue.
#[tauri::command]
pub async fn at<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    command: String,
    options: Option<crate::at::session::AtCommandOptions>,
) -> Result<crate::at::parse::AtCommandResult, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || serial.at(path, command, options))
        .await
        .map_err(|e| Error::String(format!("at task failed: {e}")))?
}

/// Multi-phase AT flow (e.g. CMGS) as one atomic queue job.
#[tauri::command]
pub async fn at_phases<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    phases: Vec<crate::at::session::AtPhase>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || serial.at_phases(path, phases))
        .await
        .map_err(|e| Error::String(format!("at_phases task failed: {e}")))?
}

/// Built-in CMGS recipe through the native transaction queue.
#[tauri::command]
pub async fn send_sms_pdu<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    length: u32,
    pdu: Vec<u8>,
    options: Option<crate::at::session::SendSmsPduOptions>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || serial.send_sms_pdu(path, length, pdu, options))
        .await
        .map_err(|e| Error::String(format!("send_sms_pdu task failed: {e}")))?
}

/// Configure AT session defaults for native `at` on a port.
#[tauri::command]
pub async fn configure_at_session<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    session: crate::at::session::AtSessionConfig,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.configure_at_session(path, session)).await
}

/// Binary write + read-until-response (e.g. CMGS PDU).
#[tauri::command]
pub async fn exchange_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
    options: Option<crate::events::ExchangeOptions>,
) -> Result<crate::at::parse::ExchangeResponse, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        serial.exchange_binary(path, value, options.unwrap_or_default())
    })
    .await
    .map_err(|e| Error::String(format!("exchange_binary task failed: {e}")))?
}

#[derive(Debug, Clone, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnableMuxOptions {
    pub command: Option<String>,
    pub timeout_ms: Option<u64>,
}

/// Enter GSM 07.10 CMUX mode on a physical port (desktop).
#[tauri::command]
pub async fn enable_mux<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    options: Option<EnableMuxOptions>,
) -> Result<(), Error> {
    let opts = options.unwrap_or_default();
    let command = opts
        .command
        .unwrap_or_else(|| "AT+CMUX=0,0,5,31,10,2".to_string());
    let timeout_ms = opts.timeout_ms.unwrap_or(5000);
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || serial.enable_mux(path, command, timeout_ms))
        .await
        .map_err(|e| Error::String(format!("enable_mux task failed: {e}")))?
}

/// Open a virtual CMUX channel (returns composite path).
#[tauri::command]
pub async fn open_mux_channel<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    dlci: u8,
) -> Result<String, Error> {
    blocking(&serial, move |s| s.open_mux_channel(path, dlci)).await
}

/// Tear down CMUX and close virtual channels.
#[tauri::command]
pub async fn disable_mux<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    blocking(&serial, move |s| s.disable_mux(path)).await
}

/// Scan for Bluetooth LE devices advertising the Nordic UART Service.
/// Returns `ble://<id>` paths (open them like any port). Needs the `ble` feature.
#[tauri::command]
pub async fn ble_scan(
    timeout_ms: Option<u64>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    let duration = Duration::from_millis(timeout_ms.unwrap_or(3000).clamp(500, 30_000));
    #[cfg(all(feature = "ble", not(target_os = "android")))]
    {
        tauri::async_runtime::spawn_blocking(move || crate::port::ble::scan(duration))
            .await
            .map_err(|e| Error::String(format!("BLE scan task failed: {e}")))?
            .map_err(|e| Error::String(e.to_string()))
    }
    #[cfg(not(all(feature = "ble", not(target_os = "android"))))]
    {
        let _ = duration;
        Err(Error::String(
            "BLE needs the plugin's `ble` cargo feature (desktop and iOS; not Android yet)".into(),
        ))
    }
}
