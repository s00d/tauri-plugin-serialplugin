#![cfg(feature = "fake-transport")]

//! Stall recovery in reader loop.

use android_usb_serial::error::{ReadOutcome, TransferError, UsbSerialError};
use android_usb_serial::reader::SerialReader;
use android_usb_serial::transport::BulkIn;
use std::sync::Mutex;
use std::time::{Duration, Instant};

struct StallOnceBulkIn {
    calls: Mutex<u32>,
}

impl BulkIn for StallOnceBulkIn {
    fn read(
        &mut self,
        _buf: &mut [u8],
        _timeout_ms: u32,
    ) -> android_usb_serial::Result<ReadOutcome> {
        let mut calls = self.calls.lock().unwrap();
        *calls += 1;
        if *calls == 1 {
            return Err(UsbSerialError::from(TransferError::Stall));
        }
        Ok(ReadOutcome::Data(vec![b'X']))
    }

    fn cancel_all(&mut self) {}

    fn clear_halt(&mut self) -> android_usb_serial::Result<()> {
        Ok(())
    }
}

#[test]
fn reader_recovers_after_single_stall() {
    let bulk = StallOnceBulkIn {
        calls: Mutex::new(0),
    };
    let mut reader = SerialReader::start(Box::new(bulk), 64, 100, vec![]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut out = [0u8; 4];
    let mut n = 0;
    while n == 0 && Instant::now() < deadline {
        n = reader.try_read(&mut out).unwrap_or(0);
        if n == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    reader.stop();
    assert_eq!(n, 1);
    assert_eq!(out[0], b'X');
}

#[test]
fn reader_double_stall_surfaces_error() {
    struct StallTwiceBulkIn;
    impl BulkIn for StallTwiceBulkIn {
        fn read(
            &mut self,
            _buf: &mut [u8],
            _timeout_ms: u32,
        ) -> android_usb_serial::Result<ReadOutcome> {
            Err(UsbSerialError::from(TransferError::Stall))
        }
        fn cancel_all(&mut self) {}
        fn clear_halt(&mut self) -> android_usb_serial::Result<()> {
            Ok(())
        }
    }
    let mut reader = SerialReader::start(Box::new(StallTwiceBulkIn), 64, 100, vec![]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut out = [0u8; 4];
    let mut got_err = false;
    while Instant::now() < deadline {
        if reader.try_read(&mut out).is_err() {
            got_err = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    reader.stop();
    assert!(got_err);
}
