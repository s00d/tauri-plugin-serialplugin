//! Physical port write abstraction (unified `serialport::SerialPort` bearer).

use serialport::SerialPort;
use std::io::Write;
use std::sync::{Arc, Mutex};

/// Writes raw bytes to the physical CMUX bearer.
pub trait CmuxPhysicalIo: Send + Sync {
    fn write_all(&self, data: &[u8]) -> Result<(), String>;
}

pub struct SerialPortIo(pub Arc<Mutex<Box<dyn SerialPort>>>);

impl CmuxPhysicalIo for SerialPortIo {
    fn write_all(&self, data: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|e| format!("port lock failed: {e}"))?
            .write_all(data)
            .map_err(|e| format!("cmux write failed: {e}"))
    }
}
