//! Cancel in-flight exchange on physical or CMUX virtual ports.

use crate::cmux::CmuxSession;
use crate::port::tx_queue::PortTxQueue;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Cancel in-flight exchange on a physical port (hub waiter + TX queue).
pub fn cancel_physical_exchange(
    exchange_cancel: &AtomicBool,
    tx_queue: &PortTxQueue,
    hub_cancel: impl FnOnce(),
) {
    exchange_cancel.store(true, Ordering::SeqCst);
    hub_cancel();
    tx_queue.cancel_all();
    tx_queue.clear_halt();
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
    tx_queue.clear_halt();
}

/// Type-erased hub cancel callback for physical ports.
pub type HubCancelFn = Box<dyn FnOnce() + Send>;

/// Cancel physical exchange when hub cancel is optional (poisoned lock, etc.).
pub fn cancel_physical_exchange_optional(
    exchange_cancel: &Arc<AtomicBool>,
    tx_queue: &Arc<PortTxQueue>,
    hub_cancel: Option<HubCancelFn>,
) {
    exchange_cancel.store(true, Ordering::SeqCst);
    if let Some(cancel) = hub_cancel {
        cancel();
    }
    tx_queue.cancel_all();
    tx_queue.clear_halt();
}
