//! Physical port write abstraction (desktop serial vs Android USB).

#[cfg(desktop)]
use serialport::SerialPort;
#[cfg(desktop)]
use std::io::Write;
#[cfg(target_os = "android")]
use std::sync::Arc;
#[cfg(desktop)]
use std::sync::{Arc, Mutex};

/// Writes raw bytes to the physical CMUX bearer.
pub trait CmuxPhysicalIo: Send + Sync {
    fn write_all(&self, data: &[u8]) -> Result<(), String>;
}

#[cfg(desktop)]
pub struct SerialPortIo(pub Arc<Mutex<Box<dyn SerialPort>>>);

#[cfg(desktop)]
impl CmuxPhysicalIo for SerialPortIo {
    fn write_all(&self, data: &[u8]) -> Result<(), String> {
        self.0
            .lock()
            .map_err(|e| format!("port lock failed: {e}"))?
            .write_all(data)
            .map_err(|e| format!("cmux write failed: {e}"))
    }
}

#[cfg(target_os = "android")]
pub struct MobileCmuxIo {
    write_fn: Box<dyn Fn(&[u8]) -> Result<(), String> + Send + Sync>,
}

#[cfg(target_os = "android")]
impl MobileCmuxIo {
    pub fn new<F>(write_fn: F) -> Arc<Self>
    where
        F: Fn(&[u8]) -> Result<(), String> + Send + Sync + 'static,
    {
        Arc::new(Self {
            write_fn: Box::new(write_fn),
        })
    }
}

#[cfg(target_os = "android")]
impl CmuxPhysicalIo for MobileCmuxIo {
    fn write_all(&self, data: &[u8]) -> Result<(), String> {
        (self.write_fn)(data)
    }
}
