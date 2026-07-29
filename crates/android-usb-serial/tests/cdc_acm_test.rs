#![cfg(feature = "fake-transport")]

//! CDC ACM driver semantic tests on fake transport.

use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::drivers::line_coding_bytes;
use android_usb_serial::fake::{FakeTransport, RecordedControl};
use android_usb_serial::transport::{EndpointInfo, InterfaceInfo, Transport};
use std::sync::Arc;
use std::time::{Duration, Instant};

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
fn modem_status_defaults_false_without_notification_endpoint() {
    let fake = FakeTransport::cdc_single_iface();
    let mut port = open_on(&fake, 0);
    let status = port.modem_status().expect("modem");
    assert!(!status.cts);
    assert!(!status.dsr);
    assert!(!status.ri);
    assert!(!status.cd);
}

#[test]
fn notification_endpoint_open_failure_releases_interfaces_for_retry() {
    let fake = FakeTransport::cdc_iad();
    let interrupt_in = fake
        .open_interrupt_in(0x81, 64)
        .expect("reserve interrupt endpoint");
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());

    assert!(open_port(transport.clone(), 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());

    drop(interrupt_in);
    let port = open_port(transport, 0).expect("retry after initialization failure");
    drop(port);
    assert!(fake.claimed_interfaces().is_empty());
}

#[test]
fn failure_after_notification_reader_starts_cleans_up() {
    let fake = FakeTransport::cdc_iad();
    fake.configure_endpoints(&[
        (
            0,
            vec![EndpointInfo {
                address: 0x81,
                attributes: 3,
                max_packet_size: 64,
                interval: 1,
            }],
        ),
        (
            1,
            vec![EndpointInfo {
                address: 0x82,
                attributes: 2,
                max_packet_size: 64,
                interval: 0,
            }],
        ),
    ]);
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());

    assert!(open_port(transport, 0).is_err());
    assert!(fake.claimed_interfaces().is_empty());
    fake.open_interrupt_in(0x81, 64)
        .expect("notification endpoint released after initialization failure");
}

#[test]
fn serial_state_notifications_update_modem_status_and_release_endpoint() {
    let fake = FakeTransport::cdc_iad();
    fake.push_interrupt_in(&[0xa1, 0x20, 0, 0, 0, 0, 2, 0, 0x0b, 0]);

    let mut port = open_on(&fake, 0);
    let deadline = Instant::now() + Duration::from_secs(1);
    let status = loop {
        let status = port.modem_status().expect("modem");
        if status.cd && status.dsr && status.ri {
            break status;
        }
        assert!(Instant::now() < deadline, "serial state was not updated");
        std::thread::sleep(Duration::from_millis(5));
    };
    assert!(!status.cts);

    fake.push_interrupt_in(&[0xa1, 0x20, 0, 0, 0, 0, 2, 0, 0, 0]);
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        let status = port.modem_status().expect("modem");
        if !status.cd && !status.dsr && !status.ri {
            break;
        }
        assert!(Instant::now() < deadline, "serial state was not cleared");
        std::thread::sleep(Duration::from_millis(5));
    }

    drop(port);
    fake.open_interrupt_in(0x81, 64)
        .expect("interrupt endpoint released when the port closes");
}

#[test]
fn malformed_serial_state_notification_is_reported() {
    let fake = FakeTransport::cdc_iad();
    fake.push_interrupt_in(&[0xa1, 0x20, 0, 0, 0, 0, 2, 0, 1]);

    let mut port = open_on(&fake, 0);
    let deadline = Instant::now() + Duration::from_secs(1);
    let error = loop {
        match port.modem_status() {
            Ok(_) => {
                assert!(
                    Instant::now() < deadline,
                    "malformed notification was not reported"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            Err(error) => break error,
        }
    };
    assert!(error.to_string().contains("expected 10 bytes, got 9"));
}
