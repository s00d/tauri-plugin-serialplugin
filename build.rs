// Copyright 2019-2023 Tauri Programme within The Commons Conservancy
// SPDX-License-Identifier: Apache-2.0
// SPDX-License-Identifier: MIT

const COMMANDS: &[&str] = &[
    "available_ports",
    "managed_ports",
    "cancel_read",
    "close",
    "close_all",
    "force_close",
    "open",
    "read",
    "read_binary",
    "capabilities",
    "watch",
    "unwatch",
    "watch_ports",
    "unwatch_ports",
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
    "set_log_level",
    "get_log_level",
    "exchange",
    "exchange_binary",
    "cancel_exchange",
    "at",
    "at_phases",
    "send_sms_pdu",
    "configure_at_session",
    "enable_mux",
    "open_mux_channel",
    "disable_mux",
];

fn main() {
    let result = tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build();

    // when building documentation for Android the plugin build result is always Err() and is irrelevant to the crate documentation build
    if !(cfg!(docsrs) && std::env::var("TARGET").unwrap().contains("android")) {
        result.unwrap();
    }

    tauri_plugin::mobile::update_android_manifest(
        "SERIAL PLUGIN",
        "activity",
        r#"<intent-filter>
            <action android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED" />
        </intent-filter>
        <meta-data
            android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED"
            android:resource="@xml/device_filter" />"#
            .to_string(),
    )
    .expect("failed to update AndroidManifest.xml");
}
