#![cfg(feature = "fake-transport")]

//! Prolific PL2303 driver semantic tests.

use android_usb_serial::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::probe::{DriverType, ProbeTable};
use android_usb_serial::transport::Transport;
use std::sync::Arc;

const STATUS_NOTIFICATION: u8 = 0xa1;

fn script_pl2303_open(fake: &FakeTransport) {
    fake.script_control_in_response(vec![0]);
}

fn open_pl2303(fake: &FakeTransport) -> android_usb_serial::port::SerialPortHandle {
    script_pl2303_open(fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    open_port(transport, 0).expect("open")
}

fn patch_usb11(fake: &FakeTransport) {
    fake.patch_device_descriptor(|d| {
        d[2] = 0x10;
        d[3] = 0x01;
    });
}

#[test]
fn type_hx_detection() {
    let fake = FakeTransport::pl2303_hx();
    patch_usb11(&fake);
    let _port = open_pl2303(&fake);
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request_type == 0xC0 && c.value == 0x8484),
        "HX black magic vendor IN"
    );
}

#[test]
fn type_hxn_detection() {
    let fake = FakeTransport::pl2303_hxn();
    script_pl2303_open(&fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let _port = open_port(transport, 0).expect("open");
    assert!(
        !fake.recorded_controls().iter().any(|c| c.value == 0x8484),
        "HXN skips black magic reads"
    );
}

#[test]
fn type_01_detection() {
    let fake = FakeTransport::pl2303_type01();
    script_pl2303_open(&fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let _port = open_port(transport, 0).expect("open");
    let controls = fake.recorded_controls();
    assert!(
        controls
            .iter()
            .any(|c| c.request == 0x01 && c.value == 2 && c.index == 0x0024),
        "type01 magic write"
    );
}

#[test]
fn type_ta_detection() {
    let fake = FakeTransport::pl2303_ta();
    script_pl2303_open(&fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let _port = open_port(transport, 0).expect("open");
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request_type == 0xC0 && c.value == 0x8080),
        "TA hx status probe"
    );
}

#[test]
fn black_magic_order_non_hxn() {
    let fake = FakeTransport::pl2303_hx();
    patch_usb11(&fake);
    let _port = open_pl2303(&fake);
    let values: Vec<u16> = fake
        .recorded_controls()
        .iter()
        .filter(|c| c.request_type == 0xC0 || c.request_type == 0x40)
        .map(|c| c.value)
        .collect();
    assert!(values.contains(&0x8484), "black magic sequence: {values:?}");
}

#[test]
fn hxn_skips_black_magic() {
    let fake = FakeTransport::pl2303_hxn();
    script_pl2303_open(&fake);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let _port = open_port(transport, 0).expect("open");
    assert!(!fake.recorded_controls().iter().any(|c| c.value == 0x8383));
}

#[test]
fn set_line_skip_if_unchanged_then_purge() {
    let fake = FakeTransport::pl2303_hx();
    patch_usb11(&fake);
    let mut port = open_pl2303(&fake);
    let cfg = LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    };
    port.set_line_config(cfg).expect("line");
    let before = fake.recorded_controls().len();
    port.set_line_config(cfg).expect("line again");
    let after = fake.recorded_controls().len();
    assert_eq!(before, after, "unchanged line should skip SET_LINE");
    port.purge(PurgeKind::Rx).expect("purge");
    assert!(
        fake.recorded_controls()
            .iter()
            .any(|c| c.request == 0x01 && c.value == 0x0008),
        "purge after skip"
    );
}

#[test]
fn interrupt_thread_decodes_byte8() {
    let fake = FakeTransport::pl2303_hx();
    patch_usb11(&fake);
    let mut port = open_pl2303(&fake);
    let mut packet = vec![0u8; 10];
    packet[0] = STATUS_NOTIFICATION;
    packet[8] = 0x80;
    fake.push_interrupt_in(&packet);
    std::thread::sleep(std::time::Duration::from_millis(50));
    let status = port.modem_status().expect("modem");
    assert!(status.cts);
}

#[test]
fn flow_dtr_dsr_unsupported() {
    let fake = FakeTransport::pl2303_hx();
    patch_usb11(&fake);
    let mut port = open_pl2303(&fake);
    assert!(port.set_flow_control(FlowControl::DtrDsr).is_err());
}

#[test]
fn prolific_probe_table_entry() {
    let table = ProbeTable::default_table();
    assert_eq!(table.find(0x067B, 0x2303, &[]), DriverType::Prolific);
}
