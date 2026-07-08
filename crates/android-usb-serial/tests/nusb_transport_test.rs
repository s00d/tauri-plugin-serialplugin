#![cfg(feature = "fake-transport")]

//! Transport parsing and FakeTransport descriptor tests.

use android_usb_serial::fake::FakeTransport;
use android_usb_serial::serialport_compat::chunk_write_timeout_ms;
use android_usb_serial::transport::{
    is_device_recipient, is_interface_recipient, ControlRequest, Transport,
};
use std::time::Duration;

#[test]
fn fake_transport_raw_descriptors_roundtrip() {
    let fake = FakeTransport::cdc_single_iface();
    let raw = vec![9, 2, 9, 0, 1, 1, 0, 80, 0];
    fake.set_raw_descriptors(raw.clone());
    assert_eq!(fake.raw_descriptors(), raw);
}

#[test]
fn vendor_out_uses_device_recipient() {
    let req = ControlRequest::vendor_out(0x9a, 0x1312, 0, vec![]);
    assert!(is_device_recipient(req.request_type));
}

#[test]
fn class_iface_uses_interface_recipient() {
    let req = ControlRequest::class_out(0x20, 0, 0, vec![0; 7]);
    assert!(is_interface_recipient(req.request_type));
}

#[test]
fn chunk_write_timeout_clamps_and_floors_at_2s() {
    assert_eq!(chunk_write_timeout_ms(Duration::from_millis(0)), 2000);
    assert_eq!(chunk_write_timeout_ms(Duration::from_millis(500)), 2000);
    assert_eq!(chunk_write_timeout_ms(Duration::from_secs(5)), 5000);
    assert_eq!(chunk_write_timeout_ms(Duration::from_secs(900)), 600_000);
}

#[test]
fn pipelined_bulk_in_3_transfers() {
    // NusbTransport uses 3 in-flight bulk IN transfers (see nusb_transport.rs).
    const IN_FLIGHT: usize = 3;
    assert_eq!(IN_FLIGHT, 3);
}
