//! Shared RX hub facade (poll hub on desktop and Android).

use crate::cmux::CmuxSession;
use crate::events::SerialEvent;
use crate::hub::shared::{ExchangeWaiter, RxHubShared};
use std::sync::Arc;
use tauri::ipc::Channel;

/// Platform-neutral RX hub API used by exchange and watch paths.
pub trait RxHubHandle: Send + Sync {
    fn shared(&self) -> Arc<RxHubShared>;
    fn set_exchange_waiter(&self, waiter: Arc<ExchangeWaiter>);
    fn clear_exchange_waiter(&self);
    fn cancel_active_exchange(&self);
    fn attach_watch(&self, channel: Channel<SerialEvent>, batch_timeout_ms: u64, read_size: usize);
    fn detach_watch(&self);
    fn attach_cmux(&self, session: Arc<CmuxSession>);
    fn detach_cmux(&self);
    /// Stop background RX processing.
    fn shutdown_hub(&self) {}
}
