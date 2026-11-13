//! Stories for watch delivery the random model does not reach: native parsers over
//! split writes and timers, the batch tail at `close`, and writes under steady RX.
#![cfg(unix)]

use crate::common::*;

/// Wait until the port's fd holds nothing (the hub read everything; with a watch
/// attached, the hub's idle buffer stays empty).
fn fd_drained(plugin: &Plugin, path: &str) {
    eventually("fd drained", || {
        plugin.invoke("bytes_to_read", json!({ "path": path })) == Ok(json!(0))
    });
}

/// Given a watch with a `readline` parser
/// When the device sends `fir`, `st\nsec`, `ond\nrest` as separate reads, then the
/// app unwatches
/// Then the watch gets `first`, `second`, and `rest` as the remainder on unwatch
/// Given then a watch with an `interByteTimeout` parser (30 ms)
/// When the device sends three bytes and nothing after
/// Then the frame still arrives
#[test]
fn parser_frames_split_writes_and_timer_frames() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let sp = plugin.sp();
    let parser = |v: Value| Some(serde_json::from_value(v).unwrap());

    let (channel, rx) = event_channel();
    let id = sp
        .watch_framed(
            path.clone(),
            WatchOptions::default(),
            parser(json!({ "type": "readline" })),
            channel,
        )
        .expect("watch with parser");
    for part in [&b"fir"[..], b"st\nsec", b"ond\nrest"] {
        master.write_all(part).unwrap();
        fd_drained(&plugin, &path);
    }
    assert_eq!(next_data(&rx), b"first");
    assert_eq!(next_data(&rx), b"second");
    plugin
        .invoke("unwatch", json!({ "channelId": id }))
        .expect("unwatch");
    assert_eq!(next_data(&rx), b"rest", "remainder on unwatch");

    let (channel, rx) = event_channel();
    sp.watch_framed(
        path.clone(),
        WatchOptions::default(),
        parser(json!({ "type": "interByteTimeout", "interval": 30 })),
        channel,
    )
    .expect("watch with timeout parser");
    master.write_all(&[0xAA, 0x01, 0x02]).unwrap();
    assert_eq!(next_data(&rx), [0xAA, 0x01, 0x02]);
}

/// Given a watch with a 5 s batch window holding `abcdefghi` (the hub read it all)
/// When the app closes the port
/// Then the whole batch has reached the channel by the time `close` returns
#[test]
fn the_batch_tail_arrives_before_close_returns() {
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let (channel, rx) = event_channel();
    plugin
        .sp()
        .watch(
            path.clone(),
            WatchOptions {
                serial_data_flush_interval_ms: Some(5_000),
                ..Default::default()
            },
            channel,
        )
        .expect("watch");
    for part in [&b"abc"[..], b"def", b"ghi"] {
        master.write_all(part).unwrap();
        fd_drained(&plugin, &path);
    }
    plugin
        .invoke("close", json!({ "path": path }))
        .expect("close");
    let mut data = Vec::new();
    while let Ok(ev) = rx.try_recv() {
        if let SerialEvent::Data { data: d, .. } = ev {
            data.extend(d);
        }
    }
    assert_eq!(data, b"abcdefghi");
}

/// Given a watched port whose device sends a steady stream
/// When the app writes 40 times
/// Then every write gets the port and returns its byte count
#[test]
fn writes_get_the_port_under_steady_rx() {
    let plugin = Plugin::new();
    let (master, path) = pty_port(&plugin);
    let (_, channel) = ipc_channel();
    plugin
        .invoke(
            "watch",
            json!({ "path": path, "options": {}, "channel": channel }),
        )
        .expect("watch");
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut feeder = master.try_clone_native().unwrap();
    let mut drain = master;
    let feed = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                let _ = feeder.write_all(&[b'x'; 256]);
            }
        })
    };
    let drainer = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 1024];
            while !stop.load(Ordering::SeqCst) {
                let _ = drain.read(&mut buf);
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
    feed.join().unwrap();
    drainer.join().unwrap();
}

/// Given watch options in guest-js's shape (`size`, `serialDataFlushIntervalMs`,
/// `timeout`; `routeUrc` left out) on a Rust-side watch
/// When the device sends 8 bytes, then a URC line
/// Then the bytes arrive (a full `size` flushes) long before the 60 s interval, and the URC
/// line comes as plain data (`routeUrc` defaults to false)
/// Given then a watch with `routeUrc: true`
/// When the device sends the URC line again
/// Then it arrives as a URC event
/// And `watch_ports` options keep their keys
#[test]
fn watch_options_from_guest_js_keys_change_behaviour() {
    use tauri_plugin_serialplugin::events::WatchPortsOptions;
    let options = json!({ "size": 4, "serialDataFlushIntervalMs": 60000, "timeout": 100 });
    keys_survive::<WatchOptions>(options.clone());
    keys_survive::<WatchOptions>(json!({ "routeUrc": true }));
    keys_survive::<WatchPortsOptions>(json!({ "singlePortPerDevice": true, "pollIntervalMs": 50 }));
    let plugin = Plugin::new();
    let (mut master, path) = pty_port(&plugin);
    let sp = plugin.sp();

    let (channel, rx) = event_channel();
    let id = sp
        .watch(
            path.clone(),
            serde_json::from_value(options).unwrap(),
            channel,
        )
        .expect("watch");
    master.write_all(b"abcdefgh").unwrap();
    let mut data = Vec::new();
    while data.len() < 8 {
        data.extend(next_data(&rx));
    }
    assert_eq!(data, b"abcdefgh");
    master.write_all(b"\r\n+CREG: 1\r\n").unwrap();
    let mut data = Vec::new();
    while data.len() < 12 {
        match rx.recv_timeout(WAIT).expect("URC bytes as data") {
            SerialEvent::Data { data: d, .. } => data.extend(d),
            other => panic!("routeUrc is off: {other:?}"),
        }
    }
    assert_eq!(data, b"\r\n+CREG: 1\r\n");
    plugin
        .invoke("unwatch", json!({ "channelId": id }))
        .expect("unwatch");

    let (channel, rx) = event_channel();
    sp.watch(
        path.clone(),
        serde_json::from_value(json!({ "routeUrc": true })).unwrap(),
        channel,
    )
    .expect("watch with routeUrc");
    master.write_all(b"\r\n+CREG: 1\r\n").unwrap();
    let deadline = Instant::now() + WAIT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left).expect("URC event") {
            SerialEvent::Urc { line, .. } => break assert_eq!(line, "+CREG: 1"),
            _ => continue,
        }
    }

    let (_, channel) = ipc_channel();
    let ports = plugin
        .invoke(
            "watch_ports",
            json!({ "options": { "singlePortPerDevice": true, "pollIntervalMs": 50 }, "channel": channel }),
        )
        .expect("watch_ports with options");
    plugin
        .invoke("unwatch_ports", json!({ "channelId": ports }))
        .expect("unwatch_ports");
}
