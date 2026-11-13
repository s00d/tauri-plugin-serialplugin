//! Stories: split read / write handles, port timeouts and handle locking.

use crate::common::*;

// Split handles only: Windows keeps one handle and the 4.x write path (2x wire time + 1 s).
/// Given a split port with an 80 ms timeout and stalled writes,
/// when a write runs, before and while a watch keeps the hub polling,
/// then each write runs under that 80 ms timeout (the hub's poll timeout never replaces it).
#[cfg(not(windows))]
#[test]
fn set_timeout_takes_effect_and_hub_does_not_overwrite_it() {
    let fake = Fake::new();
    let m = manager_with("timeouts", fake.port(true));
    fake.0.stall_writes.store(true, Ordering::SeqCst);
    m.set_timeout("timeouts".into(), Duration::from_millis(80))
        .unwrap();
    assert!(m.write("timeouts".into(), "x".into()).is_err());

    let (sink, _rx) = old_sink();
    m.watch("timeouts".into(), watch_opts(10, false), sink)
        .unwrap();
    // The hub is polling: it sits in a read on its own handle.
    fake.hold_reads();
    wait_until("hub in a read", Duration::from_secs(2), || fake.held() == 1);
    assert!(m.write("timeouts".into(), "x".into()).is_err());
    fake.release_reads();
    assert_eq!(
        *fake.0.write_timeouts.lock().unwrap(),
        [Duration::from_millis(80); 2]
    );
    m.close("timeouts".into()).unwrap();
}

/// Given a split port with an active watch whose hub is held inside a device read,
/// when writes go out,
/// then they complete while the read is still held (a write never waits for the
/// reader's poll).
#[cfg(not(windows))]
#[test]
fn writes_do_not_wait_for_the_reader() {
    let fake = Fake::new();
    let m = manager_with("fast-write", fake.port(true));
    let (sink, _rx) = old_sink();
    m.watch("fast-write".into(), watch_opts(10, false), sink)
        .unwrap();
    fake.hold_reads();
    wait_until("hub in a read", Duration::from_secs(2), || fake.held() == 1);
    for _ in 0..3 {
        m.write_binary("fast-write".into(), b"ping\n".to_vec())
            .unwrap();
    }
    assert_eq!(fake.held(), 1, "the read is still held");
    assert_eq!(fake.tx(), b"ping\nping\nping\n");
    fake.release_reads();
    m.close("fast-write".into()).unwrap();
}

/// Given a watched port whose driver reports 7 pending bytes, split or not,
/// when bytes_to_read is asked while the hub polls,
/// then it reports 7 every time, never 0 for a busy handle.
#[test]
fn bytes_to_read_is_honest_while_the_hub_reads() {
    for clonable in [true, false] {
        let fake = Fake::new();
        fake.0.driver_pending.store(7, Ordering::SeqCst);
        let m = manager_with("honest", fake.port(clonable));
        let (s, _rx) = old_sink();
        m.watch("honest".into(), WatchOptions::default(), s)
            .unwrap();
        for _ in 0..30 {
            assert_eq!(
                m.bytes_to_read("honest".into()).unwrap(),
                7,
                "clonable={clonable}"
            );
        }
        m.close("honest".into()).unwrap();
    }
}

/// Given a split port with an 80 ms timeout and stalled writes,
/// when a 1200-byte write runs, then a 1-byte one,
/// then the big write runs under twice its wire time, the small one under the port timeout.
#[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
#[test]
fn split_write_waits_at_least_twice_the_wire_time() {
    let fake = Fake::new();
    let m = manager_with("wire", fake.port(true));
    m.set_timeout("wire".into(), Duration::from_millis(80))
        .unwrap();
    fake.0.stall_writes.store(true, Ordering::SeqCst);
    assert!(m.write_binary("wire".into(), vec![0; 1200]).is_err());
    assert!(m.write("wire".into(), "x".into()).is_err());
    // 1200 bytes * 11 bits at 115200 baud: 114 ms on the wire, so 2x is 228 ms
    // (above the 80 ms port timeout, which comes back for the small write).
    assert_eq!(
        *fake.0.write_timeouts.lock().unwrap(),
        [Duration::from_millis(228), Duration::from_millis(80)]
    );
    m.close("wire".into()).unwrap();
}

/// Given clonable fake ports under ble://, tcp:// and a local Unix path,
/// when they are inserted,
/// then only the local path gets a separate write handle.
#[test]
fn ble_and_tcp_paths_are_not_split() {
    for path in ["ble://fake", "tcp://127.0.0.1:9"] {
        let fake = Fake::new();
        let m = manager_with(path, fake.port(true));
        assert_eq!(fake.live(), 1, "{path} got a write clone");
        m.close(path.into()).unwrap();
    }
    #[cfg(unix)]
    {
        let fake = Fake::new();
        let m = manager_with("/dev/fake-split", fake.port(true));
        assert_eq!(fake.live(), 2, "a local Unix port gets a write clone");
        m.close("/dev/fake-split".into()).unwrap();
    }
}

/// Given a port handle held by a write that blocks in the device,
/// when another call needs the handle,
/// then it fails as Busy (lock timeout) instead of waiting for the write, and the write
/// completes once the device takes the bytes.
#[test]
fn a_busy_port_handle_times_out() {
    let fake = Fake::new();
    let m = manager_with("held", fake.port(true));
    m.set_timeout("held".into(), Duration::from_millis(100))
        .unwrap();
    fake.hold_writes();
    let bg = m.clone();
    let writer = thread::spawn(move || bg.write_binary("held".into(), vec![1]));
    wait_until("write in the device", Duration::from_secs(2), || {
        fake.held() == 1
    });
    let err = m.read_clear_to_send("held".into()).unwrap_err();
    assert_eq!(ErrorKind::from_message(&err.to_string()), ErrorKind::Busy);
    assert_eq!(fake.held(), 1, "the write still holds the handle");
    fake.release_writes();
    assert_eq!(writer.join().unwrap().unwrap(), 1);
    m.close("held".into()).unwrap();
}

/// Given a port that cannot be split (one handle shared with the hub),
/// when a 2000-byte write runs,
/// then the port timeout during the write is twice the wire time plus a second.
#[test]
fn single_handle_write_timeout_is_twice_the_wire_time_plus_a_second() {
    let fake = Fake::new();
    let m = manager_with("one", fake.port(false));
    m.write_binary("one".into(), vec![0; 2_000]).unwrap();
    // 2000 bytes * 11 bits at 115200 baud = 190 ms.
    let seen = fake.0.write_timeouts.lock().unwrap().clone();
    assert_eq!(seen, vec![Duration::from_millis(2 * 190 + 1000)]);
    m.close("one".into()).unwrap();
}

/// Given an open fake port,
/// when line settings, modem lines, break and bytes_to_write are used, also on an unknown path,
/// then each call reaches the device in order and returns its values; the unknown path fails.
#[test]
fn settings_and_modem_lines_reach_the_port() {
    use serialplugin_core::state::{DataBits, FlowControl, Parity};
    let fake = Fake::new();
    let m = manager_with("lines", fake.port(true));
    let p = || "lines".to_string();
    m.set_data_bits(p(), DataBits::Seven).unwrap();
    m.set_flow_control(p(), FlowControl::Hardware).unwrap();
    m.set_parity(p(), Parity::Even).unwrap();
    m.write_request_to_send(p(), true).unwrap();
    m.write_data_terminal_ready(p(), false).unwrap();
    m.set_break(p()).unwrap();
    m.clear_break(p()).unwrap();
    assert_eq!(
        *fake.0.calls.lock().unwrap(),
        [
            "data_bits Seven",
            "flow Hardware",
            "parity Even",
            "rts true",
            "dtr false",
            "break on",
            "break off"
        ]
    );
    for lines in [[true, false, true, false], [false, true, false, true]] {
        *fake.0.lines.lock().unwrap() = Some(lines);
        let read = [
            m.read_clear_to_send(p()).unwrap(),
            m.read_data_set_ready(p()).unwrap(),
            m.read_ring_indicator(p()).unwrap(),
            m.read_carrier_detect(p()).unwrap(),
        ];
        assert_eq!(read, lines);
    }
    fake.0.out_pending.store(7, Ordering::SeqCst);
    assert_eq!(m.bytes_to_write(p()).unwrap(), 7);

    let q = || "nope".to_string();
    assert!(m.set_data_bits(q(), DataBits::Eight).is_err());
    assert!(m.set_flow_control(q(), FlowControl::None).is_err());
    assert!(m.set_parity(q(), Parity::None).is_err());
    assert!(m.write_request_to_send(q(), true).is_err());
    assert!(m.write_data_terminal_ready(q(), true).is_err());
    assert!(m.set_break(q()).is_err());
    assert!(m.clear_break(q()).is_err());
    assert!(m.cancel_read(q()).is_err());
    assert!(m.disable_mux(q()).is_err());
    m.close(p()).unwrap();
}
