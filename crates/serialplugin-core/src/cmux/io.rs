//! Physical port write abstraction (unified `serialport::SerialPort` bearer).
#[cfg(target_os = "ios")]
use crate::serialport;

use serialport::SerialPort;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Writes raw bytes to the physical CMUX bearer.
pub trait CmuxPhysicalIo: Send + Sync {
    fn write_all(&self, data: &[u8]) -> Result<(), String>;

    /// How long a write of `len` bytes may take; also how long a channel write waits
    /// out flow control (FCoff, MSC FC) before it fails. Default 1 s.
    fn write_timeout(&self, len: usize) -> Duration {
        let _ = len;
        Duration::from_secs(1)
    }
}

pub struct SerialPortIo(pub Arc<Mutex<Box<dyn SerialPort>>>);

impl CmuxPhysicalIo for SerialPortIo {
    /// Writes with a timeout from the wire time ([`write_timeout`]), then puts the
    /// port's own timeout back.
    fn write_all(&self, data: &[u8]) -> Result<(), String> {
        let mut port = self
            .0
            .lock()
            .map_err(|e| format!("port lock failed: {e}"))?;
        let restore = port.timeout();
        let timeout = write_timeout(&**port, data.len());
        let _ = port.set_timeout(timeout);
        let result = port
            .write_all(data)
            .map_err(|e| format!("cmux write failed: {e}"));
        let _ = port.set_timeout(restore);
        result
    }

    fn write_timeout(&self, len: usize) -> Duration {
        match self.0.lock() {
            Ok(port) => write_timeout(&**port, len),
            Err(_) => Duration::from_secs(1),
        }
    }
}

/// Twice the wire time of `len` bytes at the port's baud rate and character format
/// (start + data + parity + stop bits), plus 1 s: the margin plain-port writes get.
fn write_timeout(port: &dyn SerialPort, len: usize) -> Duration {
    use serialport::{DataBits, Parity, StopBits};
    let data = match port.data_bits() {
        Ok(DataBits::Five) => 5,
        Ok(DataBits::Six) => 6,
        Ok(DataBits::Seven) => 7,
        _ => 8,
    };
    let parity = u64::from(matches!(port.parity(), Ok(Parity::Odd | Parity::Even)));
    let stop = if matches!(port.stop_bits(), Ok(StopBits::Two)) {
        2
    } else {
        1
    };
    let bits = 1 + data + parity + stop;
    let baud = u64::from(port.baud_rate().unwrap_or(9600).max(1));
    let wire_ms = (len as u64).saturating_mul(bits * 1000) / baud;
    Duration::from_millis(wire_ms.saturating_mul(2) + 1000)
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use crate::mock_serial::MockSerialPort;

    #[test]
    fn cmux_write_timeout_scales_with_baud() {
        let mut port = MockSerialPort::new(); // 9600 8N1: 10 bits per char
                                              // 960 bytes = 1 s on the wire: twice that plus 1 s.
        assert_eq!(write_timeout(&port, 960), Duration::from_millis(3000));
        port.baud_rate = 115_200;
        assert_eq!(
            write_timeout(&port, 960),
            Duration::from_millis(2 * 83 + 1000)
        );
        port.baud_rate = 9600;
        port.parity = serialport::Parity::Even;
        port.stop_bits = serialport::StopBits::Two;
        assert_eq!(
            write_timeout(&port, 960),
            Duration::from_millis(2 * 1200 + 1000)
        );
        port.baud_rate = 1200;
        assert_eq!(
            write_timeout(&port, 9600),
            Duration::from_millis(2 * 96_000 + 1000)
        );

        // The write runs with that timeout and puts the port's own one back.
        let mock = MockSerialPort::new();
        let tx = mock.captured_tx.clone();
        let shared: Arc<Mutex<Box<dyn SerialPort>>> = Arc::new(Mutex::new(Box::new(mock)));
        SerialPortIo(shared.clone()).write_all(b"frame").unwrap();
        assert_eq!(
            shared.lock().unwrap().timeout(),
            Duration::from_millis(1000)
        );
        assert_eq!(tx.lock().unwrap().as_slice(), b"frame");
        // The trait method on the port gives the same timeout.
        assert_eq!(
            SerialPortIo(shared).write_timeout(960),
            Duration::from_millis(3000)
        );
    }

    #[test]
    fn cmux_write_timeout_counts_the_data_bits() {
        let mut port = MockSerialPort::new(); // 9600 baud, no parity, 1 stop bit
        for (bits, per_char) in [
            (serialport::DataBits::Five, 7u64),
            (serialport::DataBits::Six, 8),
            (serialport::DataBits::Seven, 9),
            (serialport::DataBits::Eight, 10),
        ] {
            port.data_bits = bits;
            // 9600 bytes: per_char seconds on the wire, twice that plus 1 s.
            assert_eq!(
                write_timeout(&port, 9600),
                Duration::from_millis(2 * per_char * 1000 + 1000),
                "{bits:?}"
            );
        }
    }

    #[test]
    fn default_write_timeout_is_one_second() {
        struct Plain;
        impl CmuxPhysicalIo for Plain {
            fn write_all(&self, _data: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        assert_eq!(Plain.write_timeout(10_000), Duration::from_secs(1));
    }
}
