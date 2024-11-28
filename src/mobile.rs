use crate::error::Error;
use crate::mobile_api::SerialPort;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime, State};

#[derive(Debug, Serialize, Deserialize)]
pub struct MobileResult<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[cfg(desktop)]
impl<T> MobileResult<T> {
    fn success(data: T) -> Self {
        MobileResult {
            success: true,
            data: Some(data),
            error: None,
        }
    }

    fn error(message: String) -> Self {
        MobileResult {
            success: false,
            data: None,
            error: Some(message),
        }
    }
}


#[tauri::command]
pub async fn available_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    println!("get Ports");
    serial.available_ports()
}

#[tauri::command]
pub async fn available_ports_direct<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports_direct().await
}

#[tauri::command]
pub async fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.stop_listening(&path).await
}

#[tauri::command]
pub async fn close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.close(&path).await
}

#[tauri::command]
pub async fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<(), Error> {
    serial.close_all().await
}

#[tauri::command]
pub async fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.force_close(&path).await
}

#[tauri::command]
pub async fn open<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
    data_bits: Option<u8>,
    flow_control: Option<u8>,
    parity: Option<u8>,
    stop_bits: Option<u8>,
    timeout: Option<u64>,
) -> Result<(), Error> {
    serial.open(&path, baud_rate, data_bits, flow_control, parity, stop_bits, timeout).await
}

#[tauri::command]
pub async fn write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: String,
) -> Result<usize, Error> {
    serial.write(&path, &value).await
}

#[tauri::command]
pub async fn write_binary<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    value: Vec<u8>,
) -> Result<usize, Error> {
    serial.write_binary(&path, &value).await
}

#[tauri::command]
pub async fn read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<String, Error> {
    serial.read(&path, timeout, size).await
}

#[tauri::command]
pub async fn start_listening<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.start_listening(&path).await
}

#[tauri::command]
pub async fn stop_listening<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.stop_listening(&path).await
}

#[tauri::command]
pub async fn set_baud_rate<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    baud_rate: u32,
) -> Result<(), Error> {
    serial.set_baud_rate(&path, baud_rate).await
}

#[tauri::command]
pub async fn set_data_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    data_bits: u8,
) -> Result<(), Error> {
    serial.set_data_bits(&path, data_bits).await
}

#[tauri::command]
pub async fn set_flow_control<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    flow_control: u8,
) -> Result<(), Error> {
    serial.set_flow_control(&path, flow_control).await
}

#[tauri::command]
pub async fn set_parity<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    parity: u8,
) -> Result<(), Error> {
    serial.set_parity(&path, parity).await
}

#[tauri::command]
pub async fn set_stop_bits<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    stop_bits: u8,
) -> Result<(), Error> {
    serial.set_stop_bits(&path, stop_bits).await
}

#[tauri::command]
pub async fn set_timeout<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    timeout: Duration,
) -> Result<(), Error> {
    serial.set_timeout(&path, timeout).await
}

#[tauri::command]
pub async fn write_request_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_request_to_send(&path, level).await
}

#[tauri::command]
pub async fn write_data_terminal_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    serial.write_data_terminal_ready(&path, level).await
}

#[tauri::command]
pub async fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_clear_to_send(&path).await
}

#[tauri::command]
pub async fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_data_set_ready(&path).await
}

#[tauri::command]
pub async fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_ring_indicator(&path).await
}

#[tauri::command]
pub async fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_carrier_detect(&path).await
}

#[tauri::command]
pub async fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_read(&path).await
}

#[tauri::command]
pub async fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_write(&path).await
}

#[tauri::command]
pub async fn clear_buffer<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
    buffer_type: String,
) -> Result<(), Error> {
    serial.clear_buffer(&path, &buffer_type).await
}

#[tauri::command]
pub async fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.set_break(&path).await
}

#[tauri::command]
pub async fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.clear_break(&path).await
}