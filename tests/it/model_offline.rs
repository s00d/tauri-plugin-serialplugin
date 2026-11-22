//! Offline model: the plugin through IPC with no device attached. Random sequences of what a
//! webview can send (unknown paths, malformed payloads, extra keys, global commands) are
//! checked against a small reference model after every step.

use crate::common::*;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde_json::{json, Value};
use serialplugin_testing::peer;
use tauri_plugin_serialplugin::error::ErrorKind;
use tauri_plugin_serialplugin::port::list_monitor;

/// Every command on a path, with valid arguments but the path. On a path no device has they
/// fail as not found, except the ones the 4.x API lets pass ([`PASSES`]).
fn path_calls() -> Vec<(&'static str, Value)> {
    vec![
        ("cancel_read", json!({})),
        ("close", json!({})),
        ("write", json!({ "value": "x" })),
        ("write_binary", json!({ "value": [1, 2] })),
        ("read", json!({ "timeout": 10, "size": 1 })),
        ("read_binary", json!({ "timeout": 10, "size": 1 })),
        ("set_baud_rate", json!({ "baudRate": 9600 })),
        ("set_data_bits", json!({ "dataBits": "Seven" })),
        ("set_flow_control", json!({ "flowControl": "None" })),
        ("set_parity", json!({ "parity": "Even" })),
        ("set_stop_bits", json!({ "stopBits": "Two" })),
        ("set_timeout", json!({ "timeout": 10 })),
        ("write_request_to_send", json!({ "level": true })),
        ("write_data_terminal_ready", json!({ "level": false })),
        ("read_clear_to_send", json!({})),
        ("read_data_set_ready", json!({})),
        ("read_ring_indicator", json!({})),
        ("read_carrier_detect", json!({})),
        ("bytes_to_write", json!({})),
        ("bytes_to_read", json!({})),
        ("clear_buffer", json!({ "bufferType": "All" })),
        ("set_break", json!({})),
        ("clear_break", json!({})),
        (
            "exchange",
            json!({ "value": "AT\r", "options": { "timeoutMs": 100, "resultFormat": "numeric" } }),
        ),
        (
            "exchange_binary",
            json!({ "value": [65, 84, 13], "options": {} }),
        ),
        ("cancel_exchange", json!({})),
        (
            "at",
            json!({ "command": "AT", "options": { "timeoutMs": 100, "appendCr": true } }),
        ),
        (
            "at_phases",
            json!({ "phases": [{ "write": "AT", "timeoutMs": 100 }] }),
        ),
        (
            "send_sms_pdu",
            json!({ "length": 1, "pdu": [0], "options": null }),
        ),
        (
            "configure_at_session",
            json!({ "session": { "defaultTimeoutMs": 100, "expectOk": true } }),
        ),
        (
            "enable_mux",
            json!({ "options": { "command": "AT+CMUX=0", "timeoutMs": 100 } }),
        ),
        ("open_mux_channel", json!({ "dlci": 1 })),
        ("disable_mux", json!({})),
        (
            "watch",
            json!({ "options": { "routeUrc": false }, "channel": "__CHANNEL__:7" }),
        ),
        ("force_close", json!({})),
    ]
}

/// What passes on an unknown path, and its result: nothing is buffered, nothing to close.
const PASSES: &[(&str, Value)] = &[("force_close", Value::Null)];

/// Payloads guest-js never sends, to commands that pass with a valid one.
fn bad_calls() -> Vec<(&'static str, Value)> {
    vec![
        ("set_log_level", json!({ "level": "Loud" })),
        ("set_log_level", json!({})),
        ("watch_ports", json!({ "options": {} })),
        ("unwatch_ports", json!({ "channelId": "seven" })),
        ("available_ports", json!({ "singlePortPerDevice": "yes" })),
        ("bytes_to_read", json!({})),
        ("force_close", json!({ "path": 5 })),
        ("unwatch_ports", json!({})),
        ("ble_scan", json!({ "timeoutMs": "soon" })),
    ]
}

#[derive(Clone, Debug)]
enum Missing {
    File,
    /// A TCP port nothing listens on any more.
    Tcp,
    /// Not an address.
    BadTcp,
}

#[derive(Clone, Debug)]
enum Action {
    Unknown {
        call: prop::sample::Index,
        extra: bool,
    },
    Bad(prop::sample::Index),
    Open {
        target: Missing,
        extra: bool,
    },
    AvailablePorts(Option<bool>),
    Capabilities,
    SetLogLevel(&'static str),
    WatchPorts {
        options: bool,
    },
    /// Subscribe again under the id of an earlier watch.
    WatchPortsAgain(usize),
    UnwatchPorts(usize),
    UnwatchUnknown,
    BleScan,
}

const LEVELS: &[&str] = &["None", "Error", "Warn", "Info", "Debug"];

fn action() -> impl Strategy<Value = Action> {
    let missing = prop_oneof![
        Just(Missing::File),
        Just(Missing::Tcp),
        Just(Missing::BadTcp)
    ];
    prop_oneof![
        5 => (any::<prop::sample::Index>(), any::<bool>())
            .prop_map(|(call, extra)| Action::Unknown { call, extra }),
        2 => any::<prop::sample::Index>().prop_map(Action::Bad),
        1 => (missing, any::<bool>()).prop_map(|(target, extra)| Action::Open { target, extra }),
        1 => any::<Option<bool>>().prop_map(Action::AvailablePorts),
        1 => Just(Action::Capabilities),
        1 => proptest::sample::select(LEVELS).prop_map(Action::SetLogLevel),
        1 => any::<bool>().prop_map(|options| Action::WatchPorts { options }),
        1 => (0usize..4).prop_map(Action::WatchPortsAgain),
        1 => (0usize..4).prop_map(Action::UnwatchPorts),
        1 => Just(Action::UnwatchUnknown),
        1 => Just(Action::BleScan),
    ]
}

struct Case {
    plugin: Plugin,
    log: &'static str,
    /// Port-list watches, and whether they are subscribed.
    watches: Vec<(Events, bool)>,
}

impl Case {
    fn new() -> Self {
        let plugin = Plugin::new();
        plugin.ok("set_log_level", json!({ "level": "Info" }));
        Self {
            plugin,
            log: "Info",
            watches: Vec::new(),
        }
    }

    fn step(&mut self, n: usize, a: &Action) {
        let path = format!("/dev/no-device-{n}");
        let call = |cmd: &str, mut args: Value, extra: &bool| {
            if *extra {
                args["zzUnknownKey"] = json!({ "nested": [1, 2] });
            }
            self.plugin.invoke(cmd, args)
        };
        match a {
            Action::Unknown { call: which, extra } => {
                let (cmd, mut args) = which.get(&path_calls()).clone();
                args["path"] = json!(path);
                let result = call(cmd, args, extra);
                if cmd == "bytes_to_read" {
                    return assert_eq!(result, Ok(json!(0)), "nothing buffered");
                }
                match PASSES.iter().find(|(c, _)| *c == cmd) {
                    Some((_, want)) => assert_eq!(result.as_ref(), Ok(want), "{cmd}"),
                    None => assert_eq!(kind(result), ErrorKind::NotFound, "{cmd}"),
                }
            }
            Action::Bad(which) => {
                let (cmd, args) = which.get(&bad_calls()).clone();
                let result = self.plugin.invoke(cmd, args);
                assert!(bad_args(&result), "{cmd}: {result:?}");
            }
            Action::Open { target, extra } => {
                let target = match target {
                    Missing::File => path,
                    Missing::Tcp => peer::tcp_listener().1,
                    Missing::BadTcp => "tcp://no-port-here".to_string(),
                };
                let args = json!({ "path": target, "baudRate": 9600, "dataBits": "Eight",
                    "flowControl": "None", "parity": "None", "stopBits": "One",
                    "timeout": 100, "dtr": true, "rts": null });
                assert!(call("open", args, extra).is_err(), "{target}");
            }
            Action::AvailablePorts(single) => {
                let args = single.map_or(json!({}), |s| json!({ "singlePortPerDevice": s }));
                let ports = self.plugin.ok("available_ports", args);
                let core = self.plugin.sp().available_ports(single.unwrap_or(false));
                assert_eq!(ports, json!(core.unwrap()));
            }
            Action::Capabilities => assert_eq!(
                self.plugin.ok("capabilities", json!({})),
                json!({
                    "platform": std::env::consts::OS,
                    "transport": "desktop",
                    "version": env!("CARGO_PKG_VERSION"),
                })
            ),
            Action::SetLogLevel(level) => {
                self.plugin.ok("set_log_level", json!({ "level": level }));
                self.log = level;
            }
            Action::WatchPorts { options } => {
                let mut events = self.plugin.channel();
                let mut args = json!({ "channel": events.arg() });
                if *options {
                    args["options"] = json!({ "pollIntervalMs": 1000 });
                }
                assert_eq!(
                    self.plugin.invoke("watch_ports", args),
                    Ok(json!(events.id)),
                    "returns the channel id"
                );
                assert_eq!(events.message()["kind"], "snapshot");
                self.watches.push((events, true));
            }
            Action::WatchPortsAgain(i) => {
                let Some((events, on)) = self.watches.get_mut(*i) else {
                    return;
                };
                let args = json!({ "channel": events.arg() });
                let result = self.plugin.invoke("watch_ports", args);
                if *on {
                    assert_eq!(kind(result), ErrorKind::Busy);
                } else {
                    // Whatever the first watch got ends at its `unwatch_ports`.
                    result.unwrap();
                    *on = true;
                }
            }
            Action::UnwatchPorts(i) => {
                let Some((events, on)) = self.watches.get_mut(*i) else {
                    return;
                };
                self.plugin
                    .ok("unwatch_ports", json!({ "channelId": events.id }));
                *on = false;
            }
            Action::UnwatchUnknown => {
                let id = u32::MAX - 3;
                let result = self.plugin.invoke("unwatch", json!({ "channelId": id }));
                assert_eq!(kind(result), ErrorKind::NotFound);
                // Not an error: the list watch may have ended on its own.
                self.plugin.ok("unwatch_ports", json!({ "channelId": id }));
            }
            Action::BleScan => {
                let result = self.plugin.invoke("ble_scan", json!({ "timeoutMs": 500 }));
                assert_eq!(result.is_ok(), cfg!(feature = "ble"), "{result:?}");
            }
        }
    }

    /// Nothing is managed; the log level and the port-list watches are the model's.
    fn check(&self, n: usize) {
        assert!(self.plugin.managed().is_empty(), "step {n}: managed ports");
        let log = self.plugin.ok("get_log_level", json!({}));
        assert_eq!(log, json!(self.log), "step {n}: log level");
        for (events, on) in &self.watches {
            let subscribed = list_monitor::is_subscribed(events.id);
            assert_eq!(subscribed, *on, "step {n}: watch {}", events.id);
        }
    }
}

#[test]
fn offline_plugin_matches_model() {
    let mut runner = TestRunner::new(Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    });
    runner
        .run(&proptest::collection::vec(action(), 1..24), |actions| {
            let mut case = Case::new();
            for (n, a) in actions.iter().enumerate() {
                case.step(n, a);
                case.check(n);
            }
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
}
