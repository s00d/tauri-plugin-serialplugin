//! Tauri SerialPort facade and platform entrypoints.

pub mod backend;
#[cfg(desktop)]
pub mod desktop;
#[cfg(mobile)]
pub mod mobile;
pub mod serial_port;
