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
    "ble_scan",
];

/// Marker of the BLE block in the app's `gen/android/app/src/main/AndroidManifest.xml`.
const BLE_BLOCK: &str = "SERIALPLUGIN BLE";
const BLE_MARKER: &str = "<!-- SERIALPLUGIN BLE. AUTO-GENERATED. DO NOT REMOVE. -->";
/// What `ble://` needs on Android (see docs/content/4.platforms/6.ble.md).
const BLE_PERMISSIONS: &[(&str, &str)] = &[
    (
        "android.permission.BLUETOOTH",
        r#" android:maxSdkVersion="30""#,
    ),
    (
        "android.permission.BLUETOOTH_ADMIN",
        r#" android:maxSdkVersion="30""#,
    ),
    // API <= 30: BLE scan results need location.
    (
        "android.permission.ACCESS_FINE_LOCATION",
        r#" android:maxSdkVersion="30""#,
    ),
    // API 31+: "Nearby devices"; scans are not used to derive location.
    (
        "android.permission.BLUETOOTH_SCAN",
        r#" android:usesPermissionFlags="neverForLocation""#,
    ),
    ("android.permission.BLUETOOTH_CONNECT", ""),
];

/// The BLE permissions are not in the library manifest: the merger would fold their
/// `maxSdkVersion` / `usesPermissionFlags` into the app's own declarations of the same
/// permission, for every app, `ble` or not. Instead they go into a marked block of the
/// app's manifest, only with the `ble` feature, and only the ones the app does not
/// declare itself (the app's own line then stays exactly as written; a second line for
/// the same permission in one manifest would fail the merge).
fn update_ble_permissions() {
    println!("cargo:rerun-if-env-changed=TAURI_ANDROID_PROJECT_PATH");
    let Some(project) = std::env::var_os("TAURI_ANDROID_PROJECT_PATH") else {
        return;
    };
    let path = std::path::Path::new(&project).join("app/src/main/AndroidManifest.xml");
    let Ok(manifest) = std::fs::read_to_string(&path) else {
        return;
    };
    println!("cargo:rerun-if-changed={}", path.display());
    let enabled = std::env::var_os("CARGO_FEATURE_BLE").is_some();
    if !enabled && !manifest.contains(BLE_MARKER) {
        return; // never touch the manifest of an app that does not use BLE
    }
    let own = app_own_declarations(&manifest);
    let block = if enabled {
        BLE_PERMISSIONS
            .iter()
            .filter(|(name, _)| {
                !own.contains(&format!("\"{name}\"")) && !own.contains(&format!("'{name}'"))
            })
            .map(|(name, attrs)| format!(r#"<uses-permission android:name="{name}"{attrs} />"#))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        String::new()
    };
    tauri_plugin::mobile::update_android_manifest(BLE_BLOCK, "manifest", block)
        .expect("failed to update AndroidManifest.xml (BLE permissions)");
}

/// The manifest without our own block and without XML comments.
fn app_own_declarations(manifest: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in manifest.lines() {
        if line.contains(BLE_MARKER) {
            inside = !inside;
        } else if !inside {
            out.push_str(line);
            out.push('\n');
        }
    }
    while let Some(start) = out.find("<!--") {
        let end = out[start..]
            .find("-->")
            .map_or(out.len(), |e| start + e + 3);
        out.replace_range(start..end, "");
    }
    out
}

fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    if target.contains("android") {
        println!("cargo:rustc-link-lib=log");
    }

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

    update_ble_permissions();

    // Test binaries link tauri, which needs Common-Controls v6 on Windows:
    // without this manifest they fail to start (STATUS_ENTRYPOINT_NOT_FOUND).
    // Same workaround as tauri's own build.rs. Link args of a build script
    // apply only to this crate's own linked targets (it is an rlib, so its
    // test binaries), never to apps that depend on it.
    let windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc");
    if windows && msvc {
        let manifest =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("windows-test-manifest.xml");
        println!("cargo:rerun-if-changed={}", manifest.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    }
}
