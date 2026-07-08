#![cfg(feature = "fake-transport")]

//! CDC ACM driver semantic tests on fake transport.

use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::drivers::line_coding_bytes;
use android_usb_serial::fake::{FakeTransport, RecordedControl};
use android_usb_serial::transport::{EndpointInfo, InterfaceInfo, Transport};
use std::sync::Arc;

fn open_on(fake: &FakeTransport, port_index: usize) -> android_usb_serial::port::SerialPortHandle {
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    open_port(transport, port_index).expect("open")
}

fn has_set_line(controls: &[RecordedControl], index: u16) -> bool {
    controls
        .iter()
        .any(|c| c.request == 0x20 && c.index == index)
}

#[test]
fn castrated_single_iface_open_windex() {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_vendor_product(0x1234, 0x0001);
    fake.set_interfaces(vec![InterfaceInfo {
        id: 0,
        class: 2,
        subclass: 2,
        protocol: 0,
    }]);
    fake.configure_endpoints(&[(
        0,
        vec![
            EndpointInfo {
                address: 0x81,
                attributes: 2,
                max_packet_size: 64,
                interval: 0,
            },
            EndpointInfo {
                address: 0x02,
                attributes: 2,
                max_packet_size: 64,
                interval: 0,
            },
        ],
    )]);
    let mut port = open_on(&fake, 0);
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("line");
    assert_eq!(fake.claimed_interfaces(), vec![0]);
    assert!(has_set_line(&fake.recorded_controls(), 0));
}

#[test]
fn iad_scan_resolves_comm_data_pair() {
    let fake = FakeTransport::cdc_iad();
    let mut port = open_on(&fake, 0);
    port.set_line_config(LineConfig::default()).expect("line");
    let claimed = fake.claimed_interfaces();
    assert!(claimed.contains(&0));
    assert!(claimed.contains(&1));
    assert!(has_set_line(&fake.recorded_controls(), 0));
}

#[test]
fn comm_data_fallback_multi_port() {
    let fake = FakeTransport::cdc_multi();
    let mut port = open_on(&fake, 1);
    port.set_line_config(LineConfig {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("line");
    let claimed = fake.claimed_interfaces();
    assert!(claimed.contains(&2));
    assert!(claimed.contains(&3));
    assert!(has_set_line(&fake.recorded_controls(), 2));
}

#[test]
fn set_line_coding_7e1_bulk_out() {
    let fake = FakeTransport::cdc_single_iface();
    let mut port = open_on(&fake, 0);
    let cfg = LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Seven,
        parity: Parity::Even,
        stop_bits: StopBits::One,
    };
    port.set_line_config(cfg).expect("line");
    let expected = line_coding_bytes(&cfg);
    let set_line = fake
        .recorded_controls()
        .into_iter()
        .find(|c| c.request == 0x20)
        .expect("SET_LINE_CODING");
    assert_eq!(set_line.data, expected);
    port.write(b"AT\r").expect("write");
    assert_eq!(fake.take_tx(), b"AT\r");
}

#[test]
fn unsupported_modem_status_returns_ok_false() {
    let fake = FakeTransport::cdc_iad();
    let mut port = open_on(&fake, 0);
    let status = port.modem_status().expect("modem");
    assert!(!status.cts);
    assert!(!status.dsr);
    assert!(!status.ri);
    assert!(!status.cd);
}
