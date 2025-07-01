// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::commands::*;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.serialplugin";

#[cfg(desktop)]
use crate::desktop_api::SerialPort;
#[cfg(target_os = "android")]
use crate::mobile_api::SerialPort;
#[cfg(desktop)]
use std::collections::HashMap;
#[cfg(desktop)]
use std::sync::{Arc, Mutex};

/// Commands module providing Tauri commands for serial port operations
/// 
/// This module contains all the Tauri commands that can be invoked from the frontend
/// to interact with serial ports. Each command is designed to be used with the
/// `tauri::invoke` function or through the plugin's JavaScript API.
/// 
/// # Examples
/// 
/// ```rust
/// use tauri_plugin_serialplugin::commands;
/// use tauri::{AppHandle, State};
/// 
/// #[tauri::command]
/// async fn open_serial_port(
///     app: AppHandle<tauri::Wry>,
///     serial: State<'_, tauri_plugin_serialplugin::desktop_api::SerialPort<tauri::Wry>>
/// ) -> Result<(), String> {
///     commands::open(app, serial, "COM1".to_string(), 9600, None, None, None, None, None)
///         .map_err(|e| e.to_string())
/// }
/// ```
pub mod commands;
#[cfg(test)]
mod tests {
    mod mock;
    mod commands_test;
    mod state_test;
    mod error_test;
    mod desktop_api_test;
    mod mobile_api_test;
    mod serial_test;
}

#[cfg(desktop)]
/// Desktop API module providing serial port functionality for desktop platforms
/// 
/// This module contains the desktop-specific implementation of serial port
/// operations. It provides a unified interface for managing serial ports
/// across different desktop operating systems (Windows, macOS, Linux).
/// 
/// # Examples
/// 
/// ```rust
/// use tauri_plugin_serialplugin::desktop_api::SerialPort;
/// use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
/// use tauri::AppHandle;
/// use std::time::Duration;
/// 
/// // Note: In a real Tauri app, you would get the AppHandle from the command context
/// // let serial = SerialPort::new(app_handle);
/// // serial.open("COM1".to_string(), 9600, Some(DataBits::Eight), 
/// //             Some(FlowControl::None), Some(Parity::None), 
/// //             Some(StopBits::One), Some(1000))
/// //             .expect("Failed to open port");
/// ```
pub mod desktop_api;
/// Error types for serial port operations
/// 
/// This module defines the error types used throughout the serial plugin.
/// It provides a unified error handling interface for both desktop and
/// mobile platforms.
/// 
/// # Examples
/// 
/// ```rust
/// use tauri_plugin_serialplugin::error::Error;
/// 
/// // Example of error handling
/// fn handle_operation_result(result: Result<(), Error>) {
///     match result {
///         Ok(_) => println!("Operation successful"),
///         Err(Error::Io(msg)) => println!("IO error: {}", msg),
///         Err(Error::SerialPort(msg)) => println!("Serial port error: {}", msg),
///         Err(Error::String(msg)) => println!("Error: {}", msg),
///     }
/// }
/// ```
pub mod error;
#[cfg(mobile)]
/// Mobile API module providing serial port functionality for mobile platforms
/// 
/// This module contains the mobile-specific implementation of serial port
/// operations. It provides a unified interface for managing serial ports
/// on Android devices.
/// 
/// # Examples
/// 
/// ```rust
/// use tauri_plugin_serialplugin::mobile_api::SerialPort;
/// use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
/// use tauri::AppHandle;
/// use std::time::Duration;
/// 
/// // Note: In a real Tauri app, you would get the AppHandle from the command context
/// // let serial = SerialPort::new(app_handle);
/// // serial.open("/dev/ttyUSB0".to_string(), 9600, Some(DataBits::Eight), 
/// //             Some(FlowControl::None), Some(Parity::None), 
/// //             Some(StopBits::One), Some(1000))
/// //             .expect("Failed to open port");
/// ```
pub mod mobile_api;
/// State types and enums for serial port configuration
/// 
/// This module defines the data structures and enums used for serial port
/// configuration. It includes types for baud rates, data bits, flow control,
/// parity, stop bits, and other serial port settings.
/// 
/// # Examples
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits};
/// 
/// // Configure serial port settings
/// let data_bits = DataBits::Eight;
/// let flow_control = FlowControl::None;
/// let parity = Parity::None;
/// let stop_bits = StopBits::One;
/// ```
pub mod state;

/// Initializes the serial plugin for Tauri
/// 
/// This function creates and configures the serial plugin with all available
/// commands for serial port operations. It sets up the necessary state management
/// and registers the plugin with the Tauri application.
/// 
/// # Returns
/// 
/// A configured `TauriPlugin` instance that can be added to your Tauri app.
/// 
/// # Example
/// 
/// ```rust,ignore
/// use tauri_plugin_serialplugin::init;
/// 
/// fn main() {
///     tauri::Builder::default()
///         .plugin(init())
///         // .run(tauri::generate_context!())
///         // .expect("error while running tauri application");
/// }
/// ```
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("serialplugin")
        .js_init_script(include_str!("api-iife.js").to_string())
        .invoke_handler(tauri::generate_handler![
            available_ports,
            available_ports_direct,
            managed_ports,
            cancel_read,
            close,
            close_all,
            force_close,
            open,
            start_listening,
            stop_listening,
            read,
            read_binary,
            write,
            write_binary,
            set_baud_rate,
            set_data_bits,
            set_flow_control,
            set_parity,
            set_stop_bits,
            set_timeout,
            write_request_to_send,
            write_data_terminal_ready,
            read_clear_to_send,
            read_data_set_ready,
            read_ring_indicator,
            read_carrier_detect,
            bytes_to_read,
            bytes_to_write,
            clear_buffer,
            set_break,
            clear_break,
        ])
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            let handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "SerialPlugin")?;
            #[cfg(target_os = "android")]
            let serialplugin = SerialPort(handle);
            // app.manage(SerialPort(handle));
            #[cfg(desktop)]
            let serialplugin = SerialPort {
                app: app.clone(),
                serialports: Arc::new(Mutex::new(HashMap::new())),
            };

            app.manage(serialplugin);
            Ok(())
        })
        .build()
}
