// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

const COMMANDS: &[&str] = &["available_ports", "cancel_read", "close", "close_all", "force_close", "open", "read", "write", "write_binary"];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).build();
}
