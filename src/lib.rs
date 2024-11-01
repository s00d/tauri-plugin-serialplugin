// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

use tauri::{
    plugin::{Builder, TauriPlugin},
    Manager, Runtime,
};

use crate::commands::*;
use crate::state::SerialportState;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

pub mod commands;
mod error;
pub mod state;

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("serialplugin")
        .js_init_script(include_str!("api-iife.js").to_string())
        .invoke_handler(tauri::generate_handler![
            available_ports,
            available_ports_direct,
            cancel_read,
            close,
            close_all,
            force_close,
            open,
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
        .setup(|app, _| {
            app.manage(SerialportState {
                serialports: Arc::new(Mutex::new(HashMap::new())),
            });
            Ok(())
        })
        .build()
}