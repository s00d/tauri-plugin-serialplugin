//! The plugin through Tauri IPC, as a webview calls it (TESTING.md). Desktop only: the tests
//! use PTYs, TCP sockets and the `serialport` crate.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

mod common;

mod model_device;
mod model_offline;

mod compat_api;
mod story_acl;
mod story_config;
mod story_exchange;
mod story_modem;
mod story_ports;
mod story_watch;
