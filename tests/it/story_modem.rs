//! A real AT modem on a serial port, through IPC as a webview calls the plugin. Only queries:
//! no SMS, USSD, calls or settings writes.
//!
//! Run with the modem's path (and baud rate, default 115200):
//! `SERIALPLUGIN_MODEM=/dev/cu.wchusbserial210 cargo nextest run story_modem`.
//! Without `SERIALPLUGIN_MODEM` the test prints SKIP and passes.
//!
//! One `#[test]`: the stories share one physical port, and nextest runs each test in its own
//! process, so they cannot run side by side.

use crate::common::*;
use serde_json::{json, Value};
use serialplugin_testing::eventually;
use tauri_plugin_serialplugin::at::parse::{AtCommandResult, AtParseStatus, ExchangeResponse};

fn open_modem(plugin: &Plugin, path: &str, baud: u32) {
    plugin.ok(
        "open",
        json!({ "path": path, "baudRate": baud, "timeout": 200 }),
    );
}

/// Replies are parsed into the plugin's own result types: a field name typo does not compile,
/// and the JSON shape the webview gets is checked on the way.
fn typed<T: serde::de::DeserializeOwned>(v: Value) -> T {
    serde_json::from_value(v).expect("reply shape")
}

fn at(plugin: &Plugin, path: &str, command: &str) -> AtCommandResult {
    let options = json!({ "timeoutMs": ANSWERED_MS });
    typed(plugin.ok(
        "at",
        json!({ "path": path, "command": command, "options": options }),
    ))
}

/// Lines of `lines` starting with `prefix`.
fn with_prefix(lines: &[String], prefix: &str) -> usize {
    lines.iter().filter(|l| l.starts_with(prefix)).count()
}

#[test]
fn a_real_modem_answers_queries() {
    let Ok(path) = std::env::var("SERIALPLUGIN_MODEM") else {
        eprintln!("SKIP a_real_modem_answers_queries: SERIALPLUGIN_MODEM is not set");
        return;
    };
    let baud: u32 = std::env::var("SERIALPLUGIN_MODEM_BAUD")
        .ok()
        .and_then(|b| b.parse().ok())
        .unwrap_or(115_200);
    let plugin = Plugin::new();

    port_is_listed(&plugin, &path);
    open_modem(&plugin, &path, baud);
    queries_parse_into_solicited_lines(&plugin, &path);
    an_unknown_command_is_an_error_not_a_timeout(&plugin, &path);
    phases_run_in_order(&plugin, &path);
    a_watch_sees_raw_replies_and_exchange_still_works(&plugin, &path);
    close_releases_the_port(&plugin, &path, baud);
    cmux_channels_answer_and_close_returns_the_modem(&plugin, &path, baud);
}

/// Given a modem plugged in
/// When the app lists ports
/// Then its path is among them
fn port_is_listed(plugin: &Plugin, path: &str) {
    let ports = plugin
        .invoke("available_ports", json!({}))
        .expect("available_ports");
    assert!(ports.get(path).is_some(), "{path} not in {ports}");
}

/// Given an open modem (echo on or off)
/// When the app sends `ATI`, `AT+CPIN?`, `AT+CSQ`, `AT+CREG?`
/// Then each ends OK, not timed out, and the `+CMD:` lines land in `solicitedBody`
fn queries_parse_into_solicited_lines(plugin: &Plugin, path: &str) {
    let ati = at(plugin, path, "ATI");
    assert_eq!(ati.status, AtParseStatus::Ok, "{ati:?}");
    assert!(!ati.timed_out);
    assert!(!ati.solicited_body.is_empty(), "{ati:?}");
    for (cmd, prefix) in [
        ("AT+CPIN?", "+CPIN:"),
        ("AT+CSQ", "+CSQ:"),
        ("AT+CREG?", "+CREG:"),
    ] {
        let res = at(plugin, path, cmd);
        assert_eq!(res.status, AtParseStatus::Ok, "{cmd}: {res:?}");
        assert_eq!(
            with_prefix(&res.solicited_body, prefix),
            1,
            "{cmd}: {res:?}"
        );
    }
}

/// Given an open modem
/// When the app sends a command the modem does not know
/// Then the result is an error status at once, not a timeout
fn an_unknown_command_is_an_error_not_a_timeout(plugin: &Plugin, path: &str) {
    let res = at(plugin, path, "AT+NOSUCHCMD");
    assert!(
        matches!(res.status, AtParseStatus::Error | AtParseStatus::Cme),
        "{res:?}"
    );
    assert!(!res.timed_out, "{res:?}");
    // The modem still answers after the error.
    assert_eq!(at(plugin, path, "AT").status, AtParseStatus::Ok);
}

/// Given an open modem
/// When `at_phases` runs `AT` then `AT+CSQ`
/// Then both results come back OK in order
fn phases_run_in_order(plugin: &Plugin, path: &str) {
    let res: Vec<AtCommandResult> = typed(
        plugin
            .invoke(
                "at_phases",
                json!({ "path": path, "phases": [
                { "write": "AT\r", "command": "AT", "timeoutMs": ANSWERED_MS },
                { "write": "AT+CSQ\r", "command": "AT+CSQ", "timeoutMs": ANSWERED_MS },
            ] }),
            )
            .expect("at_phases"),
    );
    assert_eq!(res.len(), 2);
    assert_eq!(res[0].command, "AT");
    assert_eq!(res[1].command, "AT+CSQ");
    assert!(res.iter().all(|r| r.status == AtParseStatus::Ok), "{res:?}");
}

/// Given a watch on the open modem
/// When the app writes `AT\r` with `write`
/// Then the watch gets the raw `OK`
/// When an `exchange` of `AT+CSQ` runs while the watch is attached
/// Then the exchange gets its reply and ends OK
fn a_watch_sees_raw_replies_and_exchange_still_works(plugin: &Plugin, path: &str) {
    let mut events = plugin.watch(path, json!({}));
    assert_eq!(
        plugin.ok("write", json!({ "path": path, "value": "AT\r" })),
        3
    );
    events.data_until(path, |b| b.windows(4).any(|w| w == b"OK\r\n"));

    let options = json!({ "timeoutMs": ANSWERED_MS });
    let res: ExchangeResponse = typed(plugin.ok(
        "exchange",
        json!({ "path": path, "value": "AT+CSQ\r", "options": options }),
    ));
    assert_eq!(res.status, AtParseStatus::Ok, "{res:?}");
    assert_eq!(with_prefix(&res.lines, "+CSQ:"), 1, "{res:?}");
    plugin.unwatch(&events);
}

/// Given an open modem
/// When the app closes it and opens it again
/// Then the path is released in between and the modem answers after the reopen
fn close_releases_the_port(plugin: &Plugin, path: &str, baud: u32) {
    plugin
        .invoke("close", json!({ "path": path }))
        .expect("close");
    assert!(plugin.managed().is_empty());
    open_modem(plugin, path, baud);
    assert_eq!(at(plugin, path, "AT").status, AtParseStatus::Ok);
}

/// Given an open modem
/// When the app enables CMUX and opens DLCI 1 and 2
/// Then `AT` and `AT+CSQ` on each virtual channel end OK
/// When the app closes the physical port and opens it again
/// Then the modem left CMUX mode and answers plain `AT`
fn cmux_channels_answer_and_close_returns_the_modem(plugin: &Plugin, path: &str, baud: u32) {
    plugin
        .invoke("enable_mux", json!({ "path": path }))
        .expect("enable_mux");
    let channels: Vec<String> = [1, 2]
        .map(|dlci| {
            serde_json::from_value(
                plugin
                    .invoke("open_mux_channel", json!({ "path": path, "dlci": dlci }))
                    .expect("open_mux_channel"),
            )
            .unwrap()
        })
        .into();
    for ch in &channels {
        assert_eq!(at(plugin, ch, "AT").status, AtParseStatus::Ok, "{ch}");
        let csq = at(plugin, ch, "AT+CSQ");
        assert_eq!(
            with_prefix(&csq.solicited_body, "+CSQ:"),
            1,
            "{ch}: {csq:?}"
        );
    }
    plugin
        .invoke("close", json!({ "path": path }))
        .expect("close");
    assert!(plugin.managed().is_empty());

    open_modem(plugin, path, baud);
    // The modem may need a moment after CLD before it parses AT again.
    eventually("the modem left the multiplexer", || {
        let options = json!({ "timeoutMs": 500 });
        let args = json!({ "path": path, "command": "AT", "options": options });
        plugin.invoke("at", args).is_ok_and(|r| r["status"] == "ok")
    });
    plugin.ok("close", json!({ "path": path }));
}
