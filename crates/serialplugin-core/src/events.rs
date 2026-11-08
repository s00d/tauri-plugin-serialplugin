//! Streaming event types, event sinks and watch / exchange options.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Receives the events of one [`SerialManager::watch`](crate::SerialManager::watch).
///
/// Called on the port's RX thread, so keep it short (push into a channel for heavy
/// work); the last events of a watch (parser remainder on unwatch / close / re-watch)
/// may run on the caller's thread instead. Events arrive in order, one call at a time
/// per port, never under a manager lock: the sink may call back into the manager.
/// Return `false` once the receiver is gone; the failure is logged.
pub type EventSink = Arc<dyn Fn(SerialEvent) -> bool + Send + Sync>;

/// Receives [`PortListEvent`]s of one [`SerialManager::watch_ports`](crate::SerialManager::watch_ports).
/// Returning `false` ends the subscription.
pub type PortListSink = Arc<dyn Fn(PortListEvent) -> bool + Send + Sync>;

/// Forward events to a Tauri IPC channel (serialized to JSON per event); the sink
/// reports `false` once the webview side of the channel is gone. Feature `tauri`.
#[cfg(feature = "tauri")]
pub fn channel_sink<E>(channel: tauri::ipc::Channel<E>) -> Arc<dyn Fn(E) -> bool + Send + Sync>
where
    E: tauri::ipc::IpcResponse + Send + Sync + 'static,
{
    Arc::new(move |event| channel.send(event).is_ok())
}

/// Options for [`SerialManager::watch_ports`](crate::SerialManager::watch_ports).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchPortsOptions {
    /// Same as [`SerialManager::available_ports`](crate::SerialManager::available_ports) `single_port_per_device`.
    #[serde(default)]
    pub single_port_per_device: Option<bool>,
    /// Poll interval on desktop (ms). Default: 2000. Android also polls as fallback.
    #[serde(default)]
    pub poll_interval_ms: Option<u64>,
}

/// Port list change events delivered to a [`PortListSink`].
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

/// Event delivered to an [`EventSink`] (in the Tauri plugin: to the webview channel, as JSON).
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

/// Options for [`SerialManager::watch`](crate::SerialManager::watch). Unknown keys are
/// ignored (as in 4.0.0). A native parser is passed separately, see
/// [`SerialManager::watch_framed`](crate::SerialManager::watch_framed).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WatchOptions {
    #[serde(default)]
    pub timeout: Option<u64>,
    #[serde(default)]
    pub size: Option<usize>,
    #[serde(default)]
    pub serial_data_flush_interval_ms: Option<u64>,
    /// When `true`, idle watch RX is line-routed so AT URC lines become
    /// [`SerialEvent::Urc`] instead of raw `data`. Default `false`: bytes are
    /// forwarded unchanged (binary / NMEA / non-AT streams).
    ///
    /// The JS guest sets this automatically when `onUrc` is provided.
    #[serde(default)]
    pub route_urc: bool,
}

/// Receives the events of the core's new API: one
/// [`SerialManager::watch_core`](crate::SerialManager::watch_core) stream or the port
/// states of [`SerialManager::subscribe_core`](crate::SerialManager::subscribe_core).
///
/// Runs on a delivery thread of its own (one per subscription), never on the RX
/// thread and never under a manager lock: it may block for a while, and it may call
/// back into the manager, `at()` on the same port included. Events wait in a bounded
/// queue ([`SerialManager::set_core_queue_capacity`](crate::SerialManager::set_core_queue_capacity),
/// default [`DEFAULT_CORE_QUEUE_BYTES`]); when it is full the sink gets one
/// [`CoreEvent::Overflow`] and is unsubscribed. Returning `false` unsubscribes too.
pub type CoreSink = Arc<dyn Fn(CoreEvent) -> bool + Send + Sync>;

/// Default per-subscription queue of the core's Rust receivers (4 MiB of event data).
pub const DEFAULT_CORE_QUEUE_BYTES: usize = 4 << 20;

/// Id of a [`SerialManager::watch_core`](crate::SerialManager::watch_core) stream or a
/// [`SerialManager::subscribe_core`](crate::SerialManager::subscribe_core) subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SubscriptionId(pub u64);

/// Event of the core's new API (see [`CoreSink`]).
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum CoreEvent {
    /// A watch event, exactly as the old watch sends it (the final `Disconnect` included).
    Event(SerialEvent),
    /// The queue overflowed: data from byte `dropped_from` of this stream on (counting
    /// `Data` bytes delivered before it) is gone, and the sink is unsubscribed.
    Overflow { dropped_from: u64 },
    /// A port changed state ([`SerialManager::subscribe_core`](crate::SerialManager::subscribe_core) only).
    PortState { path: String, state: PortStatus },
}

/// A port's state change in [`CoreEvent::PortState`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PortStatus {
    Opened,
    /// Lost without `close()` (unplug, EOF, I/O error); the core has released the port.
    Disconnected {
        kind: crate::error::ErrorKind,
        reason: String,
    },
    /// Closed by `close()` / `force_close()` / `close_all()`.
    Closed,
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

pub use serial_codec::at::AtResultFormat;

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

/// Options for [`SerialManager::exchange`](crate::SerialManager::exchange) (write + read-until-response).
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
}
