//! The root crate's integration tests in one binary (`autotests = false`): the plugin
//! through Tauri IPC (mock runtime), as a webview calls it. Desktop only: the tests use
//! PTYs and the `serialport` crate, neither of which exists on Android or iOS.

#![cfg(not(any(target_os = "android", target_os = "ios")))]

mod common;

mod device_model;
mod offline_model;

mod acl;
mod api_compat;
mod config_stories;
mod exchange_stories;
mod port_stories;
mod watch_stories;

mod modem_live;
