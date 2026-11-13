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

#[cfg(test)]
#[cfg(not(target_os = "android"))]
mod tests {
    use super::*;
    use crate::hub::ExchangeWaiter;
    use std::sync::Arc;

    struct NoopIo;
    impl crate::cmux::CmuxPhysicalIo for NoopIo {
        fn write_all(&self, _data: &[u8]) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn virtual_cancel_fails_the_channel_waiter() {
        let session = CmuxSession::new("p".into(), Arc::new(NoopIo));
        session.register_dlci(2, "p#dlci=2".into());
        let waiter = ExchangeWaiter::new(
            crate::events::ExchangeOptions::default().resolve(),
            Arc::new(AtomicBool::new(false)),
        );
        session.set_exchange_waiter(2, waiter.clone());
        let flag = AtomicBool::new(false);
        cancel_virtual_exchange(&flag, &PortTxQueue::new(), &session, 2);
        assert!(flag.load(Ordering::SeqCst));
        assert_eq!(waiter.wait(2_000).unwrap_err(), "exchange cancelled");
    }
}
