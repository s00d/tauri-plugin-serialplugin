#![cfg(feature = "fake-transport")]

//! FTDI driver semantic tests on fake transport.

use android_usb_serial::config::{DataBits, LineConfig, Parity, PurgeKind, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::drivers::ftdi_baud_encoding;
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::reader::SerialReader;
use android_usb_serial::rx_filter::FtdiHeaderFilter;
use android_usb_serial::transport::Transport;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn ftdi_baud_control_value(fake: &FakeTransport) -> u16 {
    fake.recorded_controls()
        .into_iter()
        .find(|c| c.request == 3)
        .expect("SET_BAUDRATE")
        .value
}

fn open_ftdi(fake: &FakeTransport, port: usize) -> android_usb_serial::port::SerialPortHandle {
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut handle = open_port(transport, port).expect("open");
    handle
        .set_line_config(LineConfig {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        })
        .expect("line");
    handle
}

#[test]
fn baud_vector_300_0x2710() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 300,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x2710);
    let (value, _) = ftdi_baud_encoding(300, false, 0).unwrap();
    assert_eq!(value, 0x2710);
}

#[test]
fn baud_vector_9600_0x4138() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x4138);
}

#[test]
fn baud_vector_115200_0x001a() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x001a);
}

#[test]
fn baud_vector_921600_0x8003() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 921_600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x8003);
}

#[test]
fn baud_vector_2m_0x0001() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 2_000_000,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x0001);
}

#[test]
fn baud_vector_3m_0x0000() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 3_000_000,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    assert_eq!(ftdi_baud_control_value(&fake), 0x0000);
}

#[test]
fn baud_over_3_5m_errors() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    let err = port.set_line_config(LineConfig {
        baud_rate: 4_000_000,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    });
    assert!(err.is_err());
}

#[test]
fn ft2232_port1_index_encoding() {
    let fake = FakeTransport::ftdi_ft2232();
    let mut port = open_ftdi(&fake, 1);
    fake.clear_recorded();
    port.set_line_config(LineConfig {
        baud_rate: 9600,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("baud");
    let ctrl = fake
        .recorded_controls()
        .into_iter()
        .find(|c| c.request == 3)
        .expect("SET_BAUDRATE");
    assert_eq!(ctrl.index, 2, "port1 wIndex encoding");
    let _ = port;
}

#[test]
fn purge_rx_clears_write_buffer_naming() {
    let fake = FakeTransport::ftdi_ft232r();
    let mut port = open_ftdi(&fake, 0);
    fake.clear_recorded();
    port.purge(PurgeKind::Rx).expect("purge rx");
    let purge = fake
        .recorded_controls()
        .into_iter()
        .find(|c| c.request == 0 && c.value == 1)
        .expect("PURGE_RX");
    assert_eq!(purge.index, 1);
}

#[test]
fn read_filter_integration_via_reader() {
    let fake = FakeTransport::ftdi_ft232r();
    fake.push_rx(&[0u8, 0u8, b'H', b'i']);
    let bulk = fake.open_bulk_in(0x81, 64).expect("bulk in");
    let mut reader = SerialReader::start(bulk, 64, 100, vec![Box::new(FtdiHeaderFilter::new(64))]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut out = [0u8; 4];
    let mut n = 0;
    while n == 0 && Instant::now() < deadline {
        n = reader.try_read(&mut out).expect("read");
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    reader.stop();
    assert_eq!(n, 2);
    assert_eq!(&out[..2], b"Hi");
}
