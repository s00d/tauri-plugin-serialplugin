//! Tauri command handlers for serial port I/O. See README and docs.rs.

#[cfg(desktop)]
use crate::desktop_api::SerialPort;
use crate::error::Error;
use crate::events::{Capabilities, ExchangeOptions, SerialEvent, WatchOptions};
#[cfg(mobile)]
use crate::mobile_api::SerialPort;
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use std::collections::HashMap;
use std::time::Duration;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State};

/// Lists all available serial ports on the system
/// See README for examples.
#[tauri::command]
pub fn available_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    single_port_per_device: Option<bool>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports(single_port_per_device.unwrap_or(false))
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
pub fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.cancel_read(path)
}

/// Closes a serial port
/// See README for examples.
#[tauri::command]
pub fn close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.close(path)
}

/// Closes all open serial ports
/// See README for examples.
#[tauri::command]
pub fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<(), Error> {
    serial.close_all()
}

/// Forcefully closes a serial port
/// See README for examples.
#[tauri::command]
pub fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.force_close(path)
}

/// Opens a serial port with the specified configuration
/// See README for examples.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn open<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
    data_bits: Option<DataBits>,
    flow_control: Option<FlowControl>,
    parity: Option<Parity>,
    stop_bits: Option<StopBits>,
    timeout: Option<u64>,
) -> Result<(), Error> {
    serial.open(
        path,
        baud_rate,
        data_bits,
        flow_control,
        parity,
        stop_bits,
        timeout,
    )
}

/// Writes string data to a serial port
/// See README for examples.
#[tauri::command]
pub fn write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
) -> Result<usize, Error> {
    serial.write(path, value)
}

/// Writes binary data to a serial port
/// See README for examples.
#[tauri::command]
pub fn write_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
) -> Result<usize, Error> {
    serial.write_binary(path, value)
}

/// Reads string data from a serial port
/// See README for examples.
#[tauri::command]
pub fn read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<String, Error> {
    serial.read(path, timeout, size)
}

/// Reads binary data from a serial port
/// See README for examples.
#[tauri::command]
pub fn read_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<Vec<u8>, Error> {
    serial.read_binary(path, timeout, size)
}

/// Returns runtime capabilities (transport, platform, version) without window probing.
/// See README for examples.
#[tauri::command]
pub fn capabilities() -> Capabilities {
    Capabilities::current()
}

/// Streams serial port events through a Tauri IPC channel.
/// See README for examples.
#[tauri::command]
pub fn watch<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    options: WatchOptions,
    channel: Channel<SerialEvent>,
) -> Result<u32, Error> {
    serial.watch(path, options, channel)
}

/// Stops a watch session by channel id.
/// See README for examples.
#[tauri::command]
pub fn unwatch<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    channel_id: u32,
) -> Result<(), Error> {
    serial.unwatch(channel_id)
}

/// Streams available-port attach/detach events through a Tauri IPC channel.
#[tauri::command]
pub fn watch_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    options: Option<crate::events::WatchPortsOptions>,
    channel: tauri::ipc::Channel<crate::events::PortListEvent>,
) -> Result<u32, Error> {
    serial.watch_ports(options.unwrap_or_default(), channel)
}

/// Stops a port-list watch session by channel id.
#[tauri::command]
pub fn unwatch_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    channel_id: u32,
) -> Result<(), Error> {
    serial.unwatch_ports(channel_id)
}

/// Sets the baud rate for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_baud_rate<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
) -> Result<(), Error> {
    serial.set_baud_rate(path, baud_rate)
}

/// Sets the number of data bits for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_data_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    data_bits: DataBits,
) -> Result<(), Error> {
    serial.set_data_bits(path, data_bits)
}

/// Sets the flow control mode for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_flow_control<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    flow_control: FlowControl,
) -> Result<(), Error> {
    serial.set_flow_control(path, flow_control)
}

/// Sets the parity checking mode for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_parity<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    parity: Parity,
) -> Result<(), Error> {
    serial.set_parity(path, parity)
}

/// Sets the number of stop bits for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_stop_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    stop_bits: StopBits,
) -> Result<(), Error> {
    serial.set_stop_bits(path, stop_bits)
}

/// Sets the read timeout for a serial port
/// See README for examples.
#[tauri::command]
pub fn set_timeout<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: u64,
) -> Result<(), Error> {
    let timeout_duration = Duration::from_millis(timeout);
    serial.set_timeout(path, timeout_duration)
}

/// Sets the RTS (Request To Send) control signal
/// See README for examples.
#[tauri::command]
pub fn write_request_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_request_to_send(path, level)
}

/// Sets the DTR (Data Terminal Ready) control signal
/// See README for examples.
#[tauri::command]
pub fn write_data_terminal_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_data_terminal_ready(path, level)
}

/// Reads the CTS (Clear To Send) control signal state
/// See README for examples.
#[tauri::command]
pub fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_clear_to_send(path)
}

/// Reads the DSR (Data Set Ready) control signal state
/// See README for examples.
#[tauri::command]
pub fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_data_set_ready(path)
}

/// Reads the RI (Ring Indicator) control signal state
/// See README for examples.
#[tauri::command]
pub fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_ring_indicator(path)
}

/// Reads the CD (Carrier Detect) control signal state
/// See README for examples.
#[tauri::command]
pub fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_carrier_detect(path)
}

/// Gets the number of bytes available to read from the serial port
/// See README for examples.
#[tauri::command]
pub fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_read(path)
}

/// Gets the number of bytes available to write to the serial port
/// See README for examples.
#[tauri::command]
pub fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_write(path)
}

/// Clears the specified buffer of the serial port
/// See README for examples.
#[tauri::command]
pub fn clear_buffer<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    buffer_type: ClearBuffer,
) -> Result<(), Error> {
    serial.clear_buffer(path, buffer_type)
}

/// Sets the break condition on the serial port
/// See README for examples.
#[tauri::command]
pub fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.set_break(path)
}

/// Clears the break condition on the serial port
/// See README for examples.
#[tauri::command]
pub fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.clear_break(path)
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
) -> Result<crate::at_parse::ExchangeResponse, Error> {
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
pub fn cancel_exchange<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.cancel_exchange(path)
}

/// Send one AT command with native session defaults and FIFO queue.
#[tauri::command]
pub async fn at<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    command: String,
    options: Option<crate::at_session::AtCommandOptions>,
) -> Result<crate::at_parse::AtCommandResult, Error> {
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
    phases: Vec<crate::at_session::AtPhase>,
) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
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
    options: Option<crate::at_session::SendSmsPduOptions>,
) -> Result<Vec<crate::at_parse::AtCommandResult>, Error> {
    let serial = serial.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        serial.send_sms_pdu(path, length, pdu, options)
    })
    .await
    .map_err(|e| Error::String(format!("send_sms_pdu task failed: {e}")))?
}

/// Configure AT session defaults for native `at` on a port.
#[tauri::command]
pub fn configure_at_session<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    session: crate::at_session::AtSessionConfig,
) -> Result<(), Error> {
    serial.configure_at_session(path, session)
}

/// Binary write + read-until-response (e.g. CMGS PDU).
#[tauri::command]
pub async fn exchange_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
    options: Option<crate::events::ExchangeOptions>,
) -> Result<crate::at_parse::ExchangeResponse, Error> {
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
pub fn open_mux_channel<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    dlci: u8,
) -> Result<String, Error> {
    serial.open_mux_channel(path, dlci)
}

/// Tear down CMUX and close virtual channels.
#[tauri::command]
pub fn disable_mux<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.disable_mux(path)
}
