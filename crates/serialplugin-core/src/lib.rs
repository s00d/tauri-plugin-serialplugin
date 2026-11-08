//! Serial port core of `tauri-plugin-serialplugin`, usable from plain Rust (no Tauri).
//!
//! [`SerialManager`] owns every open port: one RX hub thread per port, watch streams
//! with native framing ([`events::WatchOptions::parser`]), request/response
//! [`exchange`](SerialManager::exchange), queued AT commands, GSM 07.10 CMUX virtual
//! channels, `tcp://host:port` bridges and (feature `ble`) Bluetooth LE serial.
//! Events are delivered to an [`EventSink`] closure you pass to
//! [`SerialManager::watch`].
//!
//! New in 4.2 (core only, the 4.x API is unchanged): methods returning [`CoreError`]
//! with an [`ErrorKind`] (`open_core`, `write_core`, `exchange_core`, …), any number
//! of [`SerialManager::watch_core`] streams of [`CoreEvent`]s with bounded queues,
//! port-state subscriptions ([`SerialManager::subscribe_core`]) and
//! [`SerialManager::insert_port`] for your own `SerialPort` (a fake in tests).
//!
//! ```no_run
//! use serialplugin_core::{events::{SerialEvent, WatchOptions}, EventSink, SerialManager};
//! use std::sync::Arc;
//!
//! let serial = SerialManager::new();
//! let path = serial.open("/dev/ttyUSB0".into(), 115_200, None, None, None, None, None)?;
//! let sink: EventSink = Arc::new(|event| {
//!     if let SerialEvent::Data { data, .. } = event {
//!         println!("{}", String::from_utf8_lossy(&data));
//!     }
//!     true // keep the subscription
//! });
//! let watch_id = serial.watch(path.clone(), WatchOptions::default(), sink)?;
//! let reply = serial.at(path.clone(), "AT+CSQ".into(), None)?;
//! println!("{:?}", reply.status);
//! serial.unwatch(watch_id)?;
//! serial.close(path)?;
//! # Ok::<(), serialplugin_core::error::Error>(())
//! ```

/// Logging macros that respect the global log level ([`state::set_log_level`]).
pub mod logger;

#[cfg(any(test, target_os = "android"))]
mod jni_ready;

#[cfg(target_os = "android")]
pub mod android;
pub mod api;
pub mod at;
pub mod cmux;
pub mod error;
pub mod events;
pub mod exchange;
pub mod hub;
pub mod port;
pub mod state;
pub mod sync_util;

#[cfg(target_os = "ios")]
#[path = "serialport_ios.rs"]
pub mod serialport;

#[cfg(any(not(target_os = "android"), debug_assertions, feature = "test-util"))]
pub mod mock_serial;

/// The `log` crate, for the logging macros (feature `log`, and always on Android).
#[doc(hidden)]
#[cfg(any(feature = "log", target_os = "android"))]
pub use log as __log;

pub use api::SerialManager;
pub use error::{CoreError, Error, ErrorKind};
pub use events::{CoreEvent, CoreSink, EventSink, PortListSink, SubscriptionId};
