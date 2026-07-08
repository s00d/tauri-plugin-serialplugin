//! RX hub layer (shared state + poll loop on all platforms).

pub mod desktop;
pub mod handle;
pub mod shared;

pub use desktop::PortRxHub;
pub use handle::RxHubHandle;
pub use shared::{
    emit_urc, ExchangeWaiter, HubRoutingState, LineRouter, RxHubShared, RxRouteAction,
};
