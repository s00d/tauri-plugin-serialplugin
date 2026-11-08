//! Shared RX hub facade (poll hub on desktop and Android).

use crate::cmux::CmuxSession;
use crate::events::EventSink;
use crate::hub::shared::{ExchangeWaiter, RxHubShared};
use serial_codec::framing::Framer;
use std::sync::Arc;

/// Platform-neutral RX hub API used by exchange and watch paths.
pub trait RxHubHandle: Send + Sync {
    fn shared(&self) -> Arc<RxHubShared>;
    fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>);
    fn clear_exchange_waiter(&self);
    fn cancel_active_exchange(&self);
    fn attach_watch(
        &self,
        channel: EventSink,
        batch_timeout_ms: u64,
        read_size: usize,
        route_urc: bool,
        framer: Option<Box<dyn Framer>>,
    );
    fn detach_watch(&self);
    fn attach_cmux(&self, session: Arc<CmuxSession>);
    fn detach_cmux(&self);
    /// Stop background RX processing.
    fn shutdown_hub(&self) {}
}
