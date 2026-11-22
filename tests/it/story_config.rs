//! What an app configures: the `tcp://` allowlist in tauri.conf, and the option objects of the
//! commands.

use crate::common::*;
use serde_json::{json, Value};
use serialplugin_testing::peer;
use std::net::TcpListener;
use tauri_plugin_serialplugin::error::ErrorKind;

fn assert_refused(plugin: &Plugin, listener: &TcpListener, path: &str) {
    assert!(plugin.open(path).is_err(), "{path} is not on the list");
    listener.set_nonblocking(true).unwrap();
    assert!(listener.accept().is_err(), "a refused address connected");
    assert!(plugin.managed().is_empty());
}

/// A local path is never on a TCP allowlist: opening one works, and a missing one fails as in
/// an app without an allowlist.
fn assert_local_paths_pass(plugin: &Plugin) {
    let missing = "/dev/serialplugin-no-such-port";
    let error = plugin.open(missing).expect_err("there is no such port");
    assert_eq!(Plugin::new().open(missing), Err(error));
    #[cfg(unix)]
    {
        let device = plugin.device(Kind::Pty);
        plugin.ok("close", json!({ "path": device.path }));
    }
}

/// Given no config, an empty one, or a null allowlist
/// When the webview opens a TCP bridge
/// Then it connects (every address is allowed, as in 4.x)
/// Given `tcpAllowlist: ["LocalHost:<a>"]`
/// When the webview opens `tcp://localhost:<a>`, a bridge on another port, local paths
/// Then the listed one opens (host case-insensitive), the other is refused before anything
/// connects, and local paths open as ever
/// Given an empty allowlist
/// When the webview and the app's Rust code open a bridge, and the webview a local path
/// Then the webview is refused, the Rust manager connects, and the local path opens
#[test]
fn the_allowlist_restricts_what_the_webview_connects_to() {
    for config in [json!({}), json!({ "tcpAllowlist": null })] {
        let plugin = Plugin::with_config(config).expect("init");
        plugin.device(Kind::Tcp);
    }
    Plugin::new().device(Kind::Tcp);

    let (listed, path) = peer::tcp_listener();
    let (other, other_path) = peer::tcp_listener();
    let port = path.rsplit(':').next().unwrap();
    let allow = json!({ "tcpAllowlist": [format!("LocalHost:{port}")] });
    let plugin = Plugin::with_config(allow).expect("init");
    assert_refused(&plugin, &other, &other_path);
    let path = format!("tcp://localhost:{port}");
    assert_eq!(plugin.open(&path), Ok(json!(path)));
    peer::accept(&listed);
    assert_local_paths_pass(&plugin);
    assert_eq!(plugin.managed(), [path]);

    let plugin = Plugin::with_config(json!({ "tcpAllowlist": [] })).expect("init");
    assert_refused(&plugin, &other, &other_path);
    assert_local_paths_pass(&plugin);
    plugin
        .sp()
        .open(other_path, 9600, None, None, None, None, None)
        .expect("Rust callers are not restricted");
    peer::accept(&other);
}

/// Given an allowlist that is not a list of strings
/// When the app starts
/// Then plugin setup fails (never "allow everything")
#[test]
fn a_malformed_allowlist_fails_setup() {
    for bad in [json!("10.0.0.5:4000"), json!([1]), json!({ "a": 1 })] {
        assert!(Plugin::with_config(json!({ "tcpAllowlist": bad })).is_err());
    }
}

/// Each command's option object: the arguments, where in them the object is, and the keys
/// guest-js may send in it (guest-js/types.ts).
fn option_objects() -> Vec<(&'static str, Value, &'static str, &'static [&'static str])> {
    let exchange: &[&str] = &[
        "timeoutMs",
        "maxBytes",
        "terminators",
        "idleMs",
        "rxPrepare",
        "drainIdleMs",
        "drainMaxMs",
        "completionMode",
        "resultFormat",
        "command",
        "solicitedPrefixes",
    ];
    let path = "/dev/serialplugin-no-such-port";
    let channel = "__CHANNEL__:1";
    vec![
        (
            "open",
            json!({ "path": path, "baudRate": 9600 }),
            "",
            &[
                "path",
                "baudRate",
                "dataBits",
                "flowControl",
                "parity",
                "stopBits",
                "timeout",
                "dtr",
                "rts",
            ],
        ),
        ("read", json!({ "path": path }), "", &["timeout", "size"]),
        (
            "read_binary",
            json!({ "path": path }),
            "",
            &["timeout", "size"],
        ),
        (
            "watch",
            json!({ "path": path, "options": {}, "channel": channel }),
            "/options",
            &["timeout", "size", "serialDataFlushIntervalMs", "routeUrc"],
        ),
        (
            "watch_ports",
            json!({ "options": {} }),
            "/options",
            &["singlePortPerDevice", "pollIntervalMs"],
        ),
        (
            "exchange",
            json!({ "path": path, "value": "AT\r", "options": {} }),
            "/options",
            exchange,
        ),
        (
            "exchange_binary",
            json!({ "path": path, "value": [65], "options": {} }),
            "/options",
            exchange,
        ),
        (
            "at",
            json!({ "path": path, "command": "AT", "options": {} }),
            "/options",
            &[
                "timeoutMs",
                "terminators",
                "idleMs",
                "rxPrepare",
                "completionMode",
                "resultFormat",
                "solicitedPrefixes",
                "appendCr",
            ],
        ),
        (
            "at_phases",
            json!({ "path": path, "phases": [{ "write": "AT" }] }),
            "/phases/0",
            &[
                "write",
                "completionMode",
                "resultFormat",
                "timeoutMs",
                "command",
                "rxPrepare",
            ],
        ),
        (
            "send_sms_pdu",
            json!({ "path": path, "length": 1, "pdu": [0], "options": {} }),
            "/options",
            &["timeoutMs", "resultFormat"],
        ),
        (
            "configure_at_session",
            json!({ "path": path, "session": {} }),
            "/session",
            &[
                "defaultTimeoutMs",
                "defaultTerminators",
                "defaultIdleMs",
                "defaultRxPrepare",
                "defaultSolicitedPrefixes",
                "stopOnError",
                "expectOk",
                "appendCr",
                "resultFormat",
            ],
        ),
        (
            "enable_mux",
            json!({ "path": path, "options": {} }),
            "/options",
            &["command", "timeoutMs"],
        ),
    ]
}

/// Given the options guest-js sends with each command
/// When each key carries a value of a wrong type
/// Then the command refuses its arguments: the key is read under that name
/// When a key it does not know carries the same value
/// Then it is ignored
#[test]
fn every_option_key_of_guest_js_is_read() {
    let plugin = Plugin::new();
    for (cmd, args, object, keys) in option_objects() {
        let with = |key: &str| {
            let mut args = args.clone();
            args.pointer_mut(object).unwrap()[key] = json!({ "wrong": true });
            plugin.invoke(cmd, args)
        };
        for key in keys {
            assert!(bad_args(&with(key)), "{cmd}: {key} is not read");
        }
        if cmd != "watch_ports" {
            assert!(!bad_args(&with("zzUnknownKey")), "{cmd}: unknown key");
        }
    }
    // Without a channel the arguments above never start a port-list watch; with one, they do.
    let mut ports = plugin.channel();
    plugin.ok(
        "watch_ports",
        json!({ "options": { "zzUnknownKey": { "wrong": true } }, "channel": ports.arg() }),
    );
    assert_eq!(ports.message()["kind"], "snapshot");
}

/// Given each native parser of guest-js with every key it takes
/// When a watch names it
/// Then it is accepted: only the port is missing
/// When a key is unknown, a required one is missing, or a value makes no sense
/// Then the watch is refused, whatever the port
#[test]
fn every_parser_of_guest_js_is_accepted() {
    let plugin = Plugin::new();
    let path = "/dev/serialplugin-no-such-port";
    let watch = |parser: Value| plugin.try_watch(path, json!({ "parser": parser })).1;
    for parser in [
        json!({ "type": "delimiter", "delimiter": "\n", "includeDelimiter": true, "maxBufferSize": 64 }),
        json!({ "type": "readline" }),
        json!({ "type": "readline", "delimiter": [13, 10], "includeDelimiter": false,
                "maxBufferSize": 64, "encoding": "utf8" }),
        json!({ "type": "byteLength", "length": 4 }),
        json!({ "type": "interByteTimeout", "intervalMs": 30, "maxBufferSize": 64 }),
        json!({ "type": "interByteTimeout", "interval": 30 }),
        json!({ "type": "packetLength", "delimiter": 170, "delimiterBytes": 1, "packetOverhead": 2,
                "lengthBytes": 1, "lengthOffset": 1, "maxLen": 255, "maxBufferSize": 64 }),
        json!({ "type": "packetLength", "delimiter": [1, 2], "delimiterBytes": 2, "lengthOffset": 2 }),
        json!({ "type": "ready", "delimiter": "READY" }),
        json!({ "type": "regex", "regex": "[\r\n]+", "maxBufferSize": 64, "encoding": "utf8" }),
        json!({ "type": "startEnd", "startDelimiter": "<", "endDelimiter": ">",
                "includeStartDelimiter": true, "includeEndDelimiter": true, "maxBufferSize": 64 }),
        json!({ "type": "slip", "start": 1, "esc": 2, "end": 3, "escStart": 4, "escEnd": 5, "escEsc": 6 }),
        json!({ "type": "ccTalk", "maxDelayMs": 50 }),
    ] {
        assert_eq!(kind(watch(parser.clone())), ErrorKind::NotFound, "{parser}");
    }
    for parser in [
        json!({ "type": "readline", "zzUnknownKey": 1 }),
        json!({ "type": "byteLength" }),
        json!({ "type": "noSuchParser" }),
    ] {
        assert!(bad_args(&watch(parser.clone())), "{parser}");
    }
    let empty = watch(json!({ "type": "byteLength", "length": 0 }));
    assert_eq!(kind(empty), ErrorKind::Other);
}
