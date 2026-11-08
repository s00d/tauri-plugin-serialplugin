//! RX hub layer (shared state + poll loop on all platforms).

pub(crate) mod delivery;
pub mod desktop;
pub mod handle;
pub mod shared;

pub use desktop::PortRxHub;
pub use handle::RxHubHandle;
#[cfg(feature = "tauri")]
pub use shared::emit_urc;
pub use shared::{
    emit_urc_to, ExchangeWaiter, HubRoutingState, LineRouter, RxHubShared, RxRouteAction,
};
