//! USB serial driver errors.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbSerialError {
    Unsupported(String),
    Io(String),
    TimedOut,
    Disconnected,
    Cancelled,
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

pub type Result<T> = std::result::Result<T, UsbSerialError>;

/// Low-level USB transfer failure.
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
    Data(Vec<u8>),
    TimedOut,
    Cancelled,
}
