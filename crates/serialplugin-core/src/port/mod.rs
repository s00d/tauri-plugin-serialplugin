//! Port enumeration, monitoring, TX queue, and watch registry.

#[cfg(all(feature = "ble", not(target_os = "android")))]
pub mod ble;
/// Android BLE (Kotlin GATT bridge); compiled on the host too so its tests run there.
#[cfg(any(all(feature = "ble", target_os = "android"), test))]
pub mod ble_android;
#[cfg(any(feature = "ble", test))]
pub mod ble_rx;
pub mod list;
pub mod list_monitor;
pub mod tcp;
pub mod tx_queue;
pub mod watch_registry;
