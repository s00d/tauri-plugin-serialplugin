//! Stories: the life of a `SerialPortHandle`.

use super::common::*;

/// Given an FTDI adapter
/// When its port opens, is configured, reads in the background (more than the caller's buffer,
/// then with the reader stopped), and closes
/// Then the latency timer reaches the chip with its minimum of 1 ms, the data arrives in order
/// without the status bytes, a second reader start changes nothing, the stopped reader frees
/// the endpoint, the close releases the interface, every later call is `Disconnected`, and a
/// second close is harmless. The errors read as the app shows them
#[test]
fn a_port_from_open_to_close() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_port(shared(&fake), 0).unwrap();
    assert_eq!(fake.claimed_interfaces(), [0]);
    fake.clear_recorded();
    port.set_latency_timer(4).unwrap();
    port.set_latency_timer(0).unwrap();
    assert_eq!(
        ctls(&fake),
        [(0x40, 9, 4, 1, vec![]), (0x40, 9, 1, 1, vec![])]
    );
    assert_eq!(port.try_read(&mut [0; 8]).unwrap(), 0, "no reader, no data");

    port.start_reader().unwrap();
    port.start_reader().unwrap();
    assert!(fake.open_bulk_in(0x81, 64).is_err(), "the reader owns it");
    fake.push_rx(b"\x01\x60hello");
    let mut got: Vec<u8> = vec![];
    let mut buf = [0; 2];
    assert!(eventually(|| {
        let n = port.try_read(&mut buf).unwrap();
        got.extend(&buf[..n]);
        got.len() >= 5
    }));
    assert_eq!(got, b"hello");
    port.stop_reader();
    assert!(fake.open_bulk_in(0x81, 64).is_ok());

    port.close();
    assert!(fake.claimed_interfaces().is_empty());
    let disconnected = kind(&UsbSerialError::Disconnected);
    let results = [
        port.write(b"1").map(drop),
        port.read(&mut [0; 8]).map(drop),
        port.try_read(&mut [0; 8]).map(drop),
        port.start_reader(),
        port.set_line_config(LineConfig::default()),
        port.set_flow_control(FlowControl::None),
        port.set_dtr(true),
        port.set_rts(true),
        port.set_break(true),
        port.set_latency_timer(1),
        port.purge(PurgeKind::Both),
        port.clear(PurgeKind::Both),
        port.modem_status().map(drop),
    ];
    for r in results {
        assert_eq!(r.map_err(|e| kind(&e)), Err(disconnected));
    }
    port.close();

    for (e, text) in [
        (UsbSerialError::Unsupported("x".into()), "unsupported: x"),
        (UsbSerialError::Io("x".into()), "io: x"),
        (UsbSerialError::TimedOut, "timed out"),
        (UsbSerialError::Disconnected, "disconnected"),
        (UsbSerialError::Cancelled, "cancelled"),
        (UsbSerialError::ProbeFailed("x".into()), "probe failed: x"),
    ] {
        assert_eq!(e.to_string(), text);
    }
}

/// Given an adapter whose bulk IN endpoint is held by someone else
/// When its reader is started, and again after the endpoint is free
/// Then the first start reports the failure instead of running without input, the second works
#[cfg(feature = "serialport-compat")]
#[test]
fn an_adapter_over_a_taken_endpoint() {
    use android_usb_serial::serialport_compat::SerialPortAdapter;
    let fake = FakeTransport::cp2102();
    let handle = open_port(shared(&fake), 0).unwrap();
    let port =
        SerialPortAdapter::new(handle, "usb#0", LineConfig::default(), FlowControl::None).unwrap();
    let held = fake.open_bulk_in(0x81, 64).unwrap();
    assert!(port.start_reader().is_err());
    drop(held);
    port.start_reader().unwrap();
    port.shutdown();
}
