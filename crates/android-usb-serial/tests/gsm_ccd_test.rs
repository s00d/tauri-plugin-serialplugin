#![cfg(feature = "fake-transport")]

//! GSM modem and Chrome CCD driver tests.

use android_usb_serial::config::LineConfig;
use android_usb_serial::device::open_port;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::Transport;
use std::sync::Arc;

#[test]
fn gsm_init_only_control() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x1782, 0x4D10, &[]), DriverType::GsmModem);

    let fake = FakeTransport::gsm_modem();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).expect("open");
    assert!(port.set_line_config(LineConfig::default()).is_err());
    let init = fake
        .recorded_controls()
        .iter()
        .any(|c| c.request == 0x22 && c.request_type == 0x21);
    assert!(init, "GSM init control transfer expected");
}

#[test]
fn gsm_line_config_unsupported() {
    let fake = FakeTransport::gsm_modem();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).expect("open");
    assert!(port.set_line_config(LineConfig::default()).is_err());
}

#[test]
fn ccd_three_ports_no_cdc_init() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x18D1, 0x5014, &[]), DriverType::ChromeCcd);
    assert_eq!(
        table.port_count_product(0x18D1, 0x5014, DriverType::ChromeCcd, &[]),
        3
    );

    let fake = FakeTransport::chrome_ccd_3port();
    for port_index in 0..3 {
        let t: Arc<dyn Transport> = Arc::new(fake.clone());
        let mut port = open_port(t, port_index).expect("open");
        assert!(port.set_line_config(LineConfig::default()).is_err());
    }
    assert!(
        fake.recorded_controls().iter().all(|c| c.request != 0x20),
        "no CDC SET_LINE on CCD"
    );
}

#[test]
fn ccd_port2_bulk_endpoints() {
    let fake = FakeTransport::chrome_ccd_3port();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 2).expect("open port 2");
    port.write(b"PING").expect("write");
    let bulk = fake.recorded_bulk_out();
    assert!(
        bulk.iter().any(|b| b.endpoint == 0x06),
        "port2 bulk OUT endpoint 0x06"
    );
}
