//! Tauri `SerialPort` state and the core helpers it routes through.

pub use serialplugin_core::api::{backend, serial_port};
pub mod serial;

pub use serial::SerialPort;
