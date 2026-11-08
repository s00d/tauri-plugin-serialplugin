//! Port enumeration, monitoring, TX queue, and watch registry.

#[cfg(all(feature = "ble", not(target_os = "android")))]
pub mod ble;
pub mod list;
pub mod list_monitor;
pub mod tcp;
pub mod tx_queue;
pub mod watch_registry;
