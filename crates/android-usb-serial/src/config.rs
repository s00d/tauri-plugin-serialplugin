//! Line configuration types (no plugin dependency).

pub const CHAR_XON: u8 = 17;
pub const CHAR_XOFF: u8 = 19;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataBits {
    Five,
    Six,
    Seven,
    Eight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Parity {
    None,
    Odd,
    Even,
    Mark,
    Space,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopBits {
    One,
    OnePointFive,
    Two,
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PurgeKind {
    Rx,
    Tx,
    Both,
}

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
