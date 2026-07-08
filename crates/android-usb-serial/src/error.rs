//! USB serial driver errors.

use std::fmt;

/// High-level driver / transport failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbSerialError {
    /// Operation or chip feature not supported.
    Unsupported(String),
    /// I/O or USB transfer failure (including stall mapped to a message).
    Io(String),
    /// Transfer timed out.
    TimedOut,
    /// Device unplugged or USB link lost (bulk IN detach / EPROTO, etc.).
    Disconnected,
    /// Transfer cancelled by the host.
    Cancelled,
    /// Could not identify / open a suitable driver.
    ProbeFailed(String),
}

impl fmt::Display for UsbSerialError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(msg) => write!(f, "unsupported: {msg}"),
            Self::Io(msg) => write!(f, "io: {msg}"),
            Self::TimedOut => write!(f, "timed out"),
            Self::Disconnected => write!(f, "disconnected"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::ProbeFailed(msg) => write!(f, "probe failed: {msg}"),
        }
    }
}

impl std::error::Error for UsbSerialError {}

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, UsbSerialError>;

/// Low-level USB transfer failure (maps into [`UsbSerialError`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransferError {
    TimedOut,
    Stall,
    Disconnected,
    Cancelled,
    Other(String),
}

impl From<TransferError> for UsbSerialError {
    fn from(value: TransferError) -> Self {
        match value {
            TransferError::TimedOut => Self::TimedOut,
            TransferError::Disconnected => Self::Disconnected,
            TransferError::Cancelled => Self::Cancelled,
            TransferError::Stall => Self::Io("stall".into()),
            TransferError::Other(msg) => Self::Io(msg),
        }
    }
}

/// Bulk/control transfer outcome for [`crate::transport::BulkIn::read`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReadOutcome {
    /// Payload bytes received.
    Data(Vec<u8>),
    TimedOut,
    Cancelled,
}
