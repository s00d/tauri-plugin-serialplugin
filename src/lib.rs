// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use crate::commands::*;
use tauri::Manager;
use tauri::{
    plugin::{Builder, TauriPlugin},
    Runtime,
};

#[cfg(target_os = "android")]
const PLUGIN_IDENTIFIER: &str = "app.tauri.serialplugin";

#[cfg(target_os = "android")]
mod android_log;

#[cfg(not(target_os = "android"))]
use crate::api::SerialPort;
#[cfg(target_os = "android")]
use crate::api::SerialPort;
#[cfg(not(target_os = "android"))]
use std::collections::HashMap;
#[cfg(not(target_os = "android"))]
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
///     serial: State<'_, tauri_plugin_serialplugin::api::SerialPort<tauri::Wry>>
/// ) -> Result<String, String> {
///     commands::open(app, serial, "COM1".to_string(), 9600, None, None, None, None, None, None, None)
///         .await
///         .map_err(|e| e.to_string())
/// }
/// ```
pub mod commands;

/// Centralized logging module
///
/// Provides logging macros that respect the global log level setting.
/// Use `log_error!`, `log_warn!`, `log_info!`, and `log_debug!` macros
/// instead of direct println!/eprintln! calls.
pub mod logger;

#[cfg(any(test, target_os = "android"))]
mod jni_ready;

#[cfg(target_os = "android")]
pub mod android;
pub mod api;
pub mod at;
pub mod cmux;
pub mod error;
pub mod events;
pub mod exchange;
pub mod hub;
pub mod port;
pub mod state;
pub mod sync_util;

#[cfg(target_os = "ios")]
#[path = "serialport_ios.rs"]
pub mod serialport;

#[cfg(any(not(target_os = "android"), debug_assertions))]
pub mod mock_serial;

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
        .invoke_handler(tauri::generate_handler![
            available_ports,
            managed_ports,
            cancel_read,
            close,
            close_all,
            force_close,
            open,
            capabilities,
            watch,
            unwatch,
            watch_ports,
            unwatch_ports,
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
            set_log_level,
            get_log_level,
            exchange,
            exchange_binary,
            cancel_exchange,
            at,
            at_phases,
            send_sms_pdu,
            configure_at_session,
            enable_mux,
            open_mux_channel,
            disable_mux,
            ble_scan,
        ])
        .setup(|app, _api| {
            #[cfg(target_os = "android")]
            {
                android_log::init();
                let _handle = _api.register_android_plugin(PLUGIN_IDENTIFIER, "SerialPlugin")?;
                let serialplugin = SerialPort::<R>::new();
                serialplugin.setup_teardown();
                app.manage(serialplugin);
                crate::log_info!("serial plugin setup complete");
            }
            #[cfg(not(target_os = "android"))]
            {
                let serialplugin = SerialPort {
                    app: app.clone(),
                    serialports: Arc::new(Mutex::new(HashMap::new())),
                    virtual_ports: Arc::new(Mutex::new(HashMap::new())),
                };
                app.manage(serialplugin);
            }
            Ok(())
        })
        .on_event(|app, event| {
            // Release ports (and stop their hub threads) when the app exits.
            if let tauri::RunEvent::Exit = event {
                if let Some(serial) = app.try_state::<crate::api::SerialPort<R>>() {
                    let _ = serial.close_all();
                }
            }
        })
        .build()
}

#[cfg(test)]
mod tests {
    #[cfg(not(target_os = "android"))]
    mod acl_smoke_test;
    #[cfg(not(target_os = "android"))]
    mod available_ports_options_test;
    #[cfg(not(target_os = "android"))]
    mod command_inventory_test;
    #[cfg(not(target_os = "android"))]
    mod commands_test;
    #[cfg(not(target_os = "android"))]
    mod desktop_api_test;
    #[cfg(not(target_os = "android"))]
    mod error_test;
    #[cfg(not(target_os = "android"))]
    mod invoke_contract_test;
    #[cfg(not(target_os = "android"))]
    mod ipc_test;
    #[cfg(not(target_os = "android"))]
    mod mock_serial;
    #[cfg(not(target_os = "android"))]
    mod serial_test;
    #[cfg(not(target_os = "android"))]
    mod state_test;
    #[cfg(not(target_os = "android"))]
    mod tcp_test;
    #[cfg(not(target_os = "android"))]
    mod watch_registry_test;
}
