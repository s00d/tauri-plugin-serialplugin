//! Platform I/O backend trait and AT session configuration helpers.

use crate::at::session::AtSessionConfig;
use crate::error::Error;
use crate::port::tx_queue::PortTxQueue;
#[cfg(mobile)]
use crate::state::ClearBuffer;
use std::sync::Mutex;

pub use crate::exchange::io::{ExchangeIo, PortBackend};

#[cfg(mobile)]
pub struct MobileBackend {
    io: crate::android::usb_io::MobileUsbIo,
}

#[cfg(mobile)]
impl MobileBackend {
    pub fn new(io: crate::android::usb_io::MobileUsbIo) -> Self {
        Self { io }
    }

    pub fn write_physical(&self, path: &str, data: &[u8]) -> Result<(), Error> {
        self.io.write(path, data).map(|_| ())
    }

    pub fn purge_rx(&self, path: &str) -> Result<(), Error> {
        self.io.clear_buffer(path, ClearBuffer::Input)
    }
}

/// Configure AT session on physical or virtual path.
pub fn configure_at_session_on_path(
    virtual_ports: &Mutex<std::collections::HashMap<String, crate::state::VirtualPortRef>>,
    physical_tx_queue: Option<std::sync::Arc<PortTxQueue>>,
    path: &str,
    session: AtSessionConfig,
) -> Result<(), Error> {
    if let Ok(v) = virtual_ports.lock() {
        if let Some(vp) = v.get(path) {
            vp.tx_queue.configure_at_session(session);
            return Ok(());
        }
    }
    if let Some(q) = physical_tx_queue {
        q.configure_at_session(session);
        return Ok(());
    }
    Err(Error::String(format!("Port '{}' not found", path)))
}
