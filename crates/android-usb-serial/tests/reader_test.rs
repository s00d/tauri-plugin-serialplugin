#![cfg(feature = "fake-transport")]

//! Background reader semantic tests.

use android_usb_serial::config::{DataBits, LineConfig, Parity, StopBits};
use android_usb_serial::device::open_port;
use android_usb_serial::error::{ReadOutcome, UsbSerialError};
use android_usb_serial::fake::FakeTransport;
use android_usb_serial::reader::SerialReader;
use android_usb_serial::rx_filter::{FtdiHeaderFilter, XonXoffRxFilter};
use android_usb_serial::transport::{BulkIn, EndpointInfo, InterfaceInfo, Transport};
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};

fn cdc_dual_iface_fake() -> FakeTransport {
    let fake = FakeTransport::cdc_single_iface();
    fake.set_interfaces(vec![
        InterfaceInfo {
            id: 0,
            class: 2,
            subclass: 2,
            protocol: 0,
        },
        InterfaceInfo {
            id: 1,
            class: 10,
            subclass: 0,
            protocol: 0,
        },
    ]);
    fake.configure_endpoints(&[(
        1,
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
    fake
}

#[test]
fn write_after_start_reader_does_not_reopen_bulk_in() {
    let fake = cdc_dual_iface_fake();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).expect("open");
    port.start_reader().expect("start_reader");
    // Regression: write used to call ensure_open() which re-opened bulk IN
    // already owned by SerialReader → nusb "endpoint already in use".
    assert_eq!(port.write(b"AT\r").expect("write"), 3);
    port.close();
}

#[test]
fn port_start_reader_without_prior_write() {
    let fake = cdc_dual_iface_fake();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).expect("open");
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })
    .expect("line");
    port.start_reader().expect("start_reader");
    fake.push_rx(b"ok");
    let mut buf = [0u8; 4];
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut n = 0;
    while n == 0 && Instant::now() < deadline {
        n = port.try_read(&mut buf).expect("read");
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    assert_eq!(n, 2);
}

#[test]
fn reader_chunk_order_preserved() {
    let fake = FakeTransport::cdc_single_iface();
    fake.push_rx(b"AB");
    fake.push_rx(b"C");
    let bulk = fake.open_bulk_in(0x81, 64).expect("bulk in");
    let mut reader = SerialReader::start(bulk, 64, 100, vec![]);

    let deadline = Instant::now() + Duration::from_secs(2);
    let mut out = [0u8; 8];
    let mut total = 0usize;
    while total < 3 && Instant::now() < deadline {
        let n = reader.try_read(&mut out[total..]).expect("read");
        total += n;
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    reader.stop();
    assert_eq!(total, 3);
    assert_eq!(&out[..3], b"ABC");
}

#[test]
fn ftdi_filter_in_reader_path() {
    let fake = FakeTransport::ftdi_ft232r();
    fake.push_rx(&[0u8, 0u8, b'X']);
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
    assert_eq!(n, 1);
    assert_eq!(out[0], b'X');
}

#[test]
fn xonxoff_filter_in_reader_path() {
    let fake = FakeTransport::cdc_single_iface();
    fake.push_rx(&[b'A', 19, b'B']);
    let bulk = fake.open_bulk_in(0x81, 64).expect("bulk in");
    let mut reader = SerialReader::start(bulk, 64, 100, vec![Box::new(XonXoffRxFilter::new(true))]);
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
    assert_eq!(&out[..n], b"AB");
}

struct ErrorOnceBulkIn {
    calls: Mutex<u32>,
}

impl BulkIn for ErrorOnceBulkIn {
    fn read(
        &mut self,
        _buf: &mut [u8],
        _timeout_ms: u32,
    ) -> android_usb_serial::Result<ReadOutcome> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        Err(UsbSerialError::Io("usb disconnect".into()))
    }

    fn cancel_all(&mut self) {}

    fn clear_halt(&mut self) -> android_usb_serial::Result<()> {
        Ok(())
    }
}

#[test]
fn disconnected_emits_single_on_error() {
    let bulk = ErrorOnceBulkIn {
        calls: Mutex::new(0),
    };
    let mut reader = SerialReader::start(Box::new(bulk), 64, 100, vec![]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut out = [0u8; 4];
    let mut err = None;
    while Instant::now() < deadline {
        match reader.try_read(&mut out) {
            Ok(0) => std::thread::sleep(Duration::from_millis(5)),
            Ok(_) => break,
            Err(e) => {
                err = Some(e);
                break;
            }
        }
    }
    reader.stop();
    assert!(err.is_some());
}

#[test]
fn stop_reader_join_under_1s() {
    let fake = FakeTransport::cdc_single_iface();
    let bulk = fake.open_bulk_in(0x81, 64).expect("bulk in");
    let mut reader = SerialReader::start(bulk, 64, 100, vec![]);
    let start = Instant::now();
    reader.stop();
    assert!(start.elapsed() < Duration::from_secs(1));
}
