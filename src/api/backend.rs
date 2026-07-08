//! Platform I/O backend trait and AT session configuration helpers.

use crate::at::session::AtSessionConfig;
use crate::error::Error;
use crate::port::tx_queue::PortTxQueue;
use std::sync::Mutex;

pub use crate::exchange::io::{ExchangeIo, PortBackend};

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
