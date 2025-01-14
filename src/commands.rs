#[cfg(desktop)]
use crate::desktop_api::SerialPort;
use crate::error::Error;
#[cfg(mobile)]
use crate::mobile_api::SerialPort;
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime, State};

#[tauri::command]
pub fn available_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports()
}

#[tauri::command]
pub fn available_ports_direct<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports_direct()
}

#[tauri::command]
pub fn managed_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<Vec<String>, Error> {
    serial.managed_ports()
}

#[tauri::command]
pub fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.cancel_read(path)
}

#[tauri::command]
pub fn close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.close(path)
}

#[tauri::command]
pub fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<(), Error> {
    serial.close_all()
}

#[tauri::command]
pub fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.force_close(path)
}

#[tauri::command]
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

#[tauri::command]
pub fn write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
) -> Result<usize, Error> {
    serial.write(path, value)
}

#[tauri::command]
pub fn write_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
) -> Result<usize, Error> {
    serial.write_binary(path, value)
}

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

#[tauri::command]
pub fn start_listening<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<(), Error> {
    serial.start_listening(path, timeout, size)
}

#[tauri::command]
pub fn stop_listening<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.stop_listening(path)
}

#[tauri::command]
pub fn set_baud_rate<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
) -> Result<(), Error> {
    serial.set_baud_rate(path, baud_rate)
}

#[tauri::command]
pub fn set_data_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    data_bits: DataBits,
) -> Result<(), Error> {
    serial.set_data_bits(path, data_bits)
}

#[tauri::command]
pub fn set_flow_control<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    flow_control: FlowControl,
) -> Result<(), Error> {
    serial.set_flow_control(path, flow_control)
}

#[tauri::command]
pub fn set_parity<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    parity: Parity,
) -> Result<(), Error> {
    serial.set_parity(path, parity)
}

#[tauri::command]
pub fn set_stop_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    stop_bits: StopBits,
) -> Result<(), Error> {
    serial.set_stop_bits(path, stop_bits)
}

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

#[tauri::command]
pub fn write_request_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_request_to_send(path, level)
}

#[tauri::command]
pub fn write_data_terminal_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_data_terminal_ready(path, level)
}

#[tauri::command]
pub fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_clear_to_send(path)
}

#[tauri::command]
pub fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_data_set_ready(path)
}

#[tauri::command]
pub fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_ring_indicator(path)
}

#[tauri::command]
pub fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_carrier_detect(path)
}

#[tauri::command]
pub fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_read(path)
}

#[tauri::command]
pub fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_write(path)
}

#[tauri::command]
pub fn clear_buffer<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    buffer_type: ClearBuffer,
) -> Result<(), Error> {
    serial.clear_buffer(path, buffer_type)
}

#[tauri::command]
pub fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.set_break(path)
}

#[tauri::command]
pub fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.clear_break(path)
}
