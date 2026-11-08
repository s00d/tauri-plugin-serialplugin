//! [`SerialManager`] and the helpers it routes exchanges through.

pub mod backend;
mod details;
mod lines;
pub mod serial;
pub mod serial_port;

pub use serial::SerialManager;
