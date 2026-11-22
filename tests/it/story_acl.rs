//! Permissions: what an app's capability lets its webview call.

use crate::common::*;
use serde_json::{json, Value};
use serialplugin_testing::peer;
use std::collections::BTreeSet;

/// The commands of the plugin: the 4.x contract with guest-js and app capabilities.
const COMMANDS: &[&str] = &[
    "available_ports",
    "managed_ports",
    "cancel_read",
    "close",
    "close_all",
    "force_close",
    "open",
    "capabilities",
    "watch",
    "unwatch",
    "watch_ports",
    "unwatch_ports",
    "read",
    "read_binary",
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

fn allow(command: &str) -> String {
    format!("allow-{}", command.replace('_', "-"))
}

/// Call `command` with arguments it cannot read (the few without arguments run): reached and
/// registered, or refused by the ACL. Anything else fails.
fn reached(plugin: &Plugin, command: &str) -> bool {
    let result = plugin.invoke(
        command,
        json!({ "singlePortPerDevice": {}, "timeoutMs": {} }),
    );
    if refused(&result) {
        return false;
    }
    assert!(
        result.is_ok() || bad_args(&result),
        "{command} is not registered: {result:?}"
    );
    true
}

fn reached_set(plugin: &Plugin) -> BTreeSet<&'static str> {
    let commands = COMMANDS.iter().copied();
    commands.filter(|c| reached(plugin, c)).collect()
}

fn all() -> BTreeSet<&'static str> {
    COMMANDS.iter().copied().collect()
}

/// Given the plugin's permission files and guest-js
/// When an app grants `serialplugin:default`
/// Then default lists `allow-*` of every command and nothing else, the webview reaches exactly
/// these, and guest-js calls exactly these (`read` is for Rust callers)
/// When the app also lists `serialplugin:deny-open`
/// Then only `open` is refused
#[test]
fn the_default_set_grants_exactly_what_it_lists() {
    let manifest = manifest();
    let listed = manifest.default_permission.expect("default").permissions;
    let want: BTreeSet<String> = COMMANDS.iter().map(|c| allow(c)).collect();
    assert_eq!(BTreeSet::from_iter(listed.iter().cloned()), want);
    assert_eq!(listed.len(), want.len(), "listed twice");
    // Each command has an allow and a deny permission, and no other exists.
    let deny = |p: &String| p.replacen("allow", "deny", 1);
    let both: BTreeSet<String> = want.iter().cloned().chain(want.iter().map(deny)).collect();
    let defined: BTreeSet<String> = manifest.permissions.into_keys().collect();
    assert_eq!(defined, both);

    assert_eq!(reached_set(&Plugin::new()), all());
    let denied = Plugin::with_permissions(&["serialplugin:default", "serialplugin:deny-open"]);
    let mut without_open = all();
    without_open.remove("open");
    assert_eq!(reached_set(&denied), without_open);

    let js = std::fs::read_to_string(repo_file("guest-js/serial-port.ts")).unwrap();
    let mut called: BTreeSet<&str> = js
        .split("plugin:serialplugin|")
        .skip(1)
        .map(|rest| rest.split('\'').next().unwrap())
        .collect();
    assert!(called.insert("read"), "guest-js reads bytes");
    assert_eq!(called, all());
}

/// Given an app that grants one `allow-*` permission, for each of them
/// When its webview calls every command
/// Then only the granted one is reached; with no permission, none
#[test]
fn a_single_permission_grants_one_command() {
    assert_eq!(reached_set(&Plugin::with_permissions(&[])), BTreeSet::new());
    for command in COMMANDS {
        let permission = format!("serialplugin:{}", allow(command));
        let plugin = Plugin::with_permissions(&[&permission]);
        assert_eq!(reached_set(&plugin), BTreeSet::from([*command]));
    }
}

/// Given a live TCP bridge and an app with every permission but `allow-open`
/// When its webview opens the bridge
/// Then the call is refused, nothing connects, nothing is managed
/// When the example app's capability is granted instead
/// Then the bridge opens
#[test]
fn an_app_without_allow_open_connects_to_nothing() {
    let (listener, path) = peer::tcp_listener();
    let permissions: Vec<String> = COMMANDS
        .iter()
        .filter(|c| **c != "open")
        .map(|c| format!("serialplugin:{}", allow(c)))
        .collect();
    let permissions: Vec<&str> = permissions.iter().map(String::as_str).collect();
    let plugin = Plugin::with_permissions(&permissions);
    assert!(refused(&plugin.open(&path)));
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "a refused open connected");
    assert!(plugin.managed().is_empty());

    let example = repo_file("examples/serialport-test/src-tauri/capabilities/default.json");
    let example: Value = serde_json::from_str(&std::fs::read_to_string(example).unwrap()).unwrap();
    let permissions: Vec<&str> = example["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|p| p.starts_with("serialplugin:"))
        .collect();
    let plugin = Plugin::with_permissions(&permissions);
    assert_eq!(plugin.open(&path), Ok(json!(path)));
    peer::accept(&listener);
}
