use crate::error::Error;
use crate::state::{
    ClearBuffer, DataBits, FlowControl, Parity, ReadData, SerialportInfo, StopBits, BLUETOOTH, PCI,
    UNKNOWN, USB,
};
use serde::{Deserialize, Serialize};
use serialport::{
    DataBits as SerialDataBits, FlowControl as SerialFlowControl, Parity as SerialParity,
    StopBits as SerialStopBits,
};
use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Runtime};

/// Access to the serial port APIs for mobile platforms.
pub struct SerialPort<R: Runtime> {
    #[allow(dead_code)]
    pub(crate) app: AppHandle<R>,
    pub(crate) serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct MobileResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

impl<R: Runtime> SerialPort<R> {
    /// Get serial port list
    pub fn available_ports(&self) -> Result<HashMap<String, HashMap<String, String>>, Error> {
        let mut list = serialport::available_ports().unwrap_or_else(|_| vec![]);
        list.retain(|port| matches!(port.port_type, serialport::SerialPortType::UsbPort(_)));
        list.sort_by(|a, b| a.port_name.cmp(&b.port_name));

        let mut result_list: HashMap<String, HashMap<String, String>> = HashMap::new();

        for p in list {
            result_list.insert(p.port_name, self.get_port_info(p.port_type));
        }

        Ok(result_list)
    }

    /// Get serial port list using platform-specific commands
    pub fn available_ports_direct(
        &self,
    ) -> Result<HashMap<String, HashMap<String, String>>, Error> {
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
                    result_list.insert(format!("/dev/{}", line), port_info);
                }
                if line.starts_with("rfcomm") {
                    let mut port_info = HashMap::new();
                    port_info.insert("type".to_string(), "Bluetooth".to_string());
                    result_list.insert(format!("/dev/{}", line), port_info);
                }
                if line.starts_with("ttyACM") {
                    let mut port_info = HashMap::new();
                    port_info.insert("type".to_string(), "Virtual".to_string());
                    result_list.insert(format!("/dev/{}", line), port_info);
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

        Ok(result_list)
    }

    /// Cancel reading data from the serial port
    pub fn cancel_read(&self, path: String) -> Result<(), Error> {
        self.get_serialport(path.clone(), |serialport_info| {
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
    pub fn close(&self, path: String) -> Result<(), Error> {
        match self.serialports.lock() {
            Ok(mut serialports) => {
                if let Some(port_info) = serialports.remove(&path) {
                    // Signal the thread to stop
                    if let Some(sender) = &port_info.sender {
                        sender.send(1).map_err(|e| {
                            Error::String(format!(
                                "Failed to cancel serial port data reading: {}",
                                e
                            ))
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
    pub fn close_all(&self) -> Result<(), Error> {
        match self.serialports.lock() {
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
                            errors
                                .push(format!("Failed to join thread for port {}: {:?}", path, e));
                        }
                    }
                }

                if errors.is_empty() {
                    Ok(())
                } else {
                    Err(Error::String(format!(
                        "Errors during close: {}",
                        errors.join(", ")
                    )))
                }
            }
            Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
        }
    }

    /// Force close a serial port
    pub fn force_close(&self, path: String) -> Result<(), Error> {
        match self.serialports.lock() {
            Ok(mut map) => {
                if let Some(serial) = map.remove(&path) {
                    if let Some(sender) = &serial.sender {
                        sender.send(1).map_err(|e| {
                            Error::String(format!(
                                "Failed to cancel serial port data reading: {}",
                                e
                            ))
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
        match self.serialports.lock() {
            Ok(mut serialports) => {
                if serialports.contains_key(&path) {
                    return Err(Error::String(format!("Serial port {} is open!", path)));
                }

                let port = serialport::new(path.clone(), baud_rate)
                    .data_bits(data_bits.map(Into::into).unwrap_or(SerialDataBits::Eight))
                    .flow_control(
                        flow_control
                            .map(Into::into)
                            .unwrap_or(SerialFlowControl::None),
                    )
                    .parity(parity.map(Into::into).unwrap_or(SerialParity::None))
                    .stop_bits(stop_bits.map(Into::into).unwrap_or(SerialStopBits::One))
                    .timeout(Duration::from_millis(timeout.unwrap_or(200)))
                    .open()
                    .map_err(|e| Error::String(format!("Failed to open serial port: {}", e)))?;

                let port_info = SerialportInfo {
                    serialport: port,
                    sender: None,
                    thread_handle: None,
                };

                serialports.insert(path, port_info);
                Ok(())
            }
            Err(error) => Err(Error::String(format!("Failed to acquire lock: {}", error))),
        }
    }

    /// Read data from the serial port
    pub fn start_listening(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<(), Error> {
        println!("Starting listening on port: {}", path);

        self.get_serialport(path.clone(), |port_info| {
            if port_info.sender.is_some() {
                println!("Existing listener found, stopping it first");
                if let Some(sender) = &port_info.sender {
                    sender.send(1).map_err(|e| {
                        eprintln!("Failed to stop existing listener: {}", e);
                        Error::String(format!("Failed to stop existing listener: {}", e))
                    })?;
                }
                port_info.sender = None;

                // Wait for thread to finish
                if let Some(handle) = port_info.thread_handle.take() {
                    println!("Waiting for existing thread to finish");
                    if let Err(e) = handle.join() {
                        eprintln!("Error joining thread: {:?}", e);
                    }
                }
            }

            // Start listening immediately after opening
            let event_path = path.replace(".", "-").replace("/", "-");
            let read_event = format!("plugin-serialplugin-read-{}", &event_path);
            let disconnected_event = format!("plugin-serialplugin-disconnected-{}", &event_path);

            println!("Setting up port monitoring for: {}", read_event);

            let mut serial = port_info
                .serialport
                .try_clone()
                .map_err(|e| Error::String(format!("Failed to clone serial port: {}", e)))?;

            let (tx, rx): (Sender<usize>, Receiver<usize>) = mpsc::channel();
            port_info.sender = Some(tx);

            let app_clone = self.app.clone();
            let path_clone = path.clone();
            let thread_handle = thread::spawn(move || {
                loop {
                    match rx.try_recv() {
                        Ok(_) => break,
                        Err(TryRecvError::Disconnected) => {
                            if let Err(e) = app_clone.emit(
                                &disconnected_event,
                                format!("Serial port {} disconnected!", &path_clone),
                            ) {
                                eprintln!("Failed to send disconnection event: {}", e);
                            }
                            break;
                        }
                        Err(TryRecvError::Empty) => {}
                    }

                    let mut buffer = vec![0; size.unwrap_or(1024)];
                    match serial.read(&mut buffer) {
                        Ok(n) => {
                            if let Err(e) = app_clone.emit(
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

            Ok({})
        })
    }

    pub fn stop_listening(&self, path: String) -> Result<(), Error> {
        println!("Stopping listening on port: {}", path);

        self.get_serialport(path.clone(), |port_info| {
            if let Some(sender) = &port_info.sender {
                sender.send(1).map_err(|e| {
                    Error::String(format!("Failed to cancel serial port data reading: {}", e))
                })?;
            }
            port_info.sender = None;
            port_info.thread_handle = None;

            Ok({})
        })
    }

    /// Read data from the serial port
    pub fn read(
        &self,
        path: String,
        timeout: Option<u64>,
        size: Option<usize>,
    ) -> Result<String, Error> {
        self.get_serialport(path.clone(), |serialport_info| {
            let mut buffer = vec![0; size.unwrap_or(1024)];
            serialport_info
                .serialport
                .set_timeout(Duration::from_millis(timeout.unwrap_or(200)))
                .map_err(|e| Error::String(format!("Failed to set timeout: {}", e)))?;

            match serialport_info.serialport.read(&mut buffer) {
                Ok(n) => {
                    let data = String::from_utf8_lossy(&buffer[..n]).to_string();
                    Ok(data)
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(String::new()),
                Err(e) => Err(Error::String(format!("Failed to read data: {}", e))),
            }
        })
    }

    /// Write data to the serial port
    pub fn write(&self, path: String, value: String) -> Result<usize, Error> {
        self.get_serialport(path.clone(), |serialport_info| {
            serialport_info
                .serialport
                .write(value.as_bytes())
                .map_err(|e| Error::String(format!("Failed to write data: {}", e)))
        })
    }

    /// Write binary data to the serial port
    pub fn write_binary(&self, path: String, value: Vec<u8>) -> Result<usize, Error> {
        self.get_serialport(path.clone(), |serialport_info| {
            serialport_info
                .serialport
                .write(&value)
                .map_err(|e| Error::String(format!("Failed to write binary data: {}", e)))
        })
    }

    /// Set the baud rate
    pub fn set_baud_rate(&self, path: String, baud_rate: u32) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_baud_rate(baud_rate)
                .map_err(|e| Error::String(format!("Failed to set baud rate: {}", e)))
        })
    }

    /// Set the data bits
    pub fn set_data_bits(&self, path: String, data_bits: DataBits) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_data_bits(data_bits.into())
                .map_err(Error::from)
        })
    }

    /// Set the flow control
    pub fn set_flow_control(&self, path: String, flow_control: FlowControl) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_flow_control(flow_control.into())
                .map_err(Error::from)
        })
    }

    /// Set the parity
    pub fn set_parity(&self, path: String, parity: Parity) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_parity(parity.into())
                .map_err(Error::from)
        })
    }

    /// Set the stop bits
    pub fn set_stop_bits(&self, path: String, stop_bits: StopBits) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_stop_bits(stop_bits.into())
                .map_err(Error::from)
        })
    }

    /// Set the timeout
    pub fn set_timeout(&self, path: String, timeout: Duration) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .set_timeout(timeout)
                .map_err(Error::from)
        })
    }

    /// Set the RTS (Request To Send) control signal
    pub fn write_request_to_send(&self, path: String, level: bool) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .write_request_to_send(level)
                .map_err(Error::from)
        })
    }

    /// Set the DTR (Data Terminal Ready) control signal
    pub fn write_data_terminal_ready(&self, path: String, level: bool) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .write_data_terminal_ready(level)
                .map_err(Error::from)
        })
    }

    /// Read the CTS (Clear To Send) control signal state
    pub fn read_clear_to_send(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .read_clear_to_send()
                .map_err(Error::from)
        })
    }

    /// Read the DSR (Data Set Ready) control signal state
    pub fn read_data_set_ready(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .read_data_set_ready()
                .map_err(Error::from)
        })
    }

    /// Read the RI (Ring Indicator) control signal state
    pub fn read_ring_indicator(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .read_ring_indicator()
                .map_err(Error::from)
        })
    }

    /// Read the CD (Carrier Detect) control signal state
    pub fn read_carrier_detect(&self, path: String) -> Result<bool, Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .read_carrier_detect()
                .map_err(Error::from)
        })
    }

    /// Get the number of bytes available to read
    pub fn bytes_to_read(&self, path: String) -> Result<u32, Error> {
        self.get_serialport(path, |port_info| {
            port_info.serialport.bytes_to_read().map_err(Error::from)
        })
    }

    /// Get the number of bytes waiting to be written
    pub fn bytes_to_write(&self, path: String) -> Result<u32, Error> {
        self.get_serialport(path, |port_info| {
            port_info.serialport.bytes_to_write().map_err(Error::from)
        })
    }

    /// Clear input/output buffers
    pub fn clear_buffer(&self, path: String, buffer_to_clear: ClearBuffer) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info
                .serialport
                .clear(buffer_to_clear.into())
                .map_err(Error::from)
        })
    }

    /// Start break signal transmission
    pub fn set_break(&self, path: String) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info.serialport.set_break().map_err(Error::from)
        })
    }

    /// Stop break signal transmission
    pub fn clear_break(&self, path: String) -> Result<(), Error> {
        self.get_serialport(path, |port_info| {
            port_info.serialport.clear_break().map_err(Error::from)
        })
    }

    fn get_serialport<T, F: FnOnce(&mut SerialportInfo) -> Result<T, Error>>(
        &self,
        path: String,
        f: F,
    ) -> Result<T, Error> {
        match self.serialports.lock() {
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

    fn get_port_info(&self, port: serialport::SerialPortType) -> HashMap<String, String> {
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
}
