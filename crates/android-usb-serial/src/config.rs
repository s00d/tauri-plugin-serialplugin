//! Line configuration types (no plugin dependency).

/// ASCII XON (DC1).
pub const CHAR_XON: u8 = 17;
/// ASCII XOFF (DC3).
pub const CHAR_XOFF: u8 = 19;

/// Number of data bits in the serial frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

/// Parity bit mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

/// Stop bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

/// Hardware / software flow control.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowControl {
    None,
    RtsCts,
    DtrDsr,
    /// Host software XON/XOFF (CP21xx).
    XonXoff,
    /// Inline filter on RX (FTDI, PL2303).
    XonXoffInline,
}

/// Which UART buffers to purge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeKind {
    Rx,
    Tx,
    Both,
}

/// Baud rate and framing for [`crate::SerialPortHandle::set_line_config`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LineConfig {
    pub baud_rate: u32,
    pub data_bits: DataBits,
    pub parity: Parity,
    pub stop_bits: StopBits,
}

impl Default for LineConfig {
    fn default() -> Self {
        Self {
            baud_rate: 115_200,
            data_bits: DataBits::Eight,
            parity: Parity::None,
            stop_bits: StopBits::One,
        }
    }
}
