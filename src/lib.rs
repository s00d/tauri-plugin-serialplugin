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

mod commands;

#[cfg(desktop)]
mod desktop_api;
mod error;
#[cfg(mobile)]
mod mobile_api;
pub mod state;

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
