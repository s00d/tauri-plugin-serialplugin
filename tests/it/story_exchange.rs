//! AT from the webview: exchanges, sessions, phases, SMS.

use crate::common::*;
use serde_json::{json, Value};
use serialplugin_testing::Modem;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri_plugin_serialplugin::error::ErrorKind;

fn exchange_args(path: &str, value: &str, rx_prepare: &str) -> Value {
    let options = json!({ "timeoutMs": ANSWERED_MS, "rxPrepare": rx_prepare });
    json!({ "path": path, "value": value, "options": options })
}

fn lines(reply: &Value, key: &str) -> Vec<String> {
    serde_json::from_value(reply[key].clone()).unwrap()
}

/// Given an exchange whose device read the command and does not answer
/// When the app asks `bytes_to_read` and writes meanwhile, and the device answers after that
/// Then both calls work and the exchange ends OK
/// When the next exchange is cancelled, and one after it loses its device
/// Then they end at once, as cancelled and as disconnected, and the cancelled one leaves the
/// port usable
#[test]
fn an_exchange_waits_for_its_answer() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();
        let exchange = || plugin.start("exchange", exchange_args(&path, "AT\r", "none"));

        let waiting = exchange();
        assert_eq!(device.command(), b"AT\r");
        plugin.ok("bytes_to_read", json!({ "path": path }));
        assert_eq!(
            plugin.invoke("write", json!({ "path": path, "value": "PING" })),
            Ok(json!(4))
        );
        assert_eq!(device.recv_until(|b| b.len() >= 4), b"PING");
        device.send(b"\r\nOK\r\n");
        assert_eq!(waiting.join().unwrap().unwrap()["status"], "ok");

        let waiting = exchange();
        device.command();
        plugin.ok("cancel_exchange", json!({ "path": path }));
        assert_eq!(kind(waiting.join().unwrap()), ErrorKind::Cancelled);

        let waiting = exchange();
        device.command();
        device.send(b"\r\nOK\r\n");
        assert_eq!(waiting.join().unwrap().unwrap()["status"], "ok");

        let waiting = exchange();
        device.command();
        drop(device);
        assert_eq!(kind(waiting.join().unwrap()), ErrorKind::Disconnected);
    }
}

/// Given an `OK` the device sent before any request
/// When an exchange with `rxPrepare: none` runs and the device stays silent
/// Then it ends with that OK
/// Given such an `OK` again
/// When exchanges that drain (the default) or purge run, and the device answers ERROR
/// Then they end with the ERROR: the stale reply is gone
#[test]
fn bytes_from_before_the_request() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();
        device.send(b"\r\nOK\r\n");
        plugin.settled(&path, 6);
        let reply = plugin.ok("exchange", exchange_args(&path, "AT\r", "none"));
        assert_eq!(reply["status"], "ok");
        assert_eq!(device.command(), b"AT\r");

        for options in [
            json!({ "timeoutMs": ANSWERED_MS }),
            json!({ "timeoutMs": ANSWERED_MS, "rxPrepare": "purge" }),
        ] {
            device.send(b"\r\nOK\r\n");
            plugin.settled(&path, 6);
            let answer = device.answer(b"\r\nERROR\r\n");
            let args = json!({ "path": path, "value": "AT+CSQ\r", "options": options });
            let reply = plugin.ok("exchange", args);
            assert_eq!(reply["status"], "error", "{reply}");
            assert_eq!(answer.join().unwrap(), b"AT+CSQ\r");
        }
    }
}

/// Given a watched port
/// When the reply to `AT+CSQ` comes with a `+CREG` line before it
/// Then the exchange has `+CSQ` in its solicited body and `+CREG` as a URC line, and the watch
/// gets `+CREG` as a URC event although it does not route URCs
#[test]
fn a_urc_during_an_exchange_reaches_the_watch() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let device = plugin.device(device_kind);
        let path = device.path.clone();
        let mut events = plugin.watch(&path, json!({}));
        let answer =
            device.answer(b"\r\n+CREG: 0,1\r\n\r\nAT+CSQ\r\n\r\n+CSQ: 10,99\r\n\r\nOK\r\n");
        let reply = plugin.ok("exchange", exchange_args(&path, "AT+CSQ\r", "none"));
        assert_eq!(answer.join().unwrap(), b"AT+CSQ\r");
        assert_eq!(reply["status"], "ok");
        assert_eq!(lines(&reply, "solicitedBody"), ["+CSQ: 10,99"]);
        assert_eq!(lines(&reply, "urcLines"), ["+CREG: 0,1"]);
        assert_eq!(events.next(&path), Event::Urc("+CREG: 0,1".into()));
        plugin.unwatch(&events);
        assert_eq!(events.try_next(&path), None, "the reply is not watch data");
    }
}

/// Given a watched port with a session timeout and a modem that ignores the command
/// When `at` runs while another caller polls the port
/// Then it times out, and the status calls keep working
/// Given a session that does not stop on errors
/// When phases run whose first command gets no answer
/// Then both results come back: the first timed out, the second OK
/// When the session stops on errors and expects OK
/// Then the phases fail with the first one, and an ERROR reply fails `at`
#[test]
fn session_defaults_hold_for_every_command() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let device = plugin.device(device_kind);
        let path = device.path.clone();
        let modem = Modem::new().silent("ATMUTE");
        let log = modem.log();
        let _modem = device.serve(modem);
        let _events = plugin.watch(&path, json!({}));
        let session = |session: Value| {
            plugin.ok(
                "configure_at_session",
                json!({ "path": path, "session": session }),
            );
        };
        let at = |command: &str| {
            let args = json!({ "path": path, "command": command, "options": null });
            plugin.invoke("at", args)
        };

        session(json!({ "defaultTimeoutMs": 50, "expectOk": false }));
        let stop = Arc::new(AtomicBool::new(false));
        let polls = {
            let (stop, sp, path) = (stop.clone(), plugin.sp(), path.clone());
            std::thread::spawn(move || {
                let mut polls = 0;
                while !stop.load(Ordering::SeqCst) {
                    sp.bytes_to_read(path.clone()).unwrap();
                    // A PTY has no modem lines: only that it returns counts.
                    let _ = sp.read_clear_to_send(path.clone());
                    polls += 1;
                }
                polls
            })
        };
        // The session's time, not a default.
        assert_eq!(at("ATMUTE"), Err(json!("exchange timed out after 50 ms")));
        stop.store(true, Ordering::SeqCst);
        assert!(polls.join().unwrap() > 0);

        let phases = || {
            plugin.invoke(
                "at_phases",
                json!({ "path": path, "phases": [
                    { "write": "ATMUTE", "command": "mute", "timeoutMs": 50, "rxPrepare": "none" },
                    { "write": b"AT+CSQ\r", "command": "AT+CSQ", "timeoutMs": ANSWERED_MS },
                ] }),
            )
        };
        session(json!({ "defaultTimeoutMs": ANSWERED_MS, "stopOnError": false }));
        let results = phases().unwrap();
        assert_eq!(results.as_array().unwrap().len(), 2);
        assert_eq!(results[0]["command"], "mute");
        assert_ne!(results[0]["status"], "ok");
        assert_eq!(results[0]["timedOut"], true);
        assert_eq!(results[1]["command"], "AT+CSQ");
        assert_eq!(results[1]["status"], "ok");
        assert_eq!(results[1]["timedOut"], false);
        assert_eq!(at("ATNO").unwrap()["status"], "error");

        session(json!({ "defaultTimeoutMs": ANSWERED_MS, "expectOk": true }));
        let before = log.commands().len();
        assert_eq!(kind(phases()), ErrorKind::Timeout);
        assert!(at("ATNO").is_err(), "ERROR with expectOk");
        assert_eq!(at("AT").unwrap()["status"], "ok");
        let sent: Vec<String> = log
            .commands()
            .split_off(before)
            .into_iter()
            .map(|c| c.line)
            .collect();
        assert_eq!(sent, ["ATMUTE", "ATNO", "AT"]);
    }
}

/// Given a numeric session without CR (`appendCr: false`) and a device that answers numeric
/// result codes
/// When `at` runs without options, then with `appendCr` and `solicitedPrefixes`, then with
/// `completionMode: atIntermediate`
/// Then the first command has no CR and ends on `0`; the second has one and its prefix line as
/// solicited body; the third ends on the `> ` prompt
/// When the app sends the PDU with `exchange_binary`
/// Then it ends on `SEND OK`
#[test]
fn options_change_how_a_command_ends() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();
        let session = json!({
            "defaultTimeoutMs": ANSWERED_MS, "appendCr": false, "resultFormat": "numeric",
            "defaultRxPrepare": "none"
        });
        plugin.ok(
            "configure_at_session",
            json!({ "path": path, "session": session }),
        );
        let at = |command: &str, options: Value| {
            plugin.start(
                "at",
                json!({ "path": path, "command": command, "options": options }),
            )
        };

        let reply = at("ATZ", json!(null));
        assert_eq!(device.recv_until(|b| b.len() >= 3), b"ATZ");
        device.send(b"0\r");
        assert_eq!(reply.join().unwrap().unwrap()["status"], "ok");

        let options = json!({ "appendCr": true, "solicitedPrefixes": ["+XYZ:"] });
        let reply = at("AT", options);
        assert_eq!(device.command(), b"AT\r");
        device.send(b"+XYZ: 1\r0\r");
        let reply = reply.join().unwrap().unwrap();
        assert_eq!(reply["status"], "ok");
        assert_eq!(lines(&reply, "solicitedBody"), ["+XYZ: 1"]);

        let options = json!({ "appendCr": true, "completionMode": "atIntermediate" });
        let reply = at("AT+CMGS=2", options);
        assert_eq!(device.command(), b"AT+CMGS=2\r");
        device.send(b"\r\n> ");
        let reply = reply.join().unwrap().unwrap();
        assert!(reply["matched"].get("intermediate").is_some(), "{reply}");

        let options = json!({ "timeoutMs": ANSWERED_MS, "rxPrepare": "none", "command": "" });
        let reply = plugin.start(
            "exchange_binary",
            json!({ "path": path, "value": [0x41, 0x1A], "options": options }),
        );
        assert_eq!(device.recv_until(|b| b.ends_with(&[0x1A])), [0x41, 0x1A]);
        device.send(b"\r\nSEND OK\r\n");
        let reply = reply.join().unwrap().unwrap();
        assert_eq!(reply["matched"], "sendOk");
        assert_eq!(reply["status"], "ok");
    }
}

/// Given a modem that prompts for `AT+CMGS` and answers in text, then one in numeric format
/// When the app sends a PDU with `send_sms_pdu`
/// Then the modem got the command and the PDU, and the second result has `+CMGS: 7` as
/// solicited body
#[test]
fn an_sms_is_sent_as_a_pdu() {
    for &device_kind in KINDS {
        for format in ["verbose", "numeric"] {
            let plugin = Plugin::new();
            let device = plugin.device(device_kind);
            let modem = Modem::new();
            let modem = if format == "numeric" {
                modem.numeric()
            } else {
                modem
            };
            let log = modem.log();
            let _modem = device.serve(modem);
            let options = json!({ "timeoutMs": ANSWERED_MS, "resultFormat": format });
            let results = plugin.ok(
                "send_sms_pdu",
                json!({ "path": device.path, "length": 2, "pdu": b"0011", "options": options }),
            );
            assert_eq!(results.as_array().unwrap().len(), 2);
            assert_eq!(results[1]["status"], "ok", "{results}");
            assert_eq!(lines(&results[1], "solicitedBody"), ["+CMGS: 7"]);
            assert_eq!(log.commands()[0].line, "AT+CMGS=2");
            assert_eq!(log.pdus(), ["0011"]);
        }
    }
}
