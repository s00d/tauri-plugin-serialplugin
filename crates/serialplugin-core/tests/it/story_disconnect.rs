//! Stories: devices that die (at once, by error, by end of file, in the middle of a call),
//! that only glitch, and a hub thread that dies.

use crate::common::*;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{
    CoreEvent, ExchangeOptions, PortStatus, RxPrepareMode, SerialEvent, WatchOptions,
};
use serialplugin_core::SerialManager;
use std::io;
use std::sync::Arc;

const READ_ERROR: &str = "Serial port disconnected: fake device: read error";
const EOF: &str = "Serial port disconnected: EOF";

fn disconnected(reason: &str) -> PortStatus {
    PortStatus::Disconnected {
        kind: ErrorKind::Disconnected,
        reason: reason.into(),
    }
}

/// Given a watched port whose reads glitch,
/// when they time out, are interrupted, or fail a few times and recover,
/// then only the failures show, as one error event, and the port goes on;
/// when they keep failing (a Windows unplug is PermissionDenied for ever),
/// then the watch gets one Disconnect and nothing after it, subscribers the Disconnected
/// state, pending calls fail, and the path stays managed until it is closed.
#[test]
fn glitches_pass_and_an_unplug_ends_the_port() {
    let device = FakeDevice::new();
    let m = manager_with("usb", device.port());
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    let (events, rx) = sink::<CoreEvent>();
    m.watch_core("usb".into(), watch_opts(0, false), None, events)
        .unwrap();
    for benign in [
        io::ErrorKind::TimedOut,
        io::ErrorKind::WouldBlock,
        io::ErrorKind::Interrupted,
    ] {
        device.fail_reads_times(10, benign);
        device.send(b"ok");
        assert!(matches!(
            next(&rx, "data"),
            CoreEvent::Event(SerialEvent::Data { .. })
        ));
    }
    device.fail_reads_times(4, io::ErrorKind::PermissionDenied);
    device.send(b"back");
    assert!(matches!(
        next(&rx, "error"),
        CoreEvent::Event(SerialEvent::Error { path, message })
            if path == "usb" && message == "Serial read error: fake device: read error"
    ));
    assert_eq!(collect(&rx, 4), b"back");
    m.write("usb".into(), "x".into()).unwrap();

    let reading = background(&m, |m| m.at("usb".into(), "AT".into(), None));
    device.wait_written("the command", |w| w.ends_with(b"AT\r"));
    device.fail_reads(io::ErrorKind::PermissionDenied);
    assert_eq!(next_disconnect(&rx), ("usb".into(), READ_ERROR.into()));
    assert_eq!(next_state(&states_rx, "usb"), disconnected(READ_ERROR));
    assert_eq!(
        kind(reading.join().unwrap().unwrap_err()),
        ErrorKind::Disconnected
    );
    device.wait_closed();
    let e = m.write_core("usb".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(
        (e.kind(), e.to_string()),
        (ErrorKind::Disconnected, READ_ERROR.into())
    );
    assert_eq!(m.managed_ports().unwrap(), ["usb"]);
    m.close("usb".into()).unwrap();
    assert!(rest(rx).is_empty(), "events after Disconnect");
}

/// Given ttys after a hang-up, whose reads return nothing at once,
/// when it happens twice between two bytes, for ever, or during a command's drain,
/// then data resets the count, the third empty read in a row is the end (one Disconnect,
/// handles released, no further read), and the command fails with it.
#[test]
fn end_of_file_three_times_is_the_end() {
    let device = FakeDevice::new();
    let m = manager_with("tty", device.port());
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    let (events, rx) = sink::<CoreEvent>();
    m.watch_core("tty".into(), watch_opts(0, false), None, events)
        .unwrap();
    for byte in b"abcde" {
        // Both at once: the byte must come after the two empty reads.
        device.hold_reads(true);
        device.wait_held(1);
        device.eof_times(2);
        device.send(&[*byte]);
        device.hold_reads(false);
        assert_eq!(next_data(&rx), [*byte]);
    }
    assert_eq!(device.eof_reads(), 10);
    m.write("tty".into(), "x".into()).unwrap();
    device.eof();
    assert_eq!(next_disconnect(&rx), ("tty".into(), EOF.into()));
    assert_eq!(next_state(&states_rx, "tty"), disconnected(EOF));
    device.wait_closed();
    assert_eq!(device.eof_reads(), 13, "reads after the end");
    m.close("tty".into()).unwrap();
    assert!(rest(rx).is_empty());

    let device = FakeDevice::new();
    m.insert_port("tty".into(), Box::new(device.port()))
        .unwrap();
    let command = background(&m, |m| {
        let options = ExchangeOptions {
            rx_prepare: Some(RxPrepareMode::Drain),
            drain_idle_ms: Some(LONG_MS),
            drain_max_ms: Some(LONG_MS),
            ..Default::default()
        };
        m.exchange("tty".into(), "AT\r".into(), options)
    });
    device.eof();
    let e = command.join().unwrap().unwrap_err();
    assert_eq!(kind(e), ErrorKind::Disconnected);
    device.wait_closed();
    assert!(device.written().is_empty());
    m.close_all().unwrap();
}

/// Given a command draining a port whose reads start to fail,
/// when the errors persist,
/// then the command fails with the read error and the port ends.
#[test]
fn a_read_error_ends_a_drain() {
    let device = FakeDevice::new();
    let m = manager_with("drain", device.port());
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    // Idle bytes are dropped as the drain starts: that shows it runs.
    device.send(b"stale");
    wait_idle(&m, &device, "drain", 5);
    let command = background(&m, |m| {
        let options = ExchangeOptions {
            drain_idle_ms: Some(LONG_MS),
            drain_max_ms: Some(LONG_MS),
            ..Default::default()
        };
        m.exchange("drain".into(), "AT\r".into(), options)
    });
    wait_idle(&m, &device, "drain", 0);
    // The read this byte ends may have begun before the drain; the next one is the drain's.
    device.send(b"x");
    device.wait_read();
    device.fail_reads(io::ErrorKind::BrokenPipe);
    let e = command.join().unwrap().unwrap_err().to_string();
    assert_eq!(e, "drain read failed: fake device: read error");
    assert_eq!(next_state(&states_rx, "drain"), disconnected(READ_ERROR));
    m.close("drain".into()).unwrap();
}

/// Given devices with a greeting already waiting, and devices that are dead on arrival,
/// when they are inserted, with and without a first watch,
/// then the stream gets every byte, or Disconnect; subscribers see Opened before it, and
/// the dead path stays managed until it is closed.
#[test]
fn the_first_read_is_not_lost() {
    let m = SerialManager::new();
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    for _ in 0..5 {
        let device = FakeDevice::new();
        device.send(b"RDY\r\n");
        let (events, rx) = sink::<CoreEvent>();
        let id = m
            .insert_port_watch_core(
                "boot".into(),
                Box::new(device.port()),
                watch_opts(0, false),
                None,
                events,
            )
            .unwrap();
        assert_eq!(collect(&rx, 5), b"RDY\r\n");
        m.unwatch_core(id).unwrap();
        m.close("boot".into()).unwrap();
        assert_eq!(next_state(&states_rx, "boot"), PortStatus::Opened);
        assert_eq!(next_state(&states_rx, "boot"), PortStatus::Closed);

        let dead = FakeDevice::new();
        dead.fail_reads(io::ErrorKind::BrokenPipe);
        let (events, rx) = sink::<CoreEvent>();
        m.insert_port_watch_core(
            "boot".into(),
            Box::new(dead.port()),
            WatchOptions::default(),
            None,
            events,
        )
        .expect("a dead device still opens");
        assert_eq!(next_disconnect(&rx), ("boot".into(), READ_ERROR.into()));
        assert_eq!(next_state(&states_rx, "boot"), PortStatus::Opened);
        assert_eq!(next_state(&states_rx, "boot"), disconnected(READ_ERROR));
        assert_eq!(m.managed_ports().unwrap(), ["boot"]);
        m.close("boot".into()).unwrap();
    }
}

/// Given a watch sink that panics and kills the hub thread,
/// when reads come next,
/// then a new hub starts and later bytes are read.
#[test]
fn a_dead_hub_is_restarted_by_the_next_read() {
    let device = FakeDevice::new();
    let m = manager_with("phoenix", device.port());
    let (hit, hits) = sink::<()>();
    m.watch(
        "phoenix".into(),
        watch_opts(0, false),
        Arc::new(move |_| {
            hit(());
            panic!("sink panics (expected by this test)");
        }),
    )
    .unwrap();
    device.send(b"x");
    next(&hits, "sink call");
    device.send(b"hello");
    // Whether the dead thread has ended is not observable: a read fails until it has (its
    // watch is still attached), then starts a new hub.
    let mut got = Vec::new();
    eventually("a new hub read the bytes", || {
        if let Ok(d) = m.read_binary("phoenix".into(), Some(50), Some(5 - got.len())) {
            got.extend(d);
        }
        got.len() >= 5
    });
    assert_eq!(got, b"hello");
    let _ = m.close("phoenix".into());
}
