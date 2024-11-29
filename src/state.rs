use serde::{Deserialize, Serialize};
use serialport::{self, SerialPort};
use serialport::{
    ClearBuffer as SerialClearBuffer, DataBits as SerialDataBits, FlowControl as SerialFlowControl,
    Parity as SerialParity, StopBits as SerialStopBits,
};
use std::thread::JoinHandle;
use std::{
    collections::HashMap,
    sync::{mpsc::Sender, Arc, Mutex},
};

#[derive(Default)]
pub struct SerialportState {
    // plugin state, configuration fields
    pub serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
}
pub struct SerialportInfo {
    pub serialport: Box<dyn SerialPort>,
    pub sender: Option<Sender<usize>>,
    pub thread_handle: Option<JoinHandle<()>>,
}

#[derive(Serialize, Clone)]
pub struct InvokeResult {
    pub code: i32,
    pub message: String,
}

#[derive(Serialize, Clone)]
pub struct ReadData<'a> {
    pub data: &'a [u8],
    pub size: usize,
}

pub const UNKNOWN: &str = "Unknown";
pub const USB: &str = "USB";
pub const BLUETOOTH: &str = "Bluetooth";
pub const PCI: &str = "PCI";

/// Number of bits per character
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataBits {
    /// 5 bits per character
    Five,
    /// 6 bits per character
    Six,
    /// 7 bits per character
    Seven,
    /// 8 bits per character
    Eight,
}

impl From<DataBits> for SerialDataBits {
    fn from(bits: DataBits) -> Self {
        match bits {
            DataBits::Five => SerialDataBits::Five,
            DataBits::Six => SerialDataBits::Six,
            DataBits::Seven => SerialDataBits::Seven,
            DataBits::Eight => SerialDataBits::Eight,
        }
    }
}

impl DataBits {
    pub fn as_u8(&self) -> u8 {
        match self {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        }
    }
}

/// Flow control modes
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowControl {
    /// No flow control
    None,
    /// Flow control using XON/XOFF bytes
    Software,
    /// Flow control using RTS/CTS signals
    Hardware,
}

impl From<FlowControl> for SerialFlowControl {
    fn from(flow: FlowControl) -> Self {
        match flow {
            FlowControl::None => SerialFlowControl::None,
            FlowControl::Software => SerialFlowControl::Software,
            FlowControl::Hardware => SerialFlowControl::Hardware,
        }
    }
}

impl FlowControl {
    pub fn as_u8(&self) -> u8 {
        match self {
            FlowControl::None => 0,
            FlowControl::Software => 1,
            FlowControl::Hardware => 2,
        }
    }
}

/// Parity checking modes
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Parity {
    /// No parity bit
    None,
    /// Parity bit sets odd number of 1 bits
    Odd,
    /// Parity bit sets even number of 1 bits
    Even,
}

impl From<Parity> for SerialParity {
    fn from(parity: Parity) -> Self {
        match parity {
            Parity::None => SerialParity::None,
            Parity::Odd => SerialParity::Odd,
            Parity::Even => SerialParity::Even,
        }
    }
}

impl Parity {
    pub fn as_u8(&self) -> u8 {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// Number of stop bits
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopBits {
    /// One stop bit
    One,
    /// Two stop bits
    Two,
}

impl From<StopBits> for SerialStopBits {
    fn from(bits: StopBits) -> Self {
        match bits {
            StopBits::One => SerialStopBits::One,
            StopBits::Two => SerialStopBits::Two,
        }
    }
}

impl StopBits {
    pub fn as_u8(&self) -> u8 {
        match self {
            StopBits::One => 1,
            StopBits::Two => 2,
        }
    }
}

/// Buffer types for clearing
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClearBuffer {
    /// Input buffer (received data)
    Input,
    /// Output buffer (transmitted data)
    Output,
    /// Both input and output buffers
    All,
}

impl From<ClearBuffer> for SerialClearBuffer {
    fn from(buffer: ClearBuffer) -> Self {
        match buffer {
            ClearBuffer::Input => SerialClearBuffer::Input,
            ClearBuffer::Output => SerialClearBuffer::Output,
            ClearBuffer::All => SerialClearBuffer::All,
        }
    }
}
