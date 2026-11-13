//! Streaming event types (from [`serialplugin_core::events`]) and the plugin's runtime capabilities.

use serde::Serialize;
pub use serialplugin_core::events::*;

/// Runtime information about the native plugin build (no `window` probing).
#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub transport: &'static str,
    pub platform: &'static str,
    pub version: &'static str,
}

impl Capabilities {
    pub fn current() -> Self {
        Self {
            transport: if cfg!(mobile) { "mobile" } else { "desktop" },
            platform: current_platform(),
            version: env!("CARGO_PKG_VERSION"),
        }
    }
}

const fn current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "android") {
        "android"
    } else if cfg!(target_os = "ios") {
        "ios"
    } else {
        "unknown"
    }
}
