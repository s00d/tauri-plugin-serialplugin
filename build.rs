// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT


const COMMANDS: &[&str] = &[
    "available_ports",
    "available_ports_direct",
    "cancel_read",
    "close",
    "close_all",
    "force_close",
    "open",
    "read",
    "start_listening",
    "stop_listening",
    "write",
    "write_binary",
    "set_baud_rate",
    "set_data_bits",
    "set_flow_control",
    "set_parity",
    "set_stop_bits",
    "set_timeout",
    "write_request_to_send",
    "write_data_terminal_ready",
    "read_clear_to_send",
    "read_data_set_ready",
    "read_ring_indicator",
    "read_carrier_detect",
    "bytes_to_read",
    "bytes_to_write",
    "clear_buffer",
    "set_break",
    "clear_break",
];

fn main() {

    tauri_plugin::Builder::new(COMMANDS)
        .global_api_script_path("./src/api-iife.js")
        .build();
}
