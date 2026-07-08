//! Pure Rust USB serial drivers for Android (and Linux), built on [nusb](https://docs.rs/nusb).
//!
//! Protocol logic is ported from
//! [usb-serial-for-android](https://github.com/mik3y/usb-serial-for-android) and checked against
//! golden USB control-transfer fixtures under `tests/fixtures/`.
//!
//! ## Overview
//!
//! This crate does **not** talk to Android `UsbManager`. The host app (typically Kotlin) owns
//! USB permission, opens a `UsbDeviceConnection`, and passes its raw file descriptor into Rust.
//! The crate then:
//!
//! 1. [`from_raw_fd`] — `dup`s the fd so Java can keep the connection alive.
//! 2. [`NusbTransport`] — claims interfaces via nusb (`detach_and_claim`).
//! 3. [`ProbeTable`] / [`open_port`] — selects a vendor driver and returns a [`SerialPortHandle`].
//!
//! ```text
//! UsbManager (Kotlin) → permission → UsbDeviceConnection → fd (unclaimed)
//!         ↓
//!    from_raw_fd / NusbTransport
//!         ↓
//!    ProbeTable → Ftdi / Cp21xx / Ch34x / … drivers
//!         ↓
//!    SerialPortHandle (write / reader / modem / purge)
//! ```
//!
//! ## Android usage
//!
//! - Declare `android.hardware.usb.host` and a `device_filter.xml` in the app.
//! - Request runtime USB permission before `UsbManager.openDevice()`.
//! - **Do not** call `UsbDeviceConnection.claimInterface()` in Kotlin — nusb claims after
//!   [`from_raw_fd`]. Pre-claim causes `io interface is busy`.
//! - Keep the `UsbDeviceConnection` open for the whole session; close it only after Rust
//!   [`SerialPortHandle::close`].
//! - Prefer [`SerialPortHandle::start_reader`] **after** line config and DTR/RTS (important for
//!   weak OTG / CH340).
//! - Multi-port chips: pass `port_index` to [`open_port`] (`0`, `1`, …). App-level enumerate
//!   often exposes paths as `deviceName` / `deviceName#N`.
//!
//! Full Kotlin/permission walkthrough: crate README (*Using on Android*) in the repository.
//!
//! ## Quick start (real USB fd)
//!
//! ```ignore
//! // fd from UsbDeviceConnection.fileDescriptor (dup'd inside from_raw_fd)
//! use android_usb_serial::{from_raw_fd, open_port, NusbTransport, Transport};
//! use std::sync::Arc;
//!
//! let device = from_raw_fd(fd)?;
//! let transport = Arc::new(NusbTransport::from_device(device)?) as Arc<dyn Transport>;
//! let mut port = open_port(transport, 0)?;
//! port.write(b"AT\r\n")?;
//! ```
//!
//! ## Quick start (`fake-transport`)
//!
//! ```
//! # #[cfg(feature = "fake-transport")]
//! # {
//! use android_usb_serial::{open_port, FakeTransport, Transport};
//! use std::sync::Arc;
//!
//! let fake = FakeTransport::cdc_single_iface();
//! let transport: Arc<dyn Transport> = Arc::new(fake.clone());
//! let mut port = open_port(transport, 0).unwrap();
//! port.write(b"PING").unwrap();
//! assert_eq!(fake.take_tx(), b"PING");
//! # }
//! ```
//!
//! ## Features
//!
//! | Feature | Default | Description |
//! |---------|---------|-------------|
//! | `serialport-compat` | yes | [`serialport::SerialPort`] adapter ([`serialport_compat`]) |
//! | `fake-transport` | no | [`FakeTransport`] + `golden_record` binary |
//!
//! ## Platform notes
//!
//! - **Android / Linux:** real USB via nusb ([`from_raw_fd`], [`NusbTransport`]).
//! - **Other hosts:** drivers + [`fake`] for tests; supply your own [`Transport`]
//!   for hardware.

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
