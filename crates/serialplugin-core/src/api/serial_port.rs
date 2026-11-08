//! Shared SerialPort helpers (exchange routing, AT session config).

use crate::api::backend::configure_at_session_on_path;
use crate::at::session::AtSessionConfig;
use crate::cmux::parse_mux_path;
use crate::error::Error;
use crate::events::ExchangeOptions;
use crate::port::tx_queue::PortTxQueue;
use crate::state::VirtualPortRef;
use std::sync::{Arc, Mutex};

/// Resolve tx queue for physical or virtual path.
pub fn get_tx_queue_from_maps(
    virtual_ports: &Mutex<std::collections::HashMap<String, VirtualPortRef>>,
    physical_tx_queue: impl FnOnce(&str) -> Result<Arc<PortTxQueue>, Error>,
    path: &str,
) -> Result<Arc<PortTxQueue>, Error> {
    if let Ok(virtuals) = virtual_ports.lock() {
        if let Some(vp) = virtuals.get(path) {
            return Ok(vp.tx_queue.clone());
        }
    }
    physical_tx_queue(path)
}

/// Route exchange through mux virtual path or physical port queue.
pub fn run_exchange_queued(
    path: String,
    options: ExchangeOptions,
    get_tx_queue: impl FnOnce(&str) -> Result<Arc<PortTxQueue>, Error>,
    mux_direct: impl FnOnce(
        String,
        u8,
        String,
        Vec<u8>,
        ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error>,
    physical_direct: impl FnOnce(
        String,
        Vec<u8>,
        ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error>,
    payload: Vec<u8>,
) -> Result<crate::at::parse::ExchangeResponse, Error> {
    if let Some((physical, dlci)) = parse_mux_path(path.as_str()) {
        let physical_path = physical.to_string();
        let tx_queue = get_tx_queue(path.as_str())?;
        return tx_queue.run_serial(|| mux_direct(physical_path, dlci, path, payload, options));
    }
    let tx_queue = get_tx_queue(&path)?;
    tx_queue.run_serial(|| physical_direct(path, payload, options))
}

/// Configure AT session defaults for native `at` jobs on this port.
pub fn configure_at_session(
    virtual_ports: &Mutex<std::collections::HashMap<String, VirtualPortRef>>,
    physical_tx_queue: Option<Arc<PortTxQueue>>,
    path: &str,
    session: AtSessionConfig,
) -> Result<(), Error> {
    configure_at_session_on_path(virtual_ports, physical_tx_queue, path, session)
}
