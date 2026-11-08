//! iOS stand-in for the `serialport` crate: only the trait and value types the
//! plugin uses, no I/O.
//!
//! The real crate compiles its IOKit port enumeration for iOS, and the linker pulls
//! that object in as soon as any `serialport` symbol is used; iOS apps do not link
//! IOKit, so the app failed to link. iOS has no local serial ports anyway, the only
//! transport there is `tcp://` ([`crate::port::tcp`]), which needs just these types.
//! Kept signature-compatible with serialport 4.x.

use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoDevice,
    InvalidInput,
    Unknown,
    Io(io::ErrorKind),
}

#[derive(Debug, Clone)]
pub struct Error {
    pub kind: ErrorKind,
    pub description: String,
}

impl Error {
    pub fn new<T: Into<String>>(kind: ErrorKind, description: T) -> Self {
        Error {
            kind,
            description: description.into(),
        }
    }

    pub fn kind(&self) -> ErrorKind {
        self.kind
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.description)
    }
}

impl StdError for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Error {
        Error::new(ErrorKind::Io(e.kind()), e.to_string())
    }
}

impl From<Error> for io::Error {
    fn from(e: Error) -> io::Error {
        let kind = match e.kind {
            ErrorKind::NoDevice => io::ErrorKind::NotFound,
            ErrorKind::InvalidInput => io::ErrorKind::InvalidInput,
            ErrorKind::Unknown => io::ErrorKind::Other,
            ErrorKind::Io(kind) => kind,
        };
        io::Error::new(kind, e.description)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum StopBits {
    One,
    Two,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FlowControl {
    None,
    Software,
    Hardware,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ClearBuffer {
    Input,
    Output,
    All,
}

pub trait SerialPort: Send + io::Read + io::Write {
    fn name(&self) -> Option<String>;
    fn baud_rate(&self) -> Result<u32>;
    fn data_bits(&self) -> Result<DataBits>;
    fn flow_control(&self) -> Result<FlowControl>;
    fn parity(&self) -> Result<Parity>;
    fn stop_bits(&self) -> Result<StopBits>;
    fn timeout(&self) -> Duration;
    fn set_baud_rate(&mut self, baud_rate: u32) -> Result<()>;
    fn set_data_bits(&mut self, data_bits: DataBits) -> Result<()>;
    fn set_flow_control(&mut self, flow_control: FlowControl) -> Result<()>;
    fn set_parity(&mut self, parity: Parity) -> Result<()>;
    fn set_stop_bits(&mut self, stop_bits: StopBits) -> Result<()>;
    fn set_timeout(&mut self, timeout: Duration) -> Result<()>;
    fn write_request_to_send(&mut self, level: bool) -> Result<()>;
    fn write_data_terminal_ready(&mut self, level: bool) -> Result<()>;
    fn read_clear_to_send(&mut self) -> Result<bool>;
    fn read_data_set_ready(&mut self) -> Result<bool>;
    fn read_ring_indicator(&mut self) -> Result<bool>;
    fn read_carrier_detect(&mut self) -> Result<bool>;
    fn bytes_to_read(&self) -> Result<u32>;
    fn bytes_to_write(&self) -> Result<u32>;
    fn clear(&self, buffer_to_clear: ClearBuffer) -> Result<()>;
    fn try_clone(&self) -> Result<Box<dyn SerialPort>>;
    fn set_break(&self) -> Result<()>;
    fn clear_break(&self) -> Result<()>;
}
