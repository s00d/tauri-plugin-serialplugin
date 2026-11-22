//! A port's life: reads, disconnects, reopening, multiplexer channels, quitting the app.

use crate::common::*;
use serde_json::{json, Value};
use serialplugin_testing::mux::{CLD, SABM, UIH};
use serialplugin_testing::{eventually, Modem, MuxOptions};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri_plugin_serialplugin::error::ErrorKind;
use tauri_plugin_serialplugin::state::{PortState, SerialportInfo};

fn read_waiting(path: &str) -> Value {
    json!({ "path": path, "timeout": 600_000, "size": 4 })
}

/// Wait until a read holds the port's read slot, which only a refused second read shows.
/// Probes with a zero timeout hold the slot only while they are set up; a read started while
/// one did is refused itself and started again.
fn pending_read(plugin: &Plugin, path: &str) -> std::thread::JoinHandle<Result<Value, Value>> {
    let start = || plugin.start("read_binary", read_waiting(path));
    let mut read = start();
    eventually("a second read is refused", || {
        let probe = plugin.invoke("read_binary", json!({ "path": path, "timeout": 0 }));
        if kind(probe) == ErrorKind::Busy {
            return true;
        }
        if read.is_finished() {
            let refused = std::mem::replace(&mut read, start()).join().unwrap();
            assert_eq!(kind(refused), ErrorKind::Busy);
        }
        false
    });
    read
}

/// Given an open device with a read waiting for data
/// When a second read is sent
/// Then it is refused as busy, and the first keeps waiting
/// When the read is cancelled, another one gets its bytes, and a third one waits as the device
/// goes away
/// Then they end as cancelled, with the bytes, and as disconnected
/// When the app closes the path, which stays managed, and opens the device again
/// Then the path is free in between and the reopened port talks
#[test]
fn reads_wait_until_cancelled_answered_or_unplugged() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();

        let read = pending_read(&plugin, &path);
        assert!(!read.is_finished(), "the first read keeps waiting");
        plugin.ok("cancel_read", json!({ "path": path }));
        assert_eq!(kind(read.join().unwrap()), ErrorKind::Cancelled);

        let read = pending_read(&plugin, &path);
        device.send(b"da");
        device.send(b"ta");
        assert_eq!(read.join().unwrap(), Ok(json!(b"data")));

        plugin.ok("close", json!({ "path": path }));
        assert!(plugin.managed().is_empty());
        plugin.open_device(&mut device);
        assert_eq!(
            plugin.invoke("write", json!({ "path": path, "value": "again" })),
            Ok(json!(5))
        );
        assert_eq!(device.recv_until(|b| b.len() >= 5), b"again");

        let read = pending_read(&plugin, &path);
        drop(device);
        assert_eq!(kind(read.join().unwrap()), ErrorKind::Disconnected);
        assert_eq!(plugin.managed(), std::slice::from_ref(&path));
        plugin.ok("close", json!({ "path": path }));
        let again = plugin.invoke("close", json!({ "path": path }));
        assert_eq!(kind(again), ErrorKind::NotFound);
    }
}

/// Given a modem without a multiplexer, then one with it
/// When the app enables the multiplexer
/// Then the first refuses and stays usable; the second starts it, control channel first
/// When the app opens DLCI 0, 1 and 2, talks AT on both data channels and watches one
/// Then DLCI 0 is refused, the replies come back on their channels, and what the modem sends
/// on DLCI 2 reaches its watch
/// When a channel is closed while another caller lists the ports
/// Then both finish and the channel is gone
/// When the multiplexer is disabled, then enabled again and the port closed
/// Then the modem gets CLD each time, answers plain AT after the first, and nothing is managed
#[test]
fn multiplexer_channels_from_enable_to_close() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let device = plugin.device(device_kind);
        let path = device.path.as_str();
        let enable = || {
            let options = json!({ "command": "AT+CMUX=0", "timeoutMs": ANSWERED_MS });
            plugin.invoke("enable_mux", json!({ "path": path, "options": options }))
        };
        let at = |path: &str| {
            let options = json!({ "timeoutMs": ANSWERED_MS });
            plugin.ok(
                "at",
                json!({ "path": path, "command": "AT+CSQ", "options": options }),
            )
        };
        let channel = |dlci: u8| {
            let args = json!({ "path": path, "dlci": dlci });
            plugin.invoke("open_mux_channel", args)
        };

        let plain = device.serve(Modem::new());
        assert!(enable().is_err(), "the modem answers ERROR");
        assert_eq!(at(path)["status"], "ok");
        assert!(channel(1).is_err(), "no multiplexer");
        drop(plain);

        let modem = Modem::new().mux(MuxOptions::default());
        let log = modem.log();
        let _modem = device.serve(modem);
        enable().expect("enable_mux");
        assert!(channel(0).is_err(), "DLCI 0 is the control channel");
        let one = channel(1).unwrap().as_str().unwrap().to_string();
        let two = channel(2).unwrap().as_str().unwrap().to_string();
        let sabm: Vec<u8> = log
            .frames()
            .iter()
            .filter(|f| f.kind == SABM)
            .map(|f| f.dlci)
            .collect();
        assert_eq!(sabm, [0, 1, 2]);
        for (path, dlci) in [(&one, 1), (&two, 2)] {
            let reply = at(path);
            assert_eq!(reply["status"], "ok");
            assert_eq!(reply["solicitedBody"], json!(["+CSQ: 20,99"]));
            let last = log.commands().pop().unwrap();
            assert_eq!((last.dlci, last.line.as_str()), (Some(dlci), "AT+CSQ"));
        }
        let mut events = plugin.watch(&two, json!({}));
        plugin.ok("write", json!({ "path": two, "value": "AT\r" }));
        events.data_until(&two, |b| b.ends_with(b"OK\r\n"));

        let stop = Arc::new(AtomicBool::new(false));
        let lister = {
            let (stop, sp) = (stop.clone(), plugin.sp());
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    sp.managed_ports().unwrap();
                }
            })
        };
        plugin.ok("close", json!({ "path": two }));
        stop.store(true, Ordering::SeqCst);
        lister.join().unwrap();
        assert_eq!(plugin.managed(), [path.to_string(), one.clone()]);
        assert_eq!(at(&one)["status"], "ok");

        let clds = || {
            let frames = log.frames();
            let cld = |f: &&_| {
                let f: &serialplugin_testing::mux::Frame = f;
                f.dlci == 0 && f.kind == UIH && f.payload.first() == Some(&CLD)
            };
            frames.iter().filter(cld).count()
        };
        plugin.ok("disable_mux", json!({ "path": path }));
        assert_eq!(clds(), 1);
        assert_eq!(plugin.managed(), [path.to_string()]);
        assert_eq!(at(path)["status"], "ok");

        enable().expect("enable_mux again");
        channel(1).expect("open_mux_channel");
        plugin.ok("close", json!({ "path": path }));
        eventually("CLD on close", || clds() == 2);
        assert!(plugin.managed().is_empty());
    }
}

/// Given an app whose Rust side stopped the RX hub of an open port through the 4.x records
/// When the webview reads
/// Then the hub runs again and the read gets what the device sends
/// Given a record that Rust left closed
/// When the webview writes to it
/// Then it is refused as not found
#[test]
fn records_changed_from_rust() {
    let plugin = Plugin::new();
    let sp = plugin.sp();
    let mut device = plugin.device(Kind::Tcp);
    let path = device.path.clone();
    let hub = {
        let ports = sp.serialports.lock().unwrap();
        let Some(PortState::Connected(port)) = ports.get(&path).map(|i| &i.state) else {
            panic!("{path} is not connected");
        };
        let hub = port.rx_hub.lock().unwrap().clone();
        hub.expect("an open port has a hub")
    };
    hub.request_stop();
    eventually("the hub stopped", || hub.is_finished());
    let read = plugin.start("read_binary", read_waiting(&path));
    device.send(b"data");
    assert_eq!(read.join().unwrap(), Ok(json!(b"data")));

    let closed = SerialportInfo {
        state: PortState::Closed,
    };
    let path = "/dev/closed-record";
    sp.serialports.lock().unwrap().insert(path.into(), closed);
    let write = plugin.invoke("write", json!({ "path": path, "value": "x" }));
    assert_eq!(kind(write), ErrorKind::NotFound);
}

/// Given an app with open ports, one of them watched
/// When the user quits it
/// Then the ports are closed: the devices see it, nothing is managed, and the watch got no
/// disconnect
#[test]
fn quitting_the_app_closes_its_ports() {
    let mut plugin = Plugin::new();
    let mut device = plugin.device(Kind::Tcp);
    let path = device.path.clone();
    let _other = plugin.device(Kind::Tcp);
    let mut events = plugin.watch(&path, json!({}));
    device.send(b"bye");
    assert_eq!(events.data_until(&path, |b| b.len() >= 3), b"bye");
    plugin.exit();
    assert!(plugin.sp().managed_ports().unwrap().is_empty());
    assert_eq!(events.try_next(&path), None);
    let mut buf = [0; 1];
    eventually("the bridge sees the end of the connection", || {
        matches!(device.io().read(&mut buf), Ok(0))
    });
}
