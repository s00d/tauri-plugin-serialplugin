//! RX hub layer (shared state, desktop poll, Android push).

#[cfg(desktop)]
pub mod desktop;
pub mod handle;
#[cfg(mobile)]
pub mod mobile;
pub mod shared;

#[cfg(desktop)]
pub use desktop::PortRxHub;
pub use handle::RxHubHandle;
#[cfg(mobile)]
pub use mobile::MobileRxHub;
pub use shared::{
    emit_urc, ExchangeWaiter, HubRoutingState, LineRouter, RxHubShared, RxRouteAction,
};
