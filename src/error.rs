use serde::{Serialize, Serializer};
use std::io;
#[cfg(target_os = "android")]
use tauri::plugin::mobile::PluginInvokeError;

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

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(_) => None,
            Error::SerialPort(_) => None,
            Error::String(_) => None,
        }
    }
}

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

impl From<Error> for io::Error {
    fn from(error: Error) -> io::Error {
        match error {
            Error::Io(e) => io::Error::new(io::ErrorKind::Other, e),
            Error::String(s) => io::Error::new(io::ErrorKind::Other, s),
            Error::SerialPort(e) => io::Error::new(io::ErrorKind::Other, e),
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

#[cfg(target_os = "android")]
impl From<PluginInvokeError> for Error {
    fn from(error: PluginInvokeError) -> Self {
        Error::String(error.to_string())
    }
}

// Реализация Send и Sync для Error
unsafe impl Send for Error {}
unsafe impl Sync for Error {}
