//! Streaming event types and runtime capabilities for v3.

use serde::{Deserialize, Serialize};

/// Options for [`crate::commands::watch_ports`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchPortsOptions {
    /// Same as [`crate::commands::available_ports`] `single_port_per_device`.
    #[serde(default)]
    pub single_port_per_device: Option<bool>,
    /// Poll interval on desktop (ms). Default: 2000. Android also polls as fallback.
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
}

/// Port list change events streamed through a Tauri [`tauri::ipc::Channel`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum PortListEvent {
    /// Initial/full list when subscribing or after reconnect.
    Snapshot {
        ports: std::collections::HashMap<String, std::collections::HashMap<String, String>>,
    },
    /// A serial device appeared in the system enumeration.
    Added {
        path: String,
        info: std::collections::HashMap<String, String>,
    },
    /// A serial device disappeared from the system enumeration.
    Removed { path: String },
}

/// Payload streamed to the frontend through a Tauri [`tauri::ipc::Channel`].
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SerialEvent {
    Data {
        path: String,
        data: Vec<u8>,
        size: usize,
    },
    Disconnect {
        path: String,
        reason: String,
    },
    Urc {
        path: String,
        line: String,
    },
    Error {
        path: String,
        message: String,
    },
}

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

/// Options for [`crate::commands::watch`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchOptions {
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub size: Option<usize>,
    #[serde(default)]
    pub serial_data_flush_interval_ms: Option<u64>,
}

/// How RX is prepared before sending an exchange payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum RxPrepareMode {
    /// Read until idle (default) — soft drain, preserves URC for parsing.
    #[default]
    Drain,
    /// Hardware/driver input buffer purge.
    Purge,
    /// Do not touch RX before write.
    None,
}

/// AT result line format (V.250 verbose vs numeric `ATV0`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AtResultFormat {
    #[default]
    Verbose,
    Numeric,
}

/// When an exchange read is considered complete.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeCompletionMode {
    /// Complete when the last non-empty line is OK / ERROR / +CME|CMS ERROR (default for AT).
    #[default]
    AtFinalLine,
    /// Complete when the last line is an intermediate result (`>`, `CONNECT`, numeric `1`).
    AtIntermediate,
    /// Legacy substring terminators anywhere in the buffer.
    Substring,
}

/// Options for [`crate::commands::exchange`] (write + read-until-response).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeOptions {
    /// Wall-clock limit for the whole exchange (ms). Default: 5000.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Maximum response size in bytes. Default: 4096.
    #[serde(default)]
    pub max_bytes: Option<usize>,
    /// Substring terminators (used when `completion_mode` is `substring`).
    #[serde(default)]
    pub terminators: Option<Vec<String>>,
    /// Complete after this many ms of RX silence once data has arrived.
    #[serde(default)]
    pub idle_ms: Option<u64>,
    /// How to prepare RX before write. Default: drain.
    #[serde(default)]
    pub rx_prepare: Option<RxPrepareMode>,
    /// Idle ms during soft drain. Default: 50.
    #[serde(default)]
    pub drain_idle_ms: Option<u64>,
    /// Max ms for soft drain. Default: 200.
    #[serde(default)]
    pub drain_max_ms: Option<u64>,
    /// Completion strategy. Default: atFinalLine.
    #[serde(default)]
    pub completion_mode: Option<ExchangeCompletionMode>,
    /// Command string for AT parse (echo / solicited classification).
    #[serde(default)]
    pub command: Option<String>,
    /// Prefixes treated as solicited `+` lines (not URC), e.g. `+CSQ:`.
    #[serde(default)]
    pub solicited_prefixes: Option<Vec<String>>,
    /// Verbose (`ATV1`) vs numeric (`ATV0`) final/intermediate lines. Default: verbose.
    #[serde(default)]
    pub result_format: Option<AtResultFormat>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_event_data_roundtrip() {
        let event = SerialEvent::Data {
            path: "/dev/ttyUSB0".into(),
            data: vec![1, 2, 255],
            size: 3,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "data");
        assert_eq!(json["path"], "/dev/ttyUSB0");
        let back: SerialEvent = serde_json::from_value(json).unwrap();
        match back {
            SerialEvent::Data { size, .. } => assert_eq!(size, 3),
            _ => panic!("expected data"),
        }
    }

    #[test]
    fn serial_event_error_roundtrip() {
        let event = SerialEvent::Error {
            path: "/dev/ttyUSB0".into(),
            message: "read glitch".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "error");
        assert_eq!(json["message"], "read glitch");
        let back: SerialEvent = serde_json::from_value(json).unwrap();
        match back {
            SerialEvent::Error { message, .. } => assert_eq!(message, "read glitch"),
            _ => panic!("expected error"),
        }
    }

    #[test]
    fn serial_event_disconnect_roundtrip() {
        let event = SerialEvent::Disconnect {
            path: "/dev/ttyUSB0".into(),
            reason: "USB unplugged".into(),
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "disconnect");
        assert_eq!(json["reason"], "USB unplugged");
        let back: SerialEvent = serde_json::from_value(json).unwrap();
        match back {
            SerialEvent::Disconnect { reason, .. } => assert_eq!(reason, "USB unplugged"),
            _ => panic!("expected disconnect"),
        }
    }

    #[test]
    fn capabilities_has_version() {
        let caps = Capabilities::current();
        assert!(!caps.version.is_empty());
        assert!(caps.transport == "desktop" || caps.transport == "mobile");
    }
}
