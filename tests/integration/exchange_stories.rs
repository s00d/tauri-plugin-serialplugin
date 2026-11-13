//! Stories for AT / exchange flows the random model does not reach: concurrency with an
//! exchange in flight, timing of session timeouts, multi-phase and URC routing, and a
//! device vanishing mid-exchange. Each runs on a PTY device through IPC.
#![cfg(unix)]

use crate::common::*;

/// Run `cmd` over IPC on a helper thread.
fn invoke_bg(
    plugin: &Plugin,
    cmd: &'static str,
    args: Value,
) -> std::thread::JoinHandle<Result<Value, Value>> {
    let webview = plugin.webview.clone();
    std::thread::spawn(move || invoke(&webview, cmd, args))
}

/// Given a device that reads the command and never answers
/// When `cancel_exchange` is sent while the exchange (30 s timeout) waits
/// Then the cancel succeeds and the exchange fails at once (no wait for its timeout)
#[test]
fn cancel_ends_an_exchange_in_flight() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let exchange = invoke_bg(
        &plugin,
        "exchange",
        json!({ "path": path, "value": "AT\r", "options": { "timeoutMs": 30000, "rxPrepare": "none" } }),
    );
    assert_eq!(recv_until(&mut master, |b| b.ends_with(b"\r")), b"AT\r");
    assert!(plugin
        .invoke("cancel_exchange", json!({ "path": path }))
        .is_ok());
    assert!(exchange.join().unwrap().is_err(), "cancelled exchange");
}

/// Given an exchange waiting for its reply
/// When the app asks `bytes_to_read` and writes `PING` meanwhile
/// Then both succeed while the exchange waits, and the device, answering only after
/// it got `PING`, completes the exchange with OK
#[test]
fn status_and_write_work_while_an_exchange_waits() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let exchange = invoke_bg(
        &plugin,
        "exchange",
        json!({ "path": path, "value": "AT\r", "options": { "timeoutMs": 30000, "rxPrepare": "none" } }),
    );
    assert_eq!(recv_until(&mut master, |b| b.ends_with(b"\r")), b"AT\r");
    assert!(plugin
        .invoke("bytes_to_read", json!({ "path": path }))
        .is_ok());
    assert_eq!(
        plugin.invoke("write", json!({ "path": path, "value": "PING" })),
        Ok(json!(4))
    );
    assert_eq!(recv_until(&mut master, |b| b.len() >= 4), b"PING");
    master.write_all(b"\r\nOK\r\n").unwrap();
    let res = exchange.join().unwrap().expect("exchange");
    assert_eq!(res["status"], "ok");
}

/// Given a watched port with an AT session timeout of 300 ms and a silent device
/// When `at` runs while another caller polls `bytes_to_read` / `read_clear_to_send`
/// Then the `at` fails with its session timeout and the status calls keep working
#[test]
fn session_timeout_holds_with_watch_and_status_io() {
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let drain = {
        let (stop, mut master) = (stop.clone(), master);
        std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            while !stop.load(Ordering::SeqCst) {
                let _ = master.read(&mut buf);
            }
        })
    };
    let (_, channel) = ipc_channel();
    plugin
        .invoke(
            "watch",
            json!({ "path": path, "options": {}, "channel": channel }),
        )
        .expect("watch");
    plugin
        .invoke(
            "configure_at_session",
            json!({ "path": path, "session": { "defaultTimeoutMs": 300, "expectOk": false } }),
        )
        .expect("configure_at_session");
    let status_io = {
        let (stop, webview, path) = (stop.clone(), plugin.webview.clone(), path.clone());
        std::thread::spawn(move || {
            let mut calls = 0;
            while !stop.load(Ordering::SeqCst) {
                invoke(&webview, "bytes_to_read", json!({ "path": path })).expect("bytes_to_read");
                invoke(&webview, "read_clear_to_send", json!({ "path": path })).ok();
                calls += 1;
            }
            calls
        })
    };
    let res = plugin.invoke(
        "at",
        json!({ "path": path, "command": "AT", "options": null }),
    );
    stop.store(true, Ordering::SeqCst);
    assert!(status_io.join().unwrap() > 0);
    drain.join().unwrap();
    assert!(res.is_err(), "missing OK must time out: {res:?}");
}

/// Given a watched port
/// When the reply to `AT+CSQ` carries an unsolicited `+CREG` line before it
/// Then the exchange gets `+CSQ` in its solicited body and the watch gets `+CREG` as URC
#[test]
fn urc_during_an_exchange_goes_to_the_watch() {
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    let (channel, events) = event_channel();
    plugin
        .sp()
        .watch(path.clone(), WatchOptions::default(), channel)
        .expect("watch");
    // The story keeps `master`: dropping it would unplug the device.
    let device = responder(
        Box::new(master.try_clone_native().unwrap()),
        b"\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n",
    );
    let res = plugin
        .invoke(
            "exchange",
            json!({ "path": path, "value": "AT+CSQ\r", "options": { "timeoutMs": 3000, "rxPrepare": "none" } }),
        )
        .expect("exchange");
    assert_eq!(device.join().unwrap(), b"AT+CSQ\r");
    assert_eq!(res["status"], "ok");
    assert!(res["solicitedBody"]
        .as_array()
        .unwrap()
        .iter()
        .any(|l| l.as_str().unwrap().starts_with("+CSQ:")));
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match events.recv_timeout(left).expect("URC on the watch") {
            SerialEvent::Urc { line, .. } if line.contains("+CREG:") => break,
            SerialEvent::Urc { .. } => continue,
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// Given a modem that prompts `>` for `AT+CMGS=1` and then answers `SEND OK`
/// When the app sends the command with completion `atIntermediate`, then the PDU bytes
/// Then the first exchange ends on the intermediate prompt and the second with OK
#[test]
fn cmgs_prompt_then_send_ok() {
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    // The story keeps `master`: dropping it would unplug the device.
    let mut io = master.try_clone_native().unwrap();
    let modem = std::thread::spawn(move || {
        let cmd = recv_until(&mut io, |b| b.ends_with(b"\r"));
        io.write_all(b"\r\n>\r\n").unwrap();
        let pdu = recv_until(&mut io, |b| b.ends_with(&[0x1A]));
        io.write_all(b"\r\nSEND OK\r\n").unwrap();
        (cmd, pdu)
    });
    let prompt = plugin
        .invoke(
            "exchange",
            json!({ "path": path, "value": "AT+CMGS=1\r", "options": {
                "timeoutMs": 3000, "rxPrepare": "none",
                "completionMode": "atIntermediate", "command": "AT+CMGS=1" } }),
        )
        .expect("prompt exchange");
    assert!(prompt["matched"].get("intermediate").is_some(), "{prompt}");
    let done = plugin
        .invoke(
            "exchange_binary",
            json!({ "path": path, "value": [0x41, 0x1A], "options": {
                "timeoutMs": 3000, "rxPrepare": "none", "command": "" } }),
        )
        .expect("pdu exchange");
    assert_eq!(done["status"], "ok");
    let (cmd, pdu) = modem.join().unwrap();
    assert_eq!(cmd, b"AT+CMGS=1\r");
    assert_eq!(pdu, [0x41, 0x1A]);
}

/// Given an AT session with `stopOnError: false` and a modem that ignores the first
/// command and answers OK to the second
/// When `at_phases` runs both phases
/// Then both results come back: the first timed out (not OK), the second OK
#[test]
fn phases_continue_after_a_failure_without_stop_on_error() {
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    // The story keeps `master`: dropping it would unplug the device.
    let mut io = master.try_clone_native().unwrap();
    let modem = std::thread::spawn(move || {
        let first = recv_until(&mut io, |b| b.ends_with(b"\r"));
        let second = recv_until(&mut io, |b| b.ends_with(b"\r"));
        io.write_all(b"\r\nOK\r\n").unwrap();
        (first, second)
    });
    plugin
        .invoke(
            "configure_at_session",
            json!({ "path": path, "session": {
                "stopOnError": false, "defaultTimeoutMs": 500, "expectOk": false } }),
        )
        .expect("configure");
    let results = plugin
        .invoke(
            "at_phases",
            json!({ "path": path, "phases": [
                { "write": "ATFAIL", "command": "phase1", "timeoutMs": 150, "rxPrepare": "none" },
                { "write": "ATOK", "command": "phase2", "timeoutMs": 2000, "rxPrepare": "none" },
            ] }),
        )
        .expect("at_phases");
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_ne!(results[0]["status"], "ok");
    assert_eq!(results[0]["timedOut"], true);
    assert_eq!(results[1]["status"], "ok");
    assert_eq!(results[1]["timedOut"], false);
    let (first, second) = modem.join().unwrap();
    assert!(first.starts_with(b"ATFAIL") && second.starts_with(b"ATOK"));
}

/// Wait until the hub of `path` holds `n` idle bytes (and the fd nothing).
fn idle_holds(plugin: &Plugin, path: &str, n: usize) {
    let sp = plugin.sp();
    eventually("idle RX", || {
        hub_buffered(&sp, path) == n
            && plugin.invoke("bytes_to_read", json!({ "path": path })) == Ok(json!(n))
    });
}

/// Given an `OK` the device sent before any request, held as idle RX
/// When an exchange with `rxPrepare: none` runs and the device stays silent
/// Then the exchange completes from the replayed idle bytes
#[test]
fn idle_rx_is_replayed_into_an_exchange_without_rx_prepare() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    master.write_all(b"\r\nOK\r\n").unwrap();
    idle_holds(&plugin, &path, 6);
    let res = plugin
        .invoke(
            "exchange",
            json!({ "path": path, "value": "AT\r", "options": { "timeoutMs": 1000, "rxPrepare": "none" } }),
        )
        .expect("exchange from the idle buffer");
    assert_eq!(res["status"], "ok");
}

/// Given a stale `OK` left as idle RX
/// When a default (drain) exchange runs and the device stays silent
/// Then the stale reply does not complete it: the exchange times out
#[test]
fn a_drain_exchange_ignores_a_stale_reply() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    master.write_all(b"\r\nOK\r\n").unwrap();
    idle_holds(&plugin, &path, 6);
    let res = plugin.invoke(
        "exchange",
        json!({ "path": path, "value": "AT+CSQ\r", "options": { "timeoutMs": 300 } }),
    );
    assert!(res.is_err(), "stale OK completed the exchange: {res:?}");
}

/// Given an exchange with a 30 s timeout
/// When the device disappears right after reading the command
/// Then the exchange fails at once
#[test]
fn an_exchange_fails_when_the_device_disappears() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let exchange = invoke_bg(
        &plugin,
        "exchange",
        json!({ "path": path, "value": "AT\r", "options": { "timeoutMs": 30000, "rxPrepare": "none" } }),
    );
    assert_eq!(recv_until(&mut master, |b| b.ends_with(b"\r")), b"AT\r");
    drop(master);
    assert!(
        exchange.join().unwrap().is_err(),
        "exchange after disconnect"
    );
}

/// Given a session configured over IPC with guest-js's keys (`appendCr: false`,
/// `resultFormat: "numeric"`, `defaultIdleMs`, default drain prepare) and a device that
/// answers numeric result codes
/// When `at` runs without options, then with `appendCr`, `completionMode` and
/// `solicitedPrefixes` options
/// Then the first command goes out without CR and completes on `0`; the second gets its
/// CR and its prefix line as solicited body (not a URC), and the third ends on the
/// `> ` prompt
#[test]
fn at_and_session_options_from_guest_js_keys_change_behaviour() {
    use tauri_plugin_serialplugin::at::session::{AtCommandOptions, AtSessionConfig};
    let session = json!({
        "defaultTimeoutMs": 3000, "appendCr": false, "resultFormat": "numeric",
        "defaultRxPrepare": "drain", "expectOk": false, "stopOnError": true
    });
    let at_opts = json!({
        "appendCr": true, "solicitedPrefixes": ["+XYZ:"], "timeoutMs": 3000,
        "rxPrepare": "drain"
    });
    keys_survive::<AtSessionConfig>(session.clone());
    keys_survive::<AtSessionConfig>(json!({
        "defaultIdleMs": 30, "defaultTerminators": ["OK"], "defaultSolicitedPrefixes": ["+X:"]
    }));
    keys_survive::<AtCommandOptions>(at_opts.clone());
    keys_survive::<AtCommandOptions>(json!({ "completionMode": "atIntermediate", "idleMs": 5 }));

    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    // The story keeps `master`: dropping it would unplug the device.
    let mut io = master.try_clone_native().unwrap();
    let modem = std::thread::spawn(move || {
        let first = recv_until(&mut io, |b| b.len() >= 3);
        io.write_all(b"0\r").unwrap();
        let second = recv_until(&mut io, |b| b.ends_with(b"\r"));
        io.write_all(b"+XYZ: 1\r0\r").unwrap();
        let third = recv_until(&mut io, |b| b.ends_with(b"\r"));
        io.write_all(b"\r\n> ").unwrap();
        (first, second, third)
    });
    plugin
        .invoke(
            "configure_at_session",
            json!({ "path": path, "session": session }),
        )
        .expect("configure_at_session");
    let first = plugin
        .invoke(
            "at",
            json!({ "path": path, "command": "ATZ", "options": null }),
        )
        .expect("at without CR");
    assert_eq!(first["status"], "ok", "{first}");
    let second = plugin
        .invoke(
            "at",
            json!({ "path": path, "command": "AT", "options": at_opts }),
        )
        .expect("at with options");
    assert_eq!(second["status"], "ok", "{second}");
    assert_eq!(second["solicitedBody"], json!(["+XYZ: 1"]));
    let third = plugin
        .invoke(
            "at",
            json!({ "path": path, "command": "AT+CMGS=2", "options": {
                "completionMode": "atIntermediate", "appendCr": true, "rxPrepare": "none" } }),
        )
        .expect("at ending on the prompt");
    assert!(third["matched"].get("intermediate").is_some(), "{third}");
    let (first, second, third) = modem.join().unwrap();
    assert_eq!(first, b"ATZ");
    assert_eq!(second, b"AT\r");
    assert_eq!(third, b"AT+CMGS=2\r");
}

/// Given a modem that prompts `> ` for `AT+CMGS=2` and answers in numeric format
/// When the app sends a PDU with `send_sms_pdu` over IPC (`timeoutMs`, `resultFormat`)
/// Then the modem gets the command and the PDU with Ctrl-Z, and the second result
/// carries `+CMGS: 7` as solicited body with status ok
#[test]
fn send_sms_pdu_over_ipc_succeeds() {
    use tauri_plugin_serialplugin::at::session::SendSmsPduOptions;
    let options = json!({ "timeoutMs": 3000, "resultFormat": "numeric" });
    keys_survive::<SendSmsPduOptions>(options.clone());
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    // The story keeps `master`: dropping it would unplug the device.
    let mut io = master.try_clone_native().unwrap();
    let modem = std::thread::spawn(move || {
        let cmd = recv_until(&mut io, |b| b.ends_with(b"\r"));
        io.write_all(b"\r\n> ").unwrap();
        let pdu = recv_until(&mut io, |b| b.ends_with(&[0x1A]));
        io.write_all(b"\r\n+CMGS: 7\r\n0\r").unwrap();
        (cmd, pdu)
    });
    let results = plugin
        .invoke(
            "send_sms_pdu",
            json!({ "path": path, "length": 2, "pdu": [0x30, 0x30], "options": options }),
        )
        .expect("send_sms_pdu");
    let results = results.as_array().unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[1]["status"], "ok", "{}", results[1]);
    assert_eq!(results[1]["solicitedBody"], json!(["+CMGS: 7"]));
    let (cmd, pdu) = modem.join().unwrap();
    assert_eq!(cmd, b"AT+CMGS=2\r");
    assert_eq!(pdu, [0x30, 0x30, 0x1A]);
}
