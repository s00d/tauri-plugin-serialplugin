//! CDC ACM notification-endpoint stories the driver model cannot reach: failures
//! during open, unusual interface layouts and dropping the driver.

use super::common::*;
use android_usb_serial::drivers::CdcAcmDriver;

/// Given a CDC IAD device whose notification endpoint is already in use
/// When port 0 opens
/// Then the open fails and releases its claims, and a retry after the endpoint is
///      freed succeeds.
#[test]
fn busy_notification_endpoint_fails_open_and_allows_a_retry() {
    let fake = FakeTransport::cdc_iad();
    let held = fake.open_interrupt_in(0x81, 64).unwrap();
    assert!(open_port(shared(&fake), 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());
    drop(held);
    drop(open_port(shared(&fake), 0).expect("retry"));
    assert!(fake.claimed_interfaces().is_empty());
}

/// Given a CDC IAD device whose data interface has no bulk OUT endpoint
/// When port 0 opens (after the notification reader has started)
/// Then the open fails, claims are released and the notification endpoint is free.
#[test]
fn failure_after_the_notification_reader_starts_cleans_up() {
    let fake = FakeTransport::cdc_iad();
    fake.configure_endpoints(&[(0, vec![ep(0x81, 3)]), (1, vec![ep(0x82, 2)])]);
    assert!(open_port(shared(&fake), 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());
    assert!(fake.open_interrupt_in(0x81, 64).is_ok());
}

/// Given a vendor interface followed by a lone ACM comm interface (no data interface)
/// When port 0 opens
/// Then only the comm interface is claimed and it carries the data.
#[test]
fn lone_comm_interface_is_the_port() {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_interfaces(vec![iface(0, 255, 0), iface(1, 2, 2)]);
    fake.configure_endpoints(&[(1, vec![ep(0x81, 2), ep(0x02, 2)])]);
    let mut port = open_port(shared(&fake), 0).unwrap();
    assert_eq!(fake.claimed_interfaces(), [1]);
    port.write(b"x").unwrap();
    assert_eq!(fake.recorded_bulk_out()[0].endpoint, 0x02);
}

/// Given an open CDC driver with a running notification reader
/// When the driver is dropped without close
/// Then the reader stops and the notification endpoint is free.
#[test]
fn dropping_the_driver_stops_the_notification_reader() {
    let fake = FakeTransport::cdc_iad();
    let mut driver = CdcAcmDriver::new(0);
    driver.open(&shared(&fake)).unwrap();
    assert!(fake.open_interrupt_in(0x81, 64).is_err(), "reader owns it");
    drop(driver);
    assert!(fake.open_interrupt_in(0x81, 64).is_ok());
}
