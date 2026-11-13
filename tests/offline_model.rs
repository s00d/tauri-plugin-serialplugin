//! Offline model: the plugin through IPC with no device attached. Random sequences of
//! what a webview can send (unknown paths, malformed payloads, extra keys, global
//! commands) are checked against a small reference model after every step.

use crate::common::*;

/// A path command on a path no device has. `ok` is the 4.x result (`bytes_to_read`
/// reports 0, `force_close` of an unknown path is no error); everything else fails.
fn unknown_call(op: usize, path: &str) -> (&'static str, Value, bool) {
    let p = path;
    match op {
        0 => ("cancel_read", json!({ "path": p }), false),
        1 => ("close", json!({ "path": p }), false),
        2 => ("write", json!({ "path": p, "value": "x" }), false),
        3 => ("write_binary", json!({ "path": p, "value": [1, 2] }), false),
        4 => (
            "read",
            json!({ "path": p, "timeout": 10, "size": 1 }),
            false,
        ),
        5 => (
            "read_binary",
            json!({ "path": p, "timeout": 10, "size": 1 }),
            false,
        ),
        6 => (
            "set_baud_rate",
            json!({ "path": p, "baudRate": 9600 }),
            false,
        ),
        7 => (
            "set_data_bits",
            json!({ "path": p, "dataBits": "Seven" }),
            false,
        ),
        8 => (
            "set_flow_control",
            json!({ "path": p, "flowControl": "None" }),
            false,
        ),
        9 => ("set_parity", json!({ "path": p, "parity": "Even" }), false),
        10 => (
            "set_stop_bits",
            json!({ "path": p, "stopBits": "Two" }),
            false,
        ),
        11 => ("set_timeout", json!({ "path": p, "timeout": 10 }), false),
        12 => (
            "write_request_to_send",
            json!({ "path": p, "level": true }),
            false,
        ),
        13 => (
            "write_data_terminal_ready",
            json!({ "path": p, "level": false }),
            false,
        ),
        14 => ("read_clear_to_send", json!({ "path": p }), false),
        15 => ("read_data_set_ready", json!({ "path": p }), false),
        16 => ("read_ring_indicator", json!({ "path": p }), false),
        17 => ("read_carrier_detect", json!({ "path": p }), false),
        18 => ("bytes_to_write", json!({ "path": p }), false),
        19 => (
            "clear_buffer",
            json!({ "path": p, "bufferType": "All" }),
            false,
        ),
        20 => ("set_break", json!({ "path": p }), false),
        21 => ("clear_break", json!({ "path": p }), false),
        22 => (
            "exchange",
            json!({ "path": p, "value": "AT\r", "options": { "timeoutMs": 100, "resultFormat": "numeric" } }),
            false,
        ),
        23 => (
            "exchange_binary",
            json!({ "path": p, "value": [65, 84, 13], "options": {} }),
            false,
        ),
        24 => ("cancel_exchange", json!({ "path": p }), false),
        25 => (
            "at",
            json!({ "path": p, "command": "AT", "options": { "timeoutMs": 100, "appendCr": true } }),
            false,
        ),
        26 => (
            "at_phases",
            json!({ "path": p, "phases": [{ "write": "AT", "timeoutMs": 100 }] }),
            false,
        ),
        27 => (
            "send_sms_pdu",
            json!({ "path": p, "length": 1, "pdu": [0], "options": null }),
            false,
        ),
        28 => (
            "configure_at_session",
            json!({ "path": p, "session": { "defaultTimeoutMs": 100, "expectOk": true } }),
            false,
        ),
        29 => (
            "enable_mux",
            json!({ "path": p, "options": { "command": "AT+CMUX=0", "timeoutMs": 100 } }),
            false,
        ),
        30 => ("open_mux_channel", json!({ "path": p, "dlci": 1 }), false),
        31 => ("disable_mux", json!({ "path": p }), false),
        32 => {
            let (_, channel) = ipc_channel();
            (
                "watch",
                json!({ "path": p, "options": { "routeUrc": false }, "channel": channel }),
                false,
            )
        }
        33 => ("bytes_to_read", json!({ "path": p }), true),
        _ => ("force_close", json!({ "path": p }), true),
    }
}
const UNKNOWN_OPS: usize = 35;

/// Payloads guest-js never sends to commands that succeed with a valid one.
fn bad_call(case: usize) -> (&'static str, Value) {
    match case {
        0 => ("set_log_level", json!({ "level": "Loud" })),
        1 => ("set_log_level", json!({})),
        2 => ("watch_ports", json!({ "options": {} })),
        3 => ("unwatch_ports", json!({ "channelId": "seven" })),
        4 => ("available_ports", json!({ "singlePortPerDevice": "yes" })),
        5 => ("bytes_to_read", json!({})),
        6 => ("force_close", json!({ "path": 5 })),
        _ => ("unwatch_ports", json!({})),
    }
}
const BAD_CALLS: usize = 8;

#[derive(Clone, Debug)]
enum Action {
    Unknown {
        op: usize,
        extra: bool,
    },
    Bad(usize),
    /// `open` of something that is no device: 0 missing file, 1 refused tcp, 2 bad tcp.
    OpenMissing {
        which: u8,
        extra: bool,
    },
    AvailablePorts(Option<bool>),
    Capabilities,
    SetLogLevel(&'static str),
    WatchPorts {
        options: bool,
    },
    UnwatchPorts(usize),
    UnwatchUnknown,
    BleScan,
}

const LEVELS: [&str; 5] = ["None", "Error", "Warn", "Info", "Debug"];

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        4 => (0..UNKNOWN_OPS, any::<bool>()).prop_map(|(op, extra)| Action::Unknown { op, extra }),
        2 => (0..BAD_CALLS).prop_map(Action::Bad),
        1 => (0u8..3, any::<bool>()).prop_map(|(which, extra)| Action::OpenMissing { which, extra }),
        1 => proptest::option::of(any::<bool>()).prop_map(Action::AvailablePorts),
        1 => Just(Action::Capabilities),
        1 => proptest::sample::select(LEVELS.to_vec()).prop_map(Action::SetLogLevel),
        1 => any::<bool>().prop_map(|options| Action::WatchPorts { options }),
        1 => (0usize..4).prop_map(Action::UnwatchPorts),
        1 => Just(Action::UnwatchUnknown),
        1 => Just(Action::BleScan),
    ]
}

#[derive(Debug)]
struct Model {
    log: &'static str,
    /// Port-list watches, by creation order: still subscribed?
    list_watches: Vec<bool>,
}

impl Model {
    /// Expected `Ok`/`Err`, or `None` when the action means nothing in this state.
    fn step(&mut self, a: &Action) -> Option<bool> {
        Some(match a {
            Action::Unknown { op, .. } => unknown_call(*op, "").2,
            Action::Bad(_) | Action::OpenMissing { .. } | Action::UnwatchUnknown => false,
            Action::AvailablePorts(_) | Action::Capabilities => true,
            Action::SetLogLevel(level) => {
                self.log = level;
                true
            }
            Action::WatchPorts { .. } => {
                self.list_watches.push(true);
                true
            }
            Action::UnwatchPorts(i) => {
                *self.list_watches.get_mut(*i)? = false;
                true
            }
            Action::BleScan => cfg!(feature = "ble"),
        })
    }
}

static CASE: AtomicUsize = AtomicUsize::new(0);

fn with_extra(mut args: Value, extra: bool) -> Value {
    if extra {
        args["zzUnknownKey"] = json!({ "nested": [1, 2] });
    }
    args
}

fn run_case(actions: &[Action]) {
    let case = CASE.fetch_add(1, Ordering::Relaxed);
    let plugin = Plugin::new();
    plugin
        .invoke("set_log_level", json!({ "level": "Info" }))
        .unwrap();
    let mut model = Model {
        log: "Info",
        list_watches: Vec::new(),
    };
    let mut list_ids = Vec::new();
    for (n, a) in actions.iter().enumerate() {
        let Some(expect_ok) = model.step(a) else {
            continue;
        };
        let path = format!("/dev/case{case}_missing{n}");
        let res = match a {
            Action::Unknown { op, extra } => {
                let (cmd, args, _) = unknown_call(*op, &path);
                let res = plugin.invoke(cmd, with_extra(args, *extra));
                if cmd == "bytes_to_read" {
                    assert_eq!(res, Ok(json!(0)), "{a:?}: nothing buffered");
                }
                res
            }
            Action::Bad(case) => {
                let (cmd, args) = bad_call(*case);
                plugin.invoke(cmd, args)
            }
            Action::OpenMissing { which, extra } => {
                let target = match which {
                    0 => path.clone(),
                    1 => {
                        // A port nothing listens on any more.
                        let (listener, path) = tcp_listener();
                        drop(listener);
                        path
                    }
                    _ => "tcp://no-port-here".to_string(),
                };
                let args = json!({ "path": target, "baudRate": 9600, "dataBits": "Eight",
                    "flowControl": "None", "parity": "None", "stopBits": "One",
                    "timeout": 100, "dtr": true, "rts": null });
                plugin.invoke("open", with_extra(args, *extra))
            }
            Action::AvailablePorts(single) => {
                let args = single.map_or(json!({}), |s| json!({ "singlePortPerDevice": s }));
                let res = plugin.invoke("available_ports", args);
                if single != &Some(true) {
                    let core = plugin.sp().core().available_ports(false).unwrap();
                    assert_eq!(res.as_ref().unwrap(), &serde_json::to_value(core).unwrap());
                }
                res
            }
            Action::Capabilities => {
                let res = plugin.invoke("capabilities", json!({}));
                let caps = res.as_ref().expect("capabilities");
                assert_eq!(caps["platform"], std::env::consts::OS);
                assert_eq!(caps["transport"], "desktop");
                assert_eq!(caps["version"], env!("CARGO_PKG_VERSION"));
                res
            }
            Action::SetLogLevel(level) => plugin.invoke("set_log_level", json!({ "level": level })),
            Action::WatchPorts { options } => {
                let (id, channel) = ipc_channel();
                let args = if *options {
                    json!({ "options": { "pollIntervalMs": 1000 }, "channel": channel })
                } else {
                    json!({ "channel": channel })
                };
                let res = plugin.invoke("watch_ports", args);
                assert_eq!(res, Ok(json!(id)), "{a:?}: returns the channel id");
                list_ids.push(id);
                res
            }
            Action::UnwatchPorts(i) => {
                plugin.invoke("unwatch_ports", json!({ "channelId": list_ids[*i] }))
            }
            Action::UnwatchUnknown => {
                plugin.invoke("unwatch", json!({ "channelId": u32::MAX - 3 }))
            }
            Action::BleScan => plugin.invoke("ble_scan", json!({ "timeoutMs": 500 })),
        };
        assert_eq!(res.is_ok(), expect_ok, "step {n} {a:?}: got {res:?}");

        // Invariants: nothing managed, the log level, the port-list subscriptions.
        assert!(plugin.managed().is_empty(), "step {n}: managed ports");
        assert_eq!(
            plugin.invoke("get_log_level", json!({})),
            Ok(json!(model.log)),
            "step {n}: log level"
        );
        for (i, on) in model.list_watches.iter().enumerate() {
            assert_eq!(
                tauri_plugin_serialplugin::port::list_monitor::is_subscribed(list_ids[i]),
                *on,
                "step {n}: port-list watch {i}"
            );
        }
    }
    for id in list_ids {
        let _ = plugin.invoke("unwatch_ports", json!({ "channelId": id }));
    }
}

#[test]
fn offline_plugin_matches_model() {
    let mut runner = TestRunner::new(Config {
        cases: 48,
        failure_persistence: None,
        max_shrink_time: 3_000,
        ..Config::default()
    });
    let result = runner.run(&proptest::collection::vec(action(), 1..24), |actions| {
        run_case(&actions);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}
