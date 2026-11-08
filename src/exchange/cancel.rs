//! Cancel in-flight exchange on physical or CMUX virtual ports.

use crate::cmux::CmuxSession;
use crate::port::tx_queue::PortTxQueue;
use std::sync::atomic::{AtomicBool, Ordering};

/// Cancel in-flight exchange on a physical port (hub waiter + TX queue).
pub fn cancel_physical_exchange(
    exchange_cancel: &AtomicBool,
    tx_queue: &PortTxQueue,
    hub_cancel: impl FnOnce(),
) {
    exchange_cancel.store(true, Ordering::SeqCst);
    hub_cancel();
    tx_queue.cancel_all();
}

/// Cancel in-flight exchange on a CMUX virtual channel.
pub fn cancel_virtual_exchange(
    exchange_cancel: &AtomicBool,
    tx_queue: &PortTxQueue,
    session: &CmuxSession,
    dlci: u8,
) {
    exchange_cancel.store(true, Ordering::SeqCst);
    session.cancel_active_exchange(dlci);
    tx_queue.cancel_all();
}
