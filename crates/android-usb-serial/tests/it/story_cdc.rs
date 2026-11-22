//! Stories: CDC ACM devices with a notification endpoint and unusual interface layouts.

use super::common::*;
use android_usb_serial::drivers::CdcAcmDriver;

/// Given a CDC IAD device whose notification endpoint is in use, and one whose data interface
/// has no bulk OUT endpoint (found only after the notification reader has started)
/// When port 0 opens
/// Then the open fails with every claim released and the notification endpoint free again, and
/// a retry once the endpoint is free succeeds
#[test]
fn an_open_that_fails_leaves_nothing_behind() {
    let fake = FakeTransport::cdc_iad();
    let held = fake.open_interrupt_in(0x81, 64).unwrap();
    assert!(open_port(shared(&fake), 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());
    drop(held);
    drop(open_port(shared(&fake), 0).expect("retry"));
    assert!(fake.claimed_interfaces().is_empty());

    fake.configure_endpoints(&[(0, vec![ep(0x81, 3)]), (1, vec![ep(0x82, 2)])]);
    assert!(open_port(shared(&fake), 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());
    assert!(fake.open_interrupt_in(0x81, 64).is_ok());
}

/// Given a vendor interface followed by a lone ACM comm interface with a notification endpoint
/// and no data interface
/// When port 0 opens, carries data, and its driver is dropped without a close
/// Then only the comm interface is claimed and carries the data, the notification reader owns
/// its endpoint while the driver lives, and the drop frees it
#[test]
fn a_lone_comm_interface_lives_and_dies() {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_interfaces(vec![iface(0, 255, 0), iface(1, 2, 2)]);
    fake.configure_endpoints(&[(1, vec![ep(0x83, 3), ep(0x81, 2), ep(0x02, 2)])]);
    let mut driver = CdcAcmDriver::new(0);
    driver.open(&shared(&fake)).unwrap();
    assert_eq!(fake.claimed_interfaces(), [1]);
    driver.write(b"x").unwrap();
    assert_eq!(fake.recorded_bulk_out()[0].endpoint, 0x02);
    assert!(
        fake.open_interrupt_in(0x83, 64).is_err(),
        "the reader owns it"
    );
    drop(driver);
    assert!(fake.open_interrupt_in(0x83, 64).is_ok());
}
