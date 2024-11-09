// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::error::Error;
use crate::state::{ReadData, SerialportInfo, SerialportState};
use serde::{Deserialize, Serialize};
use serialport::{DataBits as SerialDataBits, FlowControl as SerialFlowControl,
                 Parity as SerialParity, StopBits as SerialStopBits, ClearBuffer as SerialClearBuffer};
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime, State, Window};

const UNKNOWN: &str = "Unknown";
const USB: &str = "USB";
const BLUETOOTH: &str = "Bluetooth";
const PCI: &str = "PCI";

/// Number of bits per character
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataBits {
    /// 5 bits per character
    Five,
    /// 6 bits per character
    Six,
    /// 7 bits per character
    Seven,
    /// 8 bits per character
    Eight,
}

impl From<DataBits> for SerialDataBits {
    fn from(bits: DataBits) -> Self {
        match bits {
            DataBits::Five => SerialDataBits::Five,
            DataBits::Six => SerialDataBits::Six,
            DataBits::Seven => SerialDataBits::Seven,
            DataBits::Eight => SerialDataBits::Eight,
        }
    }
}

/// Flow control modes
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowControl {
    /// No flow control
    None,
    /// Flow control using XON/XOFF bytes
    Software,
    /// Flow control using RTS/CTS signals
    Hardware,
}

impl From<FlowControl> for SerialFlowControl {
    fn from(flow: FlowControl) -> Self {
        match flow {
            FlowControl::None => SerialFlowControl::None,
            FlowControl::Software => SerialFlowControl::Software,
            FlowControl::Hardware => SerialFlowControl::Hardware,
        }
    }
}

/// Parity checking modes
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Parity {
    /// No parity bit
    None,
    /// Parity bit sets odd number of 1 bits
    Odd,
    /// Parity bit sets even number of 1 bits
    Even,
}

impl From<Parity> for SerialParity {
    fn from(parity: Parity) -> Self {
        match parity {
            Parity::None => SerialParity::None,
            Parity::Odd => SerialParity::Odd,
            Parity::Even => SerialParity::Even,
        }
    }
}

/// Number of stop bits
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopBits {
    /// One stop bit
    One,
    /// Two stop bits
    Two,
}

impl From<StopBits> for SerialStopBits {
    fn from(bits: StopBits) -> Self {
        match bits {
            StopBits::One => SerialStopBits::One,
            StopBits::Two => SerialStopBits::Two,
        }
    }
}

/// Buffer types for clearing
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClearBuffer {
    /// Input buffer (received data)
    Input,
    /// Output buffer (transmitted data)
    Output,
    /// Both input and output buffers
    All,
}

impl From<ClearBuffer> for SerialClearBuffer {
    fn from(buffer: ClearBuffer) -> Self {
        match buffer {
            ClearBuffer::Input => SerialClearBuffer::Input,
            ClearBuffer::Output => SerialClearBuffer::Output,
            ClearBuffer::All => SerialClearBuffer::All,
        }
    }
}

/// Get serial port list
#[tauri::command]
pub fn available_ports() -> HashMap<String, HashMap<String, String>> {
    let mut list = serialport::available_ports().unwrap_or_else(|_| vec![]);
    list.retain(|port| matches!(port.port_type, serialport::SerialPortType::UsbPort(_)));
    list.sort_by(|a, b| a.port_name.cmp(&b.port_name));

    let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();

    for p in list {
        result_list.insert(p.port_name, get_port_info(p.port_type));
    }

    result_list
}

/// Get serial port list using platform-specific commands
#[tauri::command]
pub fn available_ports_direct() -> HashMap<String, HashMap<String, String>> {
    let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();

    #[cfg(target_os = "windows")]
    {
        use std::process::Command;

        // Get USB ports
        let usb_output = Command::new("wmic")
            .arg("path")
            .arg("Win32_PnPEntity")
            .arg("where")
            .arg("PNPDeviceID like '%USB%' and Name like '%(COM%'")
            .arg("get")
            .arg("Name,DeviceID")
            .output()
            .expect("Failed to execute command");

        let usb_devices = String::from_utf8_lossy(&usb_output.stdout);
        for line in usb_devices.lines().skip(1) {
            let device_info = line.trim();
            if !device_info.is_empty() {
                let parts: Vec<&str> = device_info.split_whitespace().collect();
                if parts.len() >= 2 {
                    let port_name = parts[1].trim();
                    let mut port_info = HashMap::new();
                    port_info.insert("type".to_string(), "USB".to_string());
                    result_list.insert(port_name.to_string(), port_info);
                }
            }
        }

        // Get COM ports
        let com_output = Command::new("wmic")
            .arg("path")
            .arg("Win32_SerialPort")
            .arg("get")
            .arg("DeviceID,Name")
            .output()
            .expect("Failed to execute command");

        let com_devices = String::from_utf8_lossy(&com_output.stdout);
        for line in com_devices.lines().skip(1) {
            let device_info = line.trim();
            if !device_info.is_empty() {
                let parts: Vec<&str> = device_info.split_whitespace().collect();
                if parts.len() >= 2 {
                    let port_name = parts[0].trim();
                    let mut port_info = HashMap::new();
                    port_info.insert("type".to_string(), "COM".to_string());
                    result_list.insert(port_name.to_string(), port_info);
                }
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        use std::process::Command;

        // Get USB devices
        let output = Command::new("lsusb")
            .output()
            .expect("Failed to execute lsusb command");

        let usb_devices = String::from_utf8_lossy(&output.stdout);
        for line in usb_devices.lines() {
            if line.contains("Serial") || line.contains("USB") {
                let mut port_info = HashMap::new();
                port_info.insert("type".to_string(), "USB".to_string());
                result_list.insert(line.to_string(), port_info);
            }
        }

        // Get serial ports from /dev
        let dev_output = Command::new("ls")
            .arg("/dev")
            .output()
            .expect("Failed to execute ls command");

        let dev_ports = String::from_utf8_lossy(&dev_output.stdout);
        for line in dev_ports.lines() {
            if line.starts_with("ttyUSB") || line.starts_with("ttyS") {
                let mut port_info = HashMap::new();
                port_info.insert(
                    "type".to_string(),
                    if line.starts_with("ttyUSB") {
                        "USB"
                    } else {
                        "COM"
                    }
                        .to_string(),
                );
                result_list.insert(line.to_string(), port_info);
            }
            if line.starts_with("rfcomm") {
                let mut port_info = HashMap::new();
                port_info.insert("type".to_string(), "Bluetooth".to_string());
                result_list.insert(line.to_string(), port_info);
            }
            if line.starts_with("ttyACM") {
                let mut port_info = HashMap::new();
                port_info.insert("type".to_string(), "Virtual".to_string());
                result_list.insert(line.to_string(), port_info);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::process::Command;

        // Get USB devices
        let output = Command::new("system_profiler")
            .arg("SPUSBDataType")
            .output()
            .expect("Failed to execute system_profiler");

        let usb_devices = String::from_utf8_lossy(&output.stdout);
        for line in usb_devices.lines() {
            if line.contains("Serial") || line.contains("USB") {
                let mut port_info = HashMap::new();
                port_info.insert("type".to_string(), "USB".to_string());
                result_list.insert(line.to_string(), port_info);
            }
        }

        // Check devices in /dev
        let dev_output = Command::new("ls")
            .arg("/dev")
            .output()
            .expect("Failed to execute ls command");

        let dev_ports = String::from_utf8_lossy(&dev_output.stdout);
        for line in dev_ports.lines() {
            if line.starts_with("cu.") || line.starts_with("tty.") {
                let mut port_info = HashMap::new();
                if line.contains("Bluetooth") {
                    port_info.insert("type".to_string(), "Bluetooth".to_string());
                } else if line.starts_with("cu.") {
                    port_info.insert("type".to_string(), "USB".to_string());
                } else {
                    port_info.insert("type".to_string(), "COM".to_string());
                }
                result_list.insert(format!("/dev/{}", line), port_info);
            }
        }
    }

    result_list
}

/// Cancel reading data from the serial port
#[tauri::command]
pub async fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<(), Error> {
    get_serialport(state, path.clone(), |serialport_info| {
        if let Some(sender) = &serialport_info.sender {
            sender.send(1).map_err(|e| {
                Error::String(format!("Failed to cancel serial port data reading: {}", e))
            })?;
        }
        serialport_info.sender = None;
        Ok(())
    })
}

/// Close the specified serial port
#[tauri::command]
pub fn close<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<(), Error> {
    match state.serialports.lock() {
        Ok(mut serialports) => {
            if let Some(port_info) = serialports.remove(&path) {
                // Signal the thread to stop
                if let Some(sender) = &port_info.sender {
                    sender.send(1).map_err(|e| {
                        Error::String(format!("Failed to cancel serial port data reading: {}", e))
                    })?;
                }

                // Wait for the thread to finish
                if let Some(handle) = port_info.thread_handle {
                    handle.join().map_err(|e| {
                        Error::String(format!("Failed to join thread: {:?}", e))
                    })?;
                }

                Ok(())
            } else {
                Err(Error::String(format!("Serial port {} is not open!", &path)))
            }
        }
        Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
    }
}

/// Close all open serial ports
#[tauri::command]
pub fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
) -> Result<(), Error> {
    match state.serialports.lock() {
        Ok(mut map) => {
            let mut errors = Vec::new();

            for (path, port_info) in map.drain() {
                if let Some(sender) = &port_info.sender {
                    if let Err(e) = sender.send(1) {
                        errors.push(format!("Failed to cancel port {}: {}", path, e));
                        continue;
                    }
                }

                if let Some(handle) = port_info.thread_handle {
                    if let Err(e) = handle.join() {
                        errors.push(format!("Failed to join thread for port {}: {:?}", path, e));
                    }
                }
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(Error::String(format!("Errors during close: {}", errors.join(", "))))
            }
        }
        Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
    }
}

/// Force close a serial port
#[tauri::command]
pub fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<(), Error> {
    match state.serialports.lock() {
        Ok(mut map) => {
            if let Some(serial) = map.remove(&path) {
                if let Some(sender) = &serial.sender {
                    sender.send(1).map_err(|e| {
                        Error::String(format!("Failed to cancel serial port data reading: {}", e))
                    })?;
                }

                if let Some(handle) = serial.thread_handle {
                    handle.join().map_err(|e| {
                        Error::String(format!("Failed to join thread: {:?}", e))
                    })?;
                }
            }
            Ok(())
        }
        Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
    }
}

#[tauri::command]
pub fn open<R: Runtime>(
    _app: AppHandle<R>,
    window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
    baud_rate: u32,
    data_bits: Option<DataBits>,
    flow_control: Option<FlowControl>,
    parity: Option<Parity>,
    stop_bits: Option<StopBits>,
    timeout: Option<u64>,
) -> Result<(), Error> {
    match state.serialports.lock() {
        Ok(mut serialports) => {
            if serialports.contains_key(&path) {
                return Err(Error::String(format!("Serial port {} is open!", path)));
            }

            let port = serialport::new(path.clone(), baud_rate)
                .data_bits(data_bits.map(Into::into).unwrap_or(SerialDataBits::Eight))
                .flow_control(flow_control.map(Into::into).unwrap_or(SerialFlowControl::None))
                .parity(parity.map(Into::into).unwrap_or(SerialParity::None))
                .stop_bits(stop_bits.map(Into::into).unwrap_or(SerialStopBits::One))
                .timeout(Duration::from_millis(timeout.unwrap_or(200)))
                .open()
                .map_err(|e| Error::String(format!("Failed to open serial port: {}", e)))?;

            let mut port_info = SerialportInfo {
                serialport: port,
                sender: None,
                thread_handle: None,
            };

            // Start listening immediately after opening
            let event_path = path.replace(".", "");
            let read_event = format!("plugin-serialplugin-read-{}", &event_path);
            let disconnected_event = format!("plugin-serialplugin-disconnected-{}", &event_path);

            let mut serial = port_info
                .serialport
                .try_clone()
                .map_err(|e| Error::String(format!("Failed to clone serial port: {}", e)))?;

            let (tx, rx): (Sender<usize>, Receiver<usize>) = mpsc::channel();
            port_info.sender = Some(tx);

            let window_clone = window.clone();
            let path_clone = path.clone();
            let thread_handle = thread::spawn(move || {
                loop {
                    match rx.try_recv() {
                        Ok(_) => break,
                        Err(TryRecvError::Disconnected) => {
                            if let Err(e) = window_clone.emit(
                                &disconnected_event,
                                format!("Serial port {} disconnected!", &path_clone),
                            ) {
                                eprintln!("Failed to send disconnection event: {}", e);
                            }
                            break;
                        }
                        Err(TryRecvError::Empty) => {}
                    }

                    let mut buffer = vec![0; 1024];
                    match serial.read(&mut buffer) {
                        Ok(n) => {
                            if let Err(e) = window.emit(
                                &read_event,
                                ReadData {
                                    data: &buffer[..n],
                                    size: n,
                                },
                            ) {
                                eprintln!("Failed to send data: {}", e);
                            }
                        }
                        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(e) => {
                            eprintln!("Failed to read data: {}", e);
                            break; // Exit on error
                        }
                    }

                    thread::sleep(Duration::from_millis(timeout.unwrap_or(200)));
                }
            });

            port_info.thread_handle = Some(thread_handle);

            serialports.insert(path, port_info);
            Ok(())
        }
        Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
    }
}

/// Read data from the serial port
#[tauri::command]
pub fn read<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
    timeout: Option<u64>,
    size: Option<usize>,
) -> Result<String, Error> {
    get_serialport(state.clone(), path.clone(), |serialport_info| {
        let mut buffer = vec![0; size.unwrap_or(1024)];
        serialport_info.serialport.set_timeout(Duration::from_millis(timeout.unwrap_or(200)))
            .map_err(|e| Error::String(format!("Failed to set timeout: {}", e)))?;

        match serialport_info.serialport.read(&mut buffer) {
            Ok(n) => {
                let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                Ok(data)
            },
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(String::new()),
            Err(e) => Err(Error::String(format!("Failed to read data: {}", e))),
        }
    })
}

/// Write data to the serial port
#[tauri::command]
pub fn write<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
    value: String,
) -> Result<usize, Error> {
    get_serialport(state, path.clone(), |serialport_info| {
        serialport_info
            .serialport
            .write(value.as_bytes())
            .map_err(|e| Error::String(format!("Failed to write data: {}", e)))
    })
}

/// Write binary data to the serial port
#[tauri::command]
pub fn write_binary<R: Runtime>(
    _app: AppHandle<R>,
    _window: Window<R>,
    state: State<'_, SerialportState>,
    path: String,
    value: Vec<u8>,
) -> Result<usize, Error> {
    get_serialport(state, path.clone(), |serialport_info| {
        serialport_info
            .serialport
            .write(&value)
            .map_err(|e| Error::String(format!("Failed to write binary data: {}", e)))
    })
}

/// Set the baud rate
#[tauri::command]
pub async fn set_baud_rate<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    baud_rate: u32,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_baud_rate(baud_rate)
            .map_err(Error::from)
    })
}

/// Set the data bits
#[tauri::command]
pub async fn set_data_bits<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    data_bits: DataBits,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_data_bits(data_bits.into())
            .map_err(Error::from)
    })
}

/// Set the flow control
#[tauri::command]
pub async fn set_flow_control<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    flow_control: FlowControl,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_flow_control(flow_control.into())
            .map_err(Error::from)
    })
}

/// Set the parity
#[tauri::command]
pub async fn set_parity<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    parity: Parity,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_parity(parity.into())
            .map_err(Error::from)
    })
}

/// Set the stop bits
#[tauri::command]
pub async fn set_stop_bits<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    stop_bits: StopBits,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_stop_bits(stop_bits.into())
            .map_err(Error::from)
    })
}

/// Set the timeout
#[tauri::command]
pub async fn set_timeout<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    timeout: Duration,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .set_timeout(timeout)
            .map_err(Error::from)
    })
}

/// Set the RTS (Request To Send) control signal
#[tauri::command]
pub async fn write_request_to_send<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .write_request_to_send(level)
            .map_err(Error::from)
    })
}

/// Set the DTR (Data Terminal Ready) control signal
#[tauri::command]
pub async fn write_data_terminal_ready<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    level: bool,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .write_data_terminal_ready(level)
            .map_err(Error::from)
    })
}

/// Read the CTS (Clear To Send) control signal state
#[tauri::command]
pub async fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<bool, Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .read_clear_to_send()
            .map_err(Error::from)
    })
}

/// Read the DSR (Data Set Ready) control signal state
#[tauri::command]
pub async fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<bool, Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .read_data_set_ready()
            .map_err(Error::from)
    })
}

/// Read the RI (Ring Indicator) control signal state
#[tauri::command]
pub async fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<bool, Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .read_ring_indicator()
            .map_err(Error::from)
    })
}

/// Read the CD (Carrier Detect) control signal state
#[tauri::command]
pub async fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<bool, Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .read_carrier_detect()
            .map_err(Error::from)
    })
}

/// Get the number of bytes available to read
#[tauri::command]
pub async fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<u32, Error> {
    get_serialport(state, path, |port_info| {
        port_info.serialport.bytes_to_read().map_err(Error::from)
    })
}

/// Get the number of bytes waiting to be written
#[tauri::command]
pub async fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<u32, Error> {
    get_serialport(state, path, |port_info| {
        port_info.serialport.bytes_to_write().map_err(Error::from)
    })
}

/// Clear input/output buffers
#[tauri::command]
pub async fn clear_buffer<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
    buffer_to_clear: ClearBuffer,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info
            .serialport
            .clear(buffer_to_clear.into())
            .map_err(Error::from)
    })
}

/// Start break signal transmission
#[tauri::command]
pub async fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info.serialport.set_break().map_err(Error::from)
    })
}

/// Stop break signal transmission
#[tauri::command]
pub async fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    state: State<'_, SerialportState>,
    path: String,
) -> Result<(), Error> {
    get_serialport(state, path, |port_info| {
        port_info.serialport.clear_break().map_err(Error::from)
    })
}

fn get_serialport<T, F: FnOnce(&mut SerialportInfo) -> Result<T, Error>>(
    state: State<'_, SerialportState>,
    path: String,
    f: F,
) -> Result<T, Error> {
    match state.serialports.lock() {
        Ok(mut map) => match map.get_mut(&path) {
            Some(serialport_info) => f(serialport_info),
            None => Err(Error::String("Serial port not found".to_string())),
        },
        Err(error) => Err(Error::String(format!(
            "Failed to acquire file lock! {}",
            error
        ))),
    }
}

fn get_port_info(port: serialport::SerialPortType) -> HashMap<String, String> {
    let mut port_info: HashMap<String, String> = HashMap::new();
    port_info.insert("type".to_string(), UNKNOWN.to_string());
    port_info.insert("vid".to_string(), UNKNOWN.to_string());
    port_info.insert("pid".to_string(), UNKNOWN.to_string());
    port_info.insert("serial_number".to_string(), UNKNOWN.to_string());
    port_info.insert("manufacturer".to_string(), UNKNOWN.to_string());
    port_info.insert("product".to_string(), UNKNOWN.to_string());

    match port {
        serialport::SerialPortType::UsbPort(info) => {
            port_info.insert("type".to_string(), USB.to_string());
            port_info.insert("vid".to_string(), info.vid.to_string());
            port_info.insert("pid".to_string(), info.pid.to_string());
            port_info.insert(
                "serial_number".to_string(),
                info.serial_number.unwrap_or_else(|| UNKNOWN.to_string()),
            );
            port_info.insert(
                "manufacturer".to_string(),
                info.manufacturer.unwrap_or_else(|| UNKNOWN.to_string()),
            );
            port_info.insert(
                "product".to_string(),
                info.product.unwrap_or_else(|| UNKNOWN.to_string()),
            );
        }
        serialport::SerialPortType::BluetoothPort => {
            port_info.insert("type".to_string(), BLUETOOTH.to_string());
        }
        serialport::SerialPortType::PciPort => {
            port_info.insert("type".to_string(), PCI.to_string());
        }
        serialport::SerialPortType::Unknown => {
            port_info.insert("type".to_string(), UNKNOWN.to_string());
        }
    }

    port_info
}