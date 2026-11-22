//! Stories: the read and write handles of one port: what waits for what, with which
//! timeout, and who gets the bytes.

use crate::common::*;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{SerialEvent, WatchOptions};
use serialplugin_core::state::FlowControl;
use std::time::Duration;

/// Given a port with separate read and write handles and an 80 ms timeout,
/// when the hub is held inside a device read and writes go out, small and big, also ones
/// the device does not take,
/// then no write waits for the read, and each runs under the port timeout or twice its
/// wire time, never the hub's poll timeout.
// Windows keeps one handle per port.
#[cfg(not(windows))]
#[test]
fn a_split_port_writes_while_it_reads() {
    let device = FakeDevice::new();
    let m = manager_with("split", device.port());
    m.set_timeout("split".into(), Duration::from_millis(80))
        .unwrap();
    let (events, _rx) = sink::<SerialEvent>();
    m.watch("split".into(), watch_opts(0, false), events)
        .unwrap();
    device.hold_reads(true);
    device.wait_held(1);
    for _ in 0..3 {
        m.write_binary("split".into(), b"ping\n".to_vec()).unwrap();
    }
    assert_eq!(device.held(), 1, "the read is still held");
    assert_eq!(device.written(), b"ping\nping\nping\n");
    device.stall_writes(true);
    assert!(m.write_binary("split".into(), vec![0; 1200]).is_err());
    assert!(m.write("split".into(), "x".into()).is_err());
    m.set_baud_rate("split".into(), 0).unwrap();
    assert!(m.write("split".into(), "x".into()).is_err());
    device.hold_reads(false);
    // 1200 bytes of 11 bits at 115200 baud: 114 ms on the wire. Baud 0 counts as 1.
    let ms = Duration::from_millis;
    assert_eq!(
        device.write_timeouts(),
        [ms(80), ms(80), ms(80), ms(228), ms(80), ms(22_000)]
    );
    m.close("split".into()).unwrap();
}

/// Given ports with one handle (no clone), and network and Bluetooth paths,
/// when they are inserted and written to,
/// then only a local clonable port gets a second handle, and a write over the one handle
/// may take twice its wire time plus a second.
#[test]
fn one_handle_for_both_directions() {
    let device = FakeDevice::new();
    let m = manager_with("one", device.unclonable_port());
    m.write_binary("one".into(), vec![0; 2_000]).unwrap();
    // 2000 bytes of 11 bits at 115200 baud: 190 ms.
    assert_eq!(
        device.write_timeouts(),
        [Duration::from_millis(2 * 190 + 1000)]
    );
    for (path, handles) in [
        ("ble://fake", 1),
        ("tcp://127.0.0.1:9", 1),
        ("/dev/fake", if cfg!(windows) { 1 } else { 2 }),
    ] {
        let device = FakeDevice::new();
        m.insert_port(path.into(), Box::new(device.port())).unwrap();
        assert_eq!(device.open_ports(), handles, "{path}");
    }
    m.close_all().unwrap();
}

/// Given a write blocked in the device,
/// when another call needs its handle,
/// then it fails as busy instead of waiting, and the write completes once the device takes
/// the bytes.
#[test]
fn a_busy_handle_is_reported() {
    let device = FakeDevice::new();
    let m = manager_with("held", device.port());
    device.hold_writes(true);
    let writing = background(&m, |m| m.write_binary("held".into(), vec![1]));
    device.wait_held(1);
    let e = m
        .set_flow_control("held".into(), FlowControl::None)
        .unwrap_err();
    assert_eq!(kind(e), ErrorKind::Busy);
    assert_eq!(device.held(), 1, "the write still holds the handle");
    device.hold_writes(false);
    assert_eq!(writing.join().unwrap().unwrap(), 1);
    m.close("held".into()).unwrap();
}

/// Given a port with bytes in the driver and more than the idle buffer holds,
/// when the count is asked for while the hub reads, and the bytes are read,
/// then the count is never 0 for a busy handle, and the newest 64 KiB are kept;
/// when a read waits for nothing and is cancelled,
/// then it fails as cancelled.
#[test]
fn bytes_wait_for_a_reader() {
    for clonable in [true, false] {
        let device = FakeDevice::new();
        device.set_driver_pending(Some(7));
        let m = if clonable {
            manager_with("count", device.port())
        } else {
            manager_with("count", device.unclonable_port())
        };
        let (events, _rx) = sink::<SerialEvent>();
        m.watch("count".into(), WatchOptions::default(), events)
            .unwrap();
        for _ in 0..30 {
            assert_eq!(m.bytes_to_read("count".into()).unwrap(), 7);
        }
        m.close("count".into()).unwrap();
    }

    let device = FakeDevice::new();
    let m = manager_with("idle", device.port());
    const CAP: usize = 64 * 1024;
    let old: Vec<u8> = vec![b'o'; 10_000];
    device.send(&old);
    wait_idle(&m, &device, "idle", 10_000);
    device.send(&[b'n'; CAP - 1]);
    device.send(b"!");
    wait_idle(&m, &device, "idle", CAP as u32);
    eventually("the oldest bytes were dropped", || {
        let n = m.read_binary("idle".into(), Some(0), Some(1)).unwrap();
        assert!(n == b"o" || n == b"n");
        n == b"n"
    });
    let left = m
        .read_binary("idle".into(), Some(LONG_MS), Some(CAP - 2))
        .unwrap();
    assert_eq!(left.len(), CAP - 2);

    let reading = background(&m, |m| m.read_binary("idle".into(), Some(LONG_MS), Some(9)));
    // Nothing shows that the read waits: cancel until it ends (before, it does nothing).
    eventually("the read was cancelled", || {
        m.cancel_read("idle".into()).unwrap();
        reading.is_finished()
    });
    let e = reading.join().unwrap().unwrap_err();
    assert_eq!(kind(e), ErrorKind::Cancelled);
    m.close("idle".into()).unwrap();
}
