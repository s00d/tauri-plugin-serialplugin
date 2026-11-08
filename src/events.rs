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

/// Forward core events to a Tauri IPC channel (serialized to JSON per event).
/// The sink reports `false` once the webview side of the channel is gone.
pub(crate) fn channel_sink<E>(
    channel: tauri::ipc::Channel<E>,
) -> std::sync::Arc<dyn Fn(E) -> bool + Send + Sync>
where
    E: tauri::ipc::IpcResponse + Send + Sync + 'static,
{
    std::sync::Arc::new(move |event| channel.send(event).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_has_version() {
        let caps = Capabilities::current();
        assert!(!caps.version.is_empty());
        assert!(caps.transport == "desktop" || caps.transport == "mobile");
    }
}
