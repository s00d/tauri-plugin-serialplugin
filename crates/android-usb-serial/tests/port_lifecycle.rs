//! `SerialPortHandle` stories: a closed handle, latency forwarding, a busy IN
//! endpoint.

use super::common::*;

/// Given a handle that was closed
/// When any I/O or control call is made
/// Then each returns `Disconnected` (and a second close is harmless).
#[test]
fn closed_handle_rejects_every_call() {
    let mut h = open_port(shared(&FakeTransport::cdc_iad()), 0).unwrap();
    h.close();
    let d = kind(&UsbSerialError::Disconnected);
    let results = [
        h.write(b"1").map(drop),
        h.read(&mut [0; 8]).map(drop),
        h.try_read(&mut [0; 8]).map(drop),
        h.start_reader(),
        h.set_line_config(LineConfig::default()),
        h.set_flow_control(FlowControl::None),
        h.set_dtr(true),
        h.set_rts(true),
        h.set_break(true),
        h.set_latency_timer(1),
        h.purge(PurgeKind::Both),
        h.clear(PurgeKind::Both),
        h.modem_status().map(drop),
    ];
    for r in results {
        assert_eq!(r.map_err(|e| kind(&e)), Err(d));
    }
    h.close();
}

/// Given an open FTDI handle
/// When the latency timer is set to 4 ms and to 0 ms
/// Then the driver gets SET_LATENCY_TIMER 4 and 1 (the chip minimum) on wIndex 1.
#[test]
fn handle_forwards_the_latency_timer() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_port(shared(&fake), 0).unwrap();
    fake.clear_recorded();
    port.set_latency_timer(4).unwrap();
    port.set_latency_timer(0).unwrap();
    let sent: Vec<_> = ctls(&fake);
    assert_eq!(sent, [(0x40, 9, 4, 1, vec![]), (0x40, 9, 1, 1, vec![])]);
}

/// Given an adapter whose bulk IN endpoint is held by someone else
/// When the reader is started
/// Then `start_reader` reports the failure instead of running without input.
#[cfg(feature = "serialport-compat")]
#[test]
fn start_reader_reports_a_busy_endpoint() {
    use android_usb_serial::serialport_compat::SerialPortAdapter;
    let fake = FakeTransport::cp2102();
    let handle = open_port(shared(&fake), 0).unwrap();
    let port =
        SerialPortAdapter::new(handle, "usb#0", LineConfig::default(), FlowControl::None).unwrap();
    let _held = fake.open_bulk_in(0x81, 64).unwrap();
    assert!(port.start_reader().is_err());
}
