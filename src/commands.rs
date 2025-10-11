//! Command functions for serial port operations
//! 
//! This module contains all the Tauri command functions that can be used
//! to interact with serial ports. These functions can be imported and used
//! directly in your Rust code.
//! 
//! # Example
//! 
//! ```rust
//! use tauri_plugin_serialplugin::commands::{available_ports, open, write, read, close};
//! use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
//! use tauri::{AppHandle, State};
//! 
//! #[tauri::command]
//! async fn my_serial_function(
//!     app: AppHandle<tauri::Wry>,
//!     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
//! ) -> Result<(), String> {
//!     // Get available ports
//!     let ports = available_ports(app.clone(), serial.clone())
//!         .map_err(|e| e.to_string())?;
//!     
//!     // Open a port
//!     open(app.clone(), serial.clone(), "COM1".to_string(), 9600, None, None, None, None, None)
//!         .map_err(|e| e.to_string())?;
//!     
//!     // Write data
//!     write(app.clone(), serial.clone(), "COM1".to_string(), "Hello".to_string())
//!         .map_err(|e| e.to_string())?;
//!     
//!     // Read data
//!     let data = read(app.clone(), serial.clone(), "COM1".to_string(), Some(1000), Some(1024))
//!         .map_err(|e| e.to_string())?;
//!     
//!     // Close port
//!     close(app, serial, "COM1".to_string())
//!         .map_err(|e| e.to_string())?;
//!     
//!     Ok(())
//! }
//! ```

#[cfg(desktop)]
use crate::desktop_api::SerialPort;
use crate::error::Error;
#[cfg(mobile)]
use crate::mobile_api::SerialPort;
use crate::state::{ClearBuffer, DataBits, FlowControl, Parity, StopBits};
use std::collections::HashMap;
use std::time::Duration;
use tauri::{AppHandle, Runtime, State};

/// Lists all available serial ports on the system
/// 
/// Returns a map of port names to port information including type, manufacturer, product, etc.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// 
/// # Returns
/// 
/// A `HashMap` where keys are port names (e.g., "COM1", "/dev/ttyUSB0") and values are
/// maps containing port information like type, manufacturer, product, etc.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::available_ports;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn list_ports(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let ports = available_ports(app, serial)
///         .map_err(|e| e.to_string())?;
///     println!("Available ports: {:?}", ports);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const ports = await SerialPort.available_ports();
/// console.log("Available ports:", ports);
/// ```
#[tauri::command]
pub fn available_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports()
}

/// Lists all available serial ports using platform-specific commands
/// 
/// This function uses platform-specific system commands to detect serial ports,
/// which can provide more detailed information than the standard detection method.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// 
/// # Returns
/// 
/// A `HashMap` where keys are port names and values are maps containing
/// detailed port information obtained through platform-specific commands.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::available_ports_direct;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn list_ports_detailed(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let ports = available_ports_direct(app, serial)
///         .map_err(|e| e.to_string())?;
///     println!("Detailed port information: {:?}", ports);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const ports = await SerialPort.available_ports_direct();
/// console.log("Detailed port information:", ports);
/// ```
#[tauri::command]
pub fn available_ports_direct<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<HashMap<String, HashMap<String, String>>, Error> {
    serial.available_ports_direct()
}

/// Lists all currently managed serial ports
/// 
/// Returns a list of port names that are currently open and managed by the application.
/// These are ports that have been opened but not yet closed.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// 
/// # Returns
/// 
/// A `Vec<String>` containing the names of all currently managed ports.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::managed_ports;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn list_open_ports(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let open_ports = managed_ports(app, serial)
///         .map_err(|e| e.to_string())?;
///     println!("Currently open ports: {:?}", open_ports);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const openPorts = await SerialPort.managed_ports();
/// console.log("Currently open ports:", openPorts);
/// ```
#[tauri::command]
pub fn managed_ports<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<Vec<String>, Error> {
    serial.managed_ports()
}

/// Cancels ongoing read operations on a serial port
/// 
/// Stops any active read operations on the specified port. This is useful
/// for interrupting long-running read operations or cleaning up resources.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if the read operation was cancelled successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::cancel_read;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn stop_reading(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     cancel_read(app, serial, "COM1".to_string()).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.cancelListen();
/// ```
#[tauri::command]
pub fn cancel_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.cancel_read(path)
}

/// Closes a serial port
/// 
/// Closes the specified serial port and releases all associated resources.
/// The port must be open before it can be closed.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if the port was closed successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::close;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn close_serial_port(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     close(app, serial, "COM1".to_string()).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.close();
/// ```
#[tauri::command]
pub fn close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.close(path)
}

/// Closes all open serial ports
/// 
/// Closes all currently open serial ports and releases all associated resources.
/// This is useful for cleanup when shutting down the application.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// 
/// # Returns
/// 
/// `Ok(())` if all ports were closed successfully, or an `Error` if any failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::close_all;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn cleanup_ports(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     close_all(app, serial).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// await SerialPort.closeAll();
/// ```
#[tauri::command]
pub fn close_all<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
) -> Result<(), Error> {
    serial.close_all()
}

/// Forcefully closes a serial port
/// 
/// Forcefully closes the specified serial port, even if it's in use or has
/// active operations. This should be used as a last resort when normal
/// closing fails.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if the port was force closed successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::force_close;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn emergency_close(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     force_close(app, serial, "COM1".to_string()).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.forceClose();
/// ```
#[tauri::command]
pub fn force_close<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.force_close(path)
}

/// Opens a serial port with the specified configuration
/// 
/// Opens a serial port and configures it with the given parameters. The port must be closed
/// before it can be opened again.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `baud_rate` - The baud rate for communication (e.g., 9600, 115200)
/// * `data_bits` - Number of data bits per character (5, 6, 7, or 8)
/// * `flow_control` - Flow control mode (None, Software, or Hardware)
/// * `parity` - Parity checking mode (None, Odd, or Even)
/// * `stop_bits` - Number of stop bits (One or Two)
/// * `timeout` - Read timeout in milliseconds
/// 
/// # Returns
/// 
/// `Ok(())` if the port was opened successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::open;
/// use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn open_serial_port(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     open(
///         app,
///         serial,
///         "COM1".to_string(),
///         9600,
///         Some(DataBits::Eight),
///         Some(FlowControl::None),
///         Some(Parity::None),
///         Some(StopBits::One),
///         Some(1000)
///     ).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({
///   path: "COM1",
///   baudRate: 9600,
///   dataBits: 8,
///   flowControl: 0, // None
///   parity: 0,      // None
///   stopBits: 1
/// });
/// await port.open();
/// ```
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

/// Writes string data to a serial port
/// 
/// Sends the specified string data to the serial port. The port must be open before
/// writing data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `value` - The string data to write to the port
/// 
/// # Returns
/// 
/// The number of bytes written, or an `Error` if the operation failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::write;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn send_data(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let bytes_written = write(app, serial, "COM1".to_string(), "Hello World".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("Wrote {} bytes", bytes_written);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const bytesWritten = await port.write("Hello World");
/// console.log(`Wrote ${bytesWritten} bytes`);
/// ```
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
/// 
/// Sends the specified binary data (as a vector of bytes) to the serial port.
/// The port must be open before writing data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `value` - The binary data to write as a vector of bytes
/// 
/// # Returns
/// 
/// The number of bytes written, or an `Error` if the operation failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::write_binary;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn send_binary_data(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let binary_data = vec![0x48, 0x65, 0x6C, 0x6C, 0x6F]; // "Hello" in ASCII
///     let bytes_written = write_binary(app, serial, "COM1".to_string(), binary_data)
///         .map_err(|e| e.to_string())?;
///     println!("Wrote {} bytes of binary data", bytes_written);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const binaryData = new Uint8Array([0x48, 0x65, 0x6C, 0x6C, 0x6F]); // "Hello" in ASCII
/// const bytesWritten = await port.writeBinary(binaryData);
/// console.log(`Wrote ${bytesWritten} bytes of binary data`);
/// ```
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
/// 
/// Reads data from the serial port and returns it as a string. The port must be open
/// before reading data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `timeout` - Read timeout in milliseconds (None for no timeout)
/// * `size` - Maximum number of bytes to read (None for unlimited)
/// 
/// # Returns
/// 
/// The string data read from the port, or an `Error` if the operation failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn receive_data(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let data = read(app, serial, "COM1".to_string(), Some(1000), Some(1024))
///         .map_err(|e| e.to_string())?;
///     println!("Received: {}", data);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const data = await port.read({ timeout: 1000, size: 1024 });
/// console.log("Received:", data);
/// ```
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
/// 
/// Reads binary data from the serial port and returns it as a vector of bytes.
/// The port must be open before reading data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `timeout` - Read timeout in milliseconds (None for no timeout)
/// * `size` - Maximum number of bytes to read (None for unlimited)
/// 
/// # Returns
/// 
/// The binary data read from the port as a vector of bytes, or an `Error` if the operation failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read_binary;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn receive_binary_data(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let data = read_binary(app, serial, "COM1".to_string(), Some(1000), Some(256))
///         .map_err(|e| e.to_string())?;
///     println!("Received {} bytes: {:?}", data.len(), data);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const data = await port.readBinary({ timeout: 1000, size: 256 });
/// console.log(`Received ${data.length} bytes:`, data);
/// ```
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

/// Starts listening for data on a serial port
/// 
/// Begins continuous monitoring of the serial port for incoming data.
/// This creates a background thread that continuously reads data from the port.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `timeout` - Read timeout in milliseconds (None for no timeout)
/// * `size` - Maximum number of bytes to read per operation (None for unlimited)
/// 
/// # Returns
/// 
/// `Ok(())` if listening started successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::start_listening;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn begin_monitoring(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     start_listening(app, serial, "COM1".to_string(), Some(1000), Some(1024))
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.startListening();
/// const unsubscribe = await port.listen((data) => {
///   console.log("Received:", data);
/// });
/// ```
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

/// Stops listening for data on a serial port
/// 
/// Stops the continuous monitoring of the serial port and terminates
/// the background thread that was reading data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if listening stopped successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::stop_listening;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn end_monitoring(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     stop_listening(app, serial, "COM1".to_string()).map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.stopListening();
/// ```
#[tauri::command]
pub fn stop_listening<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.stop_listening(path)
}

/// Sets the baud rate for a serial port
/// 
/// Changes the communication speed of the serial port. Common baud rates
/// include 9600, 19200, 38400, 57600, and 115200.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `baud_rate` - The new baud rate (e.g., 9600, 115200)
/// 
/// # Returns
/// 
/// `Ok(())` if the baud rate was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_baud_rate;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn change_speed(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_baud_rate(app, serial, "COM1".to_string(), 115200)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setBaudRate(115200);
/// ```
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
/// 
/// Changes the number of data bits per character. Most modern applications
/// use 8 data bits, but some legacy systems may use 7 bits.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `data_bits` - The number of data bits (Five, Six, Seven, or Eight)
/// 
/// # Returns
/// 
/// `Ok(())` if the data bits were set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_data_bits;
/// use tauri_plugin_serialplugin::state::DataBits;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_data_bits(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_data_bits(app, serial, "COM1".to_string(), DataBits::Eight)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort, DataBits } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setDataBits(DataBits.Eight);
/// ```
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
/// 
/// Changes the flow control method used by the serial port. Flow control
/// prevents data loss by allowing the receiver to signal when it's ready
/// to receive more data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `flow_control` - The flow control mode (None, Software, or Hardware)
/// 
/// # Returns
/// 
/// `Ok(())` if the flow control was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_flow_control;
/// use tauri_plugin_serialplugin::state::FlowControl;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_flow_control(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_flow_control(app, serial, "COM1".to_string(), FlowControl::None)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort, FlowControl } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setFlowControl(FlowControl.None);
/// ```
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
/// 
/// Changes the parity checking method used by the serial port. Parity is
/// an error detection method that adds an extra bit to each character.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `parity` - The parity mode (None, Odd, or Even)
/// 
/// # Returns
/// 
/// `Ok(())` if the parity was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_parity;
/// use tauri_plugin_serialplugin::state::Parity;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_parity(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_parity(app, serial, "COM1".to_string(), Parity::None)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort, Parity } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setParity(Parity.None);
/// ```
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
/// 
/// Changes the number of stop bits used by the serial port. Stop bits
/// signal the end of a character transmission.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `stop_bits` - The number of stop bits (One or Two)
/// 
/// # Returns
/// 
/// `Ok(())` if the stop bits were set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_stop_bits;
/// use tauri_plugin_serialplugin::state::StopBits;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_stop_bits(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_stop_bits(app, serial, "COM1".to_string(), StopBits::One)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort, StopBits } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setStopBits(StopBits.One);
/// ```
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
/// 
/// Changes the timeout duration for read operations. If no data is received
/// within this timeout, the read operation will fail.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `timeout` - The timeout duration in milliseconds
/// 
/// # Returns
/// 
/// `Ok(())` if the timeout was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_timeout;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_timeout(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_timeout(app, serial, "COM1".to_string(), 5000) // 5 seconds
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setTimeout(5000); // 5 seconds
/// ```
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
/// 
/// Controls the RTS signal line on the serial port. This signal is used
/// for hardware flow control to indicate readiness to send data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `level` - The signal level (true for high, false for low)
/// 
/// # Returns
/// 
/// `Ok(())` if the RTS signal was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::write_request_to_send;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn control_rts(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     write_request_to_send(app, serial, "COM1".to_string(), true)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.writeRequestToSend(true);
/// ```
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
/// 
/// Controls the DTR signal line on the serial port. This signal indicates
/// that the terminal (computer) is ready for communication.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `level` - The signal level (true for high, false for low)
/// 
/// # Returns
/// 
/// `Ok(())` if the DTR signal was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::write_data_terminal_ready;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn control_dtr(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     write_data_terminal_ready(app, serial, "COM1".to_string(), true)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.writeDataTerminalReady(true);
/// ```
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
/// 
/// Reads the current state of the CTS signal line. This signal indicates
/// whether the remote device is ready to receive data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The CTS signal state (true for high, false for low), or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read_clear_to_send;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_cts(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let cts_state = read_clear_to_send(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("CTS signal is: {}", if cts_state { "high" } else { "low" });
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const ctsState = await port.readClearToSend();
/// console.log("CTS signal is:", ctsState ? "high" : "low");
/// ```
#[tauri::command]
pub fn read_clear_to_send<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_clear_to_send(path)
}

/// Reads the DSR (Data Set Ready) control signal state
/// 
/// Reads the current state of the DSR signal line. This signal indicates
/// whether the remote device (modem) is ready for communication.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The DSR signal state (true for high, false for low), or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read_data_set_ready;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_dsr(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let dsr_state = read_data_set_ready(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("DSR signal is: {}", if dsr_state { "high" } else { "low" });
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const dsrState = await port.readDataSetReady();
/// console.log("DSR signal is:", dsrState ? "high" : "low");
/// ```
#[tauri::command]
pub fn read_data_set_ready<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_data_set_ready(path)
}

/// Reads the RI (Ring Indicator) control signal state
/// 
/// Reads the current state of the RI signal line. This signal indicates
/// that an incoming call is being received (commonly used with modems).
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The RI signal state (true for high, false for low), or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read_ring_indicator;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_ring(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let ri_state = read_ring_indicator(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("Ring indicator is: {}", if ri_state { "active" } else { "inactive" });
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const riState = await port.readRingIndicator();
/// console.log("Ring indicator is:", riState ? "active" : "inactive");
/// ```
#[tauri::command]
pub fn read_ring_indicator<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_ring_indicator(path)
}

/// Reads the CD (Carrier Detect) control signal state
/// 
/// Reads the current state of the CD signal line. This signal indicates
/// whether a carrier signal is being received (commonly used with modems).
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The CD signal state (true for high, false for low), or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::read_carrier_detect;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_carrier(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let cd_state = read_carrier_detect(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("Carrier detect is: {}", if cd_state { "active" } else { "inactive" });
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const cdState = await port.readCarrierDetect();
/// console.log("Carrier detect is:", cdState ? "active" : "inactive");
/// ```
#[tauri::command]
pub fn read_carrier_detect<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<bool, Error> {
    serial.read_carrier_detect(path)
}

/// Gets the number of bytes available to read from the serial port
/// 
/// Returns the number of bytes that are currently available in the
/// input buffer and ready to be read.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The number of bytes available to read, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::bytes_to_read;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_available_data(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let available = bytes_to_read(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("{} bytes available to read", available);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const available = await port.bytesToRead();
/// console.log(`${available} bytes available to read`);
/// ```
#[tauri::command]
pub fn bytes_to_read<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_read(path)
}

/// Gets the number of bytes available to write to the serial port
/// 
/// Returns the number of bytes that can be written to the output
/// buffer without blocking.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// The number of bytes available to write, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::bytes_to_write;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_write_buffer(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     let available = bytes_to_write(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())?;
///     println!("{} bytes available to write", available);
///     Ok(())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// const available = await port.bytesToWrite();
/// console.log(`${available} bytes available to write`);
/// ```
#[tauri::command]
pub fn bytes_to_write<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<u32, Error> {
    serial.bytes_to_write(path)
}

/// Clears the specified buffer of the serial port
/// 
/// Clears either the input buffer, output buffer, or both buffers
/// of the serial port. This is useful for removing stale data.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// * `buffer_type` - The type of buffer to clear (Input, Output, or Both)
/// 
/// # Returns
/// 
/// `Ok(())` if the buffer was cleared successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::clear_buffer;
/// use tauri_plugin_serialplugin::state::ClearBuffer;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn clear_input_buffer(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     clear_buffer(app, serial, "COM1".to_string(), ClearBuffer::Input)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort, ClearBuffer } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.clearBuffer(ClearBuffer.Input);
/// ```
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
/// 
/// Activates the break condition, which holds the transmit line low
/// for a period longer than a character time. This is often used
/// to signal special conditions or reset devices.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if the break condition was set successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_break;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn activate_break(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     set_break(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.setBreak();
/// ```
#[tauri::command]
pub fn set_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.set_break(path)
}

/// Clears the break condition on the serial port
/// 
/// Deactivates the break condition, returning the transmit line
/// to normal operation.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `serial` - The serial port state
/// * `path` - The path to the serial port (e.g., "COM1", "/dev/ttyUSB0")
/// 
/// # Returns
/// 
/// `Ok(())` if the break condition was cleared successfully, or an `Error` if it failed.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::clear_break;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn deactivate_break(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     clear_break(app, serial, "COM1".to_string())
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";;
/// 
/// const port = new SerialPort({ path: "COM1" });
/// await port.open();
/// await port.clearBreak();
/// ```
#[tauri::command]
pub fn clear_break<R: Runtime>(
    _app: AppHandle<R>,
    serial: State<'_, SerialPort<R>>,
    path: String,
) -> Result<(), Error> {
    serial.clear_break(path)
}

/// Sets the global log level for the plugin
/// 
/// Controls how much logging output the plugin produces. Use this to reduce noise
/// in production environments or enable detailed logs for debugging.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `_serial` - The serial port state
/// * `level` - The log level to set (None, Error, Warn, Info, Debug)
/// 
/// # Returns
/// 
/// Returns `Ok(())` on success.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::set_log_level;
/// use tauri_plugin_serialplugin::state::LogLevel;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn configure_logging(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     // Set to error only to reduce noise in production
///     set_log_level(app, serial, LogLevel::Error)
///         .map_err(|e| e.to_string())
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";
/// 
/// // Disable all logs in production
/// await SerialPort.setLogLevel("None");
/// 
/// // Or show only errors
/// await SerialPort.setLogLevel("Error");
/// ```
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
/// 
/// Returns the currently configured log level for the plugin.
/// 
/// # Arguments
/// 
/// * `_app` - The Tauri app handle
/// * `_serial` - The serial port state
/// 
/// # Returns
/// 
/// Returns the current `LogLevel`.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands::get_log_level;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn check_log_level(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<String, String> {
///     let level = get_log_level(app, serial)
///         .map_err(|e| e.to_string())?;
///     Ok(format!("{:?}", level))
/// }
/// ```
/// 
/// # JavaScript Equivalent
/// 
/// ```javascript
/// import { SerialPort } from "tauri-plugin-serialplugin-api";
/// 
/// const currentLevel = await SerialPort.getLogLevel();
/// console.log("Current log level:", currentLevel);
/// ```
#[tauri::command]
pub fn get_log_level<R: Runtime>(
    _app: AppHandle<R>,
    _serial: State<'_, SerialPort<R>>,
) -> Result<crate::state::LogLevel, Error> {
    Ok(crate::state::get_log_level())
}
