#[cfg(target_os = "ios")]
use crate::serialport;
use serde::{Serialize, Serializer};
use std::io;

/// An error type for serial port operations
#[derive(Debug)]
pub enum Error {
    /// IO Error (stored as string to allow cloning)
    Io(String),
    /// String error message
    String(String),
    /// Serial port error
    SerialPort(String),
}

impl Clone for Error {
    fn clone(&self) -> Self {
        match self {
            Error::Io(s) => Error::Io(s.clone()),
            Error::String(s) => Error::String(s.clone()),
            Error::SerialPort(s) => Error::SerialPort(s.clone()),
        }
    }
}

impl Error {
    pub fn new(msg: impl Into<String>) -> Self {
        Error::String(msg.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Io(err) => write!(f, "IO error: {}", err),
            Error::String(s) => write!(f, "{}", s),
            Error::SerialPort(err) => write!(f, "Serial port error: {}", err),
        }
    }
}

/// No variant keeps its cause (they are stored as text), so `source()` is `None`.
impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err.to_string())
    }
}

impl From<serialport::Error> for Error {
    fn from(err: serialport::Error) -> Self {
        Error::SerialPort(err.to_string())
    }
}

impl From<&str> for Error {
    fn from(err: &str) -> Self {
        Error::String(err.to_string())
    }
}

impl From<String> for Error {
    fn from(err: String) -> Self {
        Error::String(err)
    }
}

/// 4.0.0 API on Android (feature `tauri`, enabled by the plugin).
#[cfg(all(target_os = "android", feature = "tauri"))]
impl From<tauri::plugin::mobile::PluginInvokeError> for Error {
    fn from(error: tauri::plugin::mobile::PluginInvokeError) -> Self {
        Error::String(error.to_string())
    }
}

impl From<Error> for io::Error {
    fn from(error: Error) -> io::Error {
        match error {
            Error::Io(e) => io::Error::other(e),
            Error::String(s) => io::Error::other(s),
            Error::SerialPort(e) => io::Error::other(e),
        }
    }
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

/// What went wrong, for code that has to react (retry, reopen, give up). See [`CoreError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ErrorKind {
    /// No such port, device or watch (also: the port is not open).
    NotFound,
    PermissionDenied,
    /// The port or a slot is taken (opened elsewhere, a read / drain / open in progress).
    Busy,
    /// The device went away (unplug, EOF, broken pipe).
    Disconnected,
    Timeout,
    Cancelled,
    Other,
}

impl ErrorKind {
    pub fn from_io(kind: io::ErrorKind) -> Self {
        use io::ErrorKind as K;
        match kind {
            K::NotFound => Self::NotFound,
            K::PermissionDenied => Self::PermissionDenied,
            K::TimedOut | K::WouldBlock => Self::Timeout,
            K::BrokenPipe
            | K::NotConnected
            | K::ConnectionAborted
            | K::ConnectionReset
            | K::UnexpectedEof => Self::Disconnected,
            K::AddrInUse | K::ResourceBusy => Self::Busy,
            K::Interrupted => Self::Cancelled,
            _ => Self::Other,
        }
    }

    pub fn from_serialport(err: &serialport::Error) -> Self {
        match err.kind() {
            // serialport reports a port in use as NoDevice too: EBUSY, its flock
            // ("Unable to acquire exclusive lock") and Windows ERROR_ACCESS_DENIED.
            serialport::ErrorKind::NoDevice
                if ["busy", "exclusive lock", "access is denied"]
                    .iter()
                    .any(|n| err.to_string().to_lowercase().contains(n)) =>
            {
                Self::Busy
            }
            serialport::ErrorKind::NoDevice => Self::NotFound,
            serialport::ErrorKind::Io(kind) => Self::from_io(kind),
            _ => Self::Other,
        }
    }

    /// Classify one of the core's own 4.x messages.
    // ponytail: text matching over the core's own message strings; carry the kind
    // through every internal call if a new message ever slips past it.
    pub fn from_message(message: &str) -> Self {
        let m = message.to_lowercase();
        let has = |needles: &[&str]| needles.iter().any(|n| m.contains(n));
        if has(&["cancel", "port closed"]) {
            Self::Cancelled
        } else if has(&["in progress", "already", "busy", "lock timeout"]) {
            Self::Busy
        } else if has(&[
            "disconnect",
            "broken pipe",
            "not connected",
            "connection reset",
            "connection aborted",
            "unexpected eof",
        ]) {
            Self::Disconnected
        } else if has(&["timed out", "timeout"]) {
            Self::Timeout
        } else if has(&["permission denied", "access is denied", "access denied"]) {
            Self::PermissionDenied
        } else if has(&[
            "not found",
            "no such file",
            "not open",
            "port is closed",
            "no active watch",
            "not enabled",
        ]) {
            Self::NotFound
        } else {
            Self::Other
        }
    }
}

/// Error of the core's new (4.2) methods (`open_core`, `write_core`, `watch_core`, …):
/// a [`kind`](Self::kind), the message (exactly the 4.x [`Error`] text) and the
/// underlying error as [`source`](std::error::Error::source). Converts into the old
/// [`Error`] with the same variant and text.
#[derive(Debug, Clone)]
pub struct CoreError {
    kind: ErrorKind,
    legacy: Error,
    source: Option<std::sync::Arc<dyn std::error::Error + Send + Sync>>,
}

impl CoreError {
    pub fn new(kind: ErrorKind, error: impl Into<Error>) -> Self {
        Self {
            kind,
            legacy: error.into(),
            source: None,
        }
    }

    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(std::sync::Arc::new(source));
        self
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }

    /// The same error with another kind.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn with_kind(mut self, kind: ErrorKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn message(&self) -> String {
        self.legacy.to_string()
    }

    /// From an I/O failure: kind from `err`, message `context: err`.
    pub(crate) fn io(context: String, err: io::Error) -> Self {
        Self::new(
            ErrorKind::from_io(err.kind()),
            Error::String(format!("{context}: {err}")),
        )
        .with_source(err)
    }

    /// From a serialport failure: kind from `err`, message `context: err`.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    pub(crate) fn serialport(context: String, err: serialport::Error) -> Self {
        Self::new(
            ErrorKind::from_serialport(&err),
            Error::String(format!("{context}: {err}")),
        )
        .with_source(err)
    }
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.legacy.fmt(f)
    }
}

impl std::error::Error for CoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_deref()
            .map(|s| s as &(dyn std::error::Error + 'static))
    }
}

impl From<CoreError> for Error {
    fn from(error: CoreError) -> Self {
        error.legacy
    }
}

impl From<Error> for CoreError {
    fn from(error: Error) -> Self {
        let kind = ErrorKind::from_message(&error.to_string());
        Self::new(kind, error)
    }
}

impl Serialize for CoreError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_kinds_map_to_core_kinds() {
        use io::ErrorKind as K;
        for (io_kind, kind) in [
            (K::NotFound, ErrorKind::NotFound),
            (K::PermissionDenied, ErrorKind::PermissionDenied),
            (K::TimedOut, ErrorKind::Timeout),
            (K::WouldBlock, ErrorKind::Timeout),
            (K::BrokenPipe, ErrorKind::Disconnected),
            (K::UnexpectedEof, ErrorKind::Disconnected),
            (K::AddrInUse, ErrorKind::Busy),
            (K::ResourceBusy, ErrorKind::Busy),
            (K::Interrupted, ErrorKind::Cancelled),
            (K::InvalidData, ErrorKind::Other),
        ] {
            assert_eq!(ErrorKind::from_io(io_kind), kind, "{io_kind:?}");
        }
    }

    #[test]
    fn core_error_message_is_the_legacy_text() {
        let e = CoreError::new(ErrorKind::Busy, Error::String("port busy".into()));
        assert_eq!(e.message(), "port busy");
        let io = CoreError::io("Failed to write".into(), io::Error::other("gone"));
        assert_eq!(io.message(), "Failed to write: gone");
        assert!(std::error::Error::source(&io).is_some());
        assert!(std::error::Error::source(&Error::Io("x".into())).is_none());
    }
}
