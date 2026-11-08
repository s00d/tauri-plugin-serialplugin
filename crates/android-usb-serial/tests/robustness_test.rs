#![cfg(feature = "fake-transport")]

//! Drivers used before `open` and short control-IN replies return errors, not panics.

use android_usb_serial::config::LineConfig;
use android_usb_serial::device::open_port;
use android_usb_serial::drivers::create_driver;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::probe::DriverType;
use android_usb_serial::transport::Transport;
use std::sync::Arc;

const ALL: [DriverType; 7] = [
    DriverType::CdcAcm,
    DriverType::Ftdi,
    DriverType::Cp21xx,
    DriverType::Ch34x,
    DriverType::Prolific,
    DriverType::GsmModem,
    DriverType::ChromeCcd,
];

#[test]
fn driver_before_open_errors_instead_of_panicking() {
    for t in ALL {
        let mut d = create_driver(t, 0);
        assert!(d.write(b"x").is_err(), "{t:?} write before open");
        // Line / control calls may be no-ops on some chips; they must not panic.
        let _ = d.set_dtr(true);
        let _ = d.set_rts(true);
        let _ = d.set_break(true);
        let _ = d.set_line_config(LineConfig::default());
        let _ = d.modem_status();
    }
}

#[test]
fn driver_write_after_close_errors() {
    for (t, fake) in [
        (DriverType::CdcAcm, FakeTransport::cdc_single_iface()),
        (DriverType::Ftdi, FakeTransport::ftdi_ft232r()),
        (DriverType::Cp21xx, FakeTransport::cp2102()),
        (DriverType::Ch34x, FakeTransport::ch340_dual_iface()),
    ] {
        let transport: Arc<dyn Transport> = Arc::new(fake);
        let mut d = create_driver(t, 0);
        d.open(&transport).expect("open");
        d.close().expect("close");
        assert!(d.write(b"x").is_err(), "{t:?} write after close");
    }
}

fn open(fake: &FakeTransport) -> android_usb_serial::port::SerialPortHandle {
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    open_port(transport, 0).expect("open")
}

#[test]
fn cp21xx_empty_modem_status_reply_is_an_error() {
    let fake = FakeTransport::cp2102();
    let mut port = open(&fake);
    fake.script_control_in_response(vec![]);
    assert!(port.modem_status().is_err());
    fake.script_control_in_response(vec![0x10]);
    assert!(port.modem_status().expect("1 byte").cts);
}

#[test]
fn ch34x_short_replies_are_errors() {
    let fake = FakeTransport::ch340_dual_iface();
    let mut port = open(&fake);
    fake.script_control_in_response(vec![]);
    assert!(port.modem_status().is_err());
    fake.script_control_in_response(vec![0x00]);
    assert!(port.set_break(true).is_err(), "set_break needs 2 bytes");
}

#[test]
fn pl2303_empty_status_reply_is_an_error() {
    let fake = FakeTransport::pl2303_hx();
    let mut port = open(&fake);
    fake.script_control_in_response(vec![]);
    assert!(port.modem_status().is_err());
}

#[test]
fn ftdi_empty_modem_status_reply_is_an_error() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open(&fake);
    fake.script_control_in_response(vec![]);
    assert!(port.modem_status().is_err());
    fake.script_control_in_response(vec![0x10, 0x60]);
    assert!(port.modem_status().expect("2 bytes").cts);
}
