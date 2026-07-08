//! Pure Rust USB serial drivers for Android (and Linux), built on [nusb](https://docs.rs/nusb).
//!
//! ## Overview
//!
//! Android apps obtain a `UsbDeviceConnection` file descriptor from
//! `UsbManager` / Kotlin, then pass it into this crate via [`from_raw_fd`].
//! Drivers (FTDI, CP21xx, CH34x, Prolific, CDC-ACM, …) speak vendor USB
//! protocols over that transport.
//!
//! ```ignore
//! // Android / Linux: fd from UsbDeviceConnection.fileDescriptor (already dup'd internally)
//! use android_usb_serial::{from_raw_fd, open_port, NusbTransport, Transport};
//! use std::sync::Arc;
//!
//! let device = from_raw_fd(fd)?;
//! let transport = Arc::new(NusbTransport::from_device(device)?) as Arc<dyn Transport>;
//! let mut port = open_port(transport, 0)?;
//! port.write(b"AT\r\n")?;
//! ```
//!
//! Host-side tests can use the optional [`fake::FakeTransport`] behind the
//! `fake-transport` feature.
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `serialport-compat` | yes | `serialport::SerialPort` adapter |
//! | `fake-transport` | no | In-memory transport + `golden_record` binary |
//!
//! ## Platform notes
//!
//! - **Android / Linux:** full USB path via `nusb` (`from_raw_fd`, [`NusbTransport`]).
//! - **Other hosts:** compile drivers + [`fake`](crate::fake) for tests; no real USB until you supply a [`Transport`].

#![cfg_attr(docsrs, feature(doc_cfg))]

/// Line / flow / purge configuration types.
pub mod config;
/// Device probe and port open helpers.
pub mod device;
/// Chip-specific USB serial drivers.
pub mod drivers;
/// Error types.
pub mod error;
/// High-level serial port handle.
pub mod port;
/// VID/PID probe table (ported from usb-serial-for-android).
pub mod probe;
/// Continuous bulk-IN reader thread.
pub mod reader;
/// RX filter chain (FTDI header strip, XON/XOFF).
pub mod rx_filter;
/// USB transport trait and request types.
pub mod transport;
/// XON/XOFF inline filter.
pub mod xonxoff;

#[cfg(feature = "fake-transport")]
#[cfg_attr(docsrs, doc(cfg(feature = "fake-transport")))]
/// In-memory [`Transport`](crate::transport::Transport) for golden parity and harnesses.
pub mod fake;

#[cfg(any(target_os = "android", target_os = "linux"))]
#[cfg_attr(docsrs, doc(cfg(any(target_os = "android", target_os = "linux"))))]
/// `nusb`-backed transport (`from_raw_fd` / Android `UsbDeviceConnection`).
pub mod nusb_transport;

#[cfg(feature = "serialport-compat")]
#[cfg_attr(docsrs, doc(cfg(feature = "serialport-compat")))]
/// Adapter implementing [`serialport::SerialPort`].
pub mod serialport_compat;

pub use config::*;
pub use device::{describe_device, open_port, DeviceDescriptor, PortDescriptor};
pub use drivers::ModemStatus;
pub use error::{ReadOutcome, Result, TransferError, UsbSerialError};
pub use port::SerialPortHandle;
pub use probe::{DriverType, ProbeTable};
pub use transport::{
    BulkIn, BulkOut, ControlRequest, EndpointInfo, InterfaceInfo, SharedTransport, Transport,
};

#[cfg(feature = "fake-transport")]
#[cfg_attr(docsrs, doc(cfg(feature = "fake-transport")))]
pub use fake::FakeTransport;

#[cfg(any(target_os = "android", target_os = "linux"))]
#[cfg_attr(docsrs, doc(cfg(any(target_os = "android", target_os = "linux"))))]
pub use nusb_transport::{from_raw_fd, NusbTransport};
