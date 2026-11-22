//! Watches from the webview: what reaches the JS channel, and when.

use crate::common::*;
use serde_json::{json, Value};
use tauri::ipc::JavaScriptChannelId;
use tauri::Manager;
use tauri_plugin_serialplugin::api::SerialPort;
use tauri_plugin_serialplugin::commands;
use tauri_plugin_serialplugin::error::ErrorKind;
use tauri_plugin_serialplugin::events::WatchOptions;

/// A batch window no test waits for.
const NEVER_MS: u64 = 600_000;

/// Given a watch with a `readline` parser
/// When the device sends `fir`, `st\nsec`, `ond\nrest` as separate reads, then the app unwatches
/// Then the channel gets `first`, `second`, and `rest` as the remainder
/// Given a watch with an `interByteTimeout` parser
/// When the device sends three bytes and nothing after
/// Then they arrive as a frame
/// Given a watch with a `readline` parser on URC routing
/// When lines and a URC arrive, the last line unfinished as the device goes away
/// Then the parser sees the lines without the URC, its remainder comes before the disconnect
#[test]
fn parsers_frame_the_stream() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();

        let mut events = plugin.watch(&path, json!({ "parser": { "type": "readline" } }));
        for part in [&b"fir"[..], b"st\nsec", b"ond\nrest"] {
            device.send(part);
            plugin.settled(&path, 0);
        }
        plugin.unwatch(&events);
        for frame in [&b"first"[..], b"second", b"rest"] {
            assert_eq!(events.next(&path), Event::Data(frame.to_vec()));
        }

        let parser = json!({ "type": "interByteTimeout", "interval": 30 });
        let mut events = plugin.watch(&path, json!({ "parser": parser }));
        device.send(&[0xAA, 1, 2]);
        assert_eq!(events.next(&path), Event::Data(vec![0xAA, 1, 2]));
        plugin.unwatch(&events);

        let args = json!({ "options": { "routeUrc": true }, "parser": { "type": "readline" } });
        let mut events = plugin.watch(&path, args);
        device.send(b"one\n+CREG: 1\ntwo\nla");
        plugin.settled(&path, 0);
        drop(device);
        for event in [
            Event::Data(b"one".to_vec()),
            Event::Urc("+CREG: 1".into()),
            Event::Data(b"two".to_vec()),
            Event::Data(b"la".to_vec()),
            Event::Disconnect,
        ] {
            assert_eq!(events.next(&path), event);
        }
    }
}

/// Given a watch whose batch window never ends (`serialDataFlushIntervalMs`, which wins over
/// `timeout`) and takes 8 bytes (`size`)
/// When the device sends 8 bytes, then 9 in three reads, and the app closes the port
/// Then the first 8 arrive at once, and the rest has arrived by the time `close` returns
/// Given the same on a reopened port
/// When the app unwatches with bytes in the batch, and watches again
/// Then the first watch got its bytes by the time `unwatch` returns, the second only its own
#[test]
fn batches_flush_when_full_and_before_the_watch_ends() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();
        let options = json!({ "options": { "size": 8, "serialDataFlushIntervalMs": NEVER_MS, "timeout": 1 } });

        let mut events = plugin.watch(&path, options.clone());
        device.send(b"12345678");
        assert_eq!(events.data_until(&path, |b| b.len() >= 8), b"12345678");
        for part in [&b"ab"[..], b"cd", b"ef"] {
            device.send(part);
            plugin.settled(&path, 0);
        }
        plugin.ok("close", json!({ "path": path }));
        assert_eq!(drain(&mut events, &path), b"abcdef");

        plugin.open_device(&mut device);
        let mut events = plugin.watch(&path, options.clone());
        device.send(b"old");
        plugin.settled(&path, 0);
        plugin.unwatch(&events);
        assert_eq!(drain(&mut events, &path), b"old");
        let mut next = plugin.watch(&path, options);
        device.send(b"newbytes");
        assert_eq!(next.data_until(&path, |b| b.len() >= 8), b"newbytes");
    }
}

/// The data that already reached the channel.
fn drain(events: &mut Events, path: &str) -> Vec<u8> {
    let mut got = Vec::new();
    while let Some(event) = events.try_next(path) {
        match event {
            Event::Data(data) => got.extend(data),
            other => panic!("{other:?}"),
        }
    }
    got
}

/// Given a watch without `routeUrc`, then one with it
/// When the device sends a URC line
/// Then the first gets its bytes as data, the second the line as a URC event
/// When a third watch takes the port without the second ending
/// Then it is refused, and the second goes on
/// Given the device gone
/// When the app watches the released port
/// Then the watch gets the disconnect
#[test]
fn urc_lines_are_routed_only_when_asked() {
    for &device_kind in KINDS {
        let plugin = Plugin::new();
        let mut device = plugin.device(device_kind);
        let path = device.path.clone();
        let urc = b"\r\n+CREG: 1\r\n";

        let mut plain = plugin.watch(&path, json!({}));
        device.send(urc);
        assert_eq!(plain.data_until(&path, |b| b.len() >= urc.len()), urc);
        plugin.unwatch(&plain);

        let routed = json!({ "options": { "routeUrc": true } });
        let mut events = plugin.watch(&path, routed.clone());
        let (mut third, refused) = plugin.try_watch(&path, routed);
        assert_eq!(kind(refused), ErrorKind::Busy);
        device.send(urc);
        loop {
            match events.next(&path) {
                Event::Urc(line) => break assert_eq!(line, "+CREG: 1"),
                Event::Data(blank) => assert_eq!(blank, b"\r\n"),
                other => panic!("{other:?}"),
            }
        }
        plugin.unwatch(&events);

        drop(device);
        serialplugin_testing::eventually("the port is released", || {
            plugin
                .invoke("bytes_to_read", json!({ "path": path }))
                .is_err()
        });
        let mut late = plugin.watch(&path, json!({}));
        assert_eq!(late.next(&path), Event::Disconnect);
        plugin.ok("close", json!({ "path": path }));
        assert_eq!(plain.try_message(), None);
        assert_eq!(third.try_message(), None);
        assert_eq!(late.try_message(), None);
    }
}

/// Given a watched port whose device sends without a pause
/// When the app writes 40 times
/// Then every write gets the port and returns its byte count
#[cfg(unix)]
#[test]
fn writes_get_the_port_under_steady_rx() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let plugin = Plugin::new();
    let device = plugin.device(Kind::Pty);
    let path = device.path.clone();
    let mut events = plugin.watch(&path, json!({}));
    let stop = Arc::new(AtomicBool::new(false));
    let sender = {
        let (stop, mut io) = (stop.clone(), device.clone_io());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let _ = io.write_all(&[b'x'; 256]);
            }
        })
    };
    let reader = {
        let (stop, mut io) = (stop.clone(), device.clone_io());
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let _ = io.read(&mut [0; 1024]);
            }
        })
    };
    for _ in 0..40 {
        assert_eq!(
            plugin.invoke("write", json!({ "path": path, "value": "ping\n" })),
            Ok(json!(5))
        );
    }
    stop.store(true, Ordering::SeqCst);
    sender.join().unwrap();
    reader.join().unwrap();
    assert!(matches!(events.next(&path), Event::Data(_)));
}

/// Given an app whose own command hands a webview's channel to `commands::watch`
/// When the device sends data and the app's webview unwatches, then watches again
/// Then the channel got the data, the watch is an ordinary one (busy for another watch, ended
/// by the plugin's `unwatch`), and a path that is not open is an error
#[test]
fn a_rust_app_streams_a_port_through_the_watch_command() {
    let plugin = Plugin::new();
    let mut device = plugin.device(Kind::Tcp);
    let path = device.path.clone();
    let watch = |events: &Events, path: &str| {
        let channel: JavaScriptChannelId = events.arg().parse().unwrap();
        let options = WatchOptions {
            route_urc: true,
            ..Default::default()
        };
        tauri::async_runtime::block_on(commands::watch(
            plugin.webview.app_handle().clone(),
            plugin.webview.state::<SerialPort<_>>(),
            path.to_string(),
            options,
            channel.channel_on(plugin.webview.as_ref().clone()),
        ))
        .map_err(|e| ErrorKind::from_message(&e.to_string()))
    };
    let mut events = plugin.channel();
    assert_eq!(watch(&events, &path), Ok(events.id));
    device.send(b"line\n+CREG: 1\n");
    assert_eq!(events.next(&path), Event::Urc("+CREG: 1".into()));
    assert_eq!(events.next(&path), Event::Data(b"line\n".to_vec()));
    assert_eq!(kind(plugin.try_watch(&path, json!({})).1), ErrorKind::Busy);
    assert_eq!(watch(&plugin.channel(), &path), Err(ErrorKind::Busy));
    plugin.unwatch(&events);
    plugin.unwatch(&plugin.watch(&path, json!({})));
    let missing = watch(&plugin.channel(), "/dev/serialplugin-no-such-port");
    assert_eq!(missing, Err(ErrorKind::NotFound));
}

/// Given a webview that watches the port list, with and without `singlePortPerDevice`
/// When it subscribes, and subscribes again under the same id
/// Then the first message is the snapshot `available_ports` gives for that option, and the id
/// is refused as busy
/// When it unsubscribes and subscribes another channel, this time without options
/// Then that one gets its snapshot, and the first got nothing more
#[test]
fn the_port_list_reaches_the_webview() {
    let plugin = Plugin::new();
    let snapshot = |v: Value, single: bool| {
        assert_eq!(v["kind"], "snapshot", "{v}");
        let args = json!({ "singlePortPerDevice": single });
        assert_eq!(v["ports"], plugin.ok("available_ports", args));
    };
    for single in [None, Some(false), Some(true)] {
        let mut ports = plugin.channel();
        // Never polled meanwhile: only subscribing sends messages.
        let options = json!({ "singlePortPerDevice": single, "pollIntervalMs": 30_000 });
        let args = json!({ "options": options, "channel": ports.arg() });
        let id = json!(ports.id);
        assert_eq!(plugin.invoke("watch_ports", args.clone()), Ok(id.clone()));
        snapshot(ports.message(), single.unwrap_or(false));
        assert_eq!(kind(plugin.invoke("watch_ports", args)), ErrorKind::Busy);
        plugin.ok("unwatch_ports", json!({ "channelId": id }));
        let mut next = plugin.channel();
        plugin.ok("watch_ports", json!({ "channel": next.arg() }));
        snapshot(next.message(), false);
        plugin.ok("unwatch_ports", json!({ "channelId": next.id }));
        assert_eq!(ports.try_message(), None);
    }
}
