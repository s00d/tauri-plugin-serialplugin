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
        None
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

impl From<Error> for io::Error {
    fn from(error: Error) -> io::Error {
        match error {
            Error::Io(s) => io::Error::new(io::ErrorKind::Other, s),
            Error::String(s) => io::Error::new(io::ErrorKind::Other, s),
            Error::SerialPort(s) => io::Error::new(io::ErrorKind::Other, s),
        }
    }
}

impl Serialize for Error {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.to_string().as_ref())
    }
}
