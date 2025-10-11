//! State types and enums for serial port configuration
//! 
//! This module contains all the types and enums used for configuring serial ports,
//! including data bits, flow control, parity, stop bits, and buffer types.
//! 
//! # Example
//! 
//! ```rust
//! use tauri_plugin_serialplugin::state::{DataBits, FlowControl, Parity, StopBits, ClearBuffer};
//! 
//! // Configure serial port settings
//! let data_bits = DataBits::Eight;
//! let flow_control = FlowControl::None;
//! let parity = Parity::None;
//! let stop_bits = StopBits::One;
//! let buffer_type = ClearBuffer::All;
//! ```

use serde::{Deserialize, Serialize};
use serialport::{self, SerialPort};
use serialport::{
    ClearBuffer as SerialClearBuffer, DataBits as SerialDataBits, FlowControl as SerialFlowControl,
    Parity as SerialParity, StopBits as SerialStopBits,
};
use std::thread::JoinHandle;
use std::{
    collections::HashMap,
    sync::{mpsc::Sender, Arc, Mutex, OnceLock},
};

/// Main state structure for managing serial ports
/// 
/// This structure holds the global state of all serial ports managed by the plugin.
/// It uses thread-safe containers to allow concurrent access from multiple threads.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::SerialportState;
/// 
/// let state = SerialportState::default();
/// ```
#[derive(Default)]
pub struct SerialportState {
    /// Thread-safe map of port names to port information
    /// 
    /// This field stores all currently managed serial ports. The outer `Arc<Mutex<>>`
    /// ensures thread safety, while the inner `HashMap` maps port names (like "COM1")
    /// to their corresponding `SerialportInfo` structures.
    pub serialports: Arc<Mutex<HashMap<String, SerialportInfo>>>,
}
/// Information structure for a single serial port
/// 
/// This structure holds all the information needed to manage a single serial port,
/// including the port itself, communication channels, and background threads.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::SerialportInfo;
/// use serialport::SerialPort;
/// 
/// // This is typically created internally by the plugin
/// // let info = SerialportInfo::new(port);
/// ```
pub struct SerialportInfo {
    /// The actual serial port implementation
    /// 
    /// This is a boxed trait object that implements the `SerialPort` trait,
    /// providing the actual serial communication functionality.
    pub serialport: Box<dyn SerialPort>,
    
    /// Optional sender for communication with background threads
    /// 
    /// This sender is used to communicate with background threads that handle
    /// asynchronous reading operations. It's `None` when no background reading
    /// is active.
    pub sender: Option<Sender<usize>>,
    
    /// Optional handle to background thread
    /// 
    /// This handle allows the plugin to manage background threads that perform
    /// continuous reading operations. It's `None` when no background thread
    /// is running.
    pub thread_handle: Option<JoinHandle<()>>,
}

impl SerialportInfo {
    /// Creates a new `SerialportInfo` instance
    /// 
    /// This constructor creates a new serial port information structure
    /// with the provided serial port implementation. The sender and thread
    /// handle are initialized to `None` and should be set later if needed.
    /// 
    /// # Arguments
    /// 
    /// * `serialport` - A boxed serial port implementation
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use tauri_plugin_serialplugin::state::SerialportInfo;
    /// use serialport::SerialPort;
    /// 
    /// // This is typically used internally by the plugin
    /// // let info = SerialportInfo::new(port);
    /// ```
    pub fn new(serialport: Box<dyn SerialPort>) -> Self {
        Self {
            serialport,
            sender: None,
            thread_handle: None,
        }
    }
}

/// Result structure for Tauri invoke operations
/// 
/// This structure is used to return results from Tauri command invocations
/// with a standardized format including a status code and message.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::InvokeResult;
/// 
/// let result = InvokeResult {
///     code: 0,
///     message: "Operation completed successfully".to_string(),
/// };
/// ```
#[derive(Serialize, Clone)]
pub struct InvokeResult {
    /// Status code indicating success (0) or error (non-zero)
    pub code: i32,
    /// Human-readable message describing the result
    pub message: String,
}

/// Structure for holding read data from serial ports
/// 
/// This structure holds data that has been read from a serial port,
/// including a reference to the data and its size.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::ReadData;
/// 
/// let data = b"Hello World";
/// let read_data = ReadData {
///     data: data,
///     size: data.len(),
/// };
/// ```
#[derive(Serialize, Clone)]
pub struct ReadData<'a> {
    /// Reference to the read data bytes
    pub data: &'a [u8],
    /// Size of the read data in bytes
    pub size: usize,
}

/// Port type constants for identifying serial port types
/// 
/// These constants are used to identify the type of serial port
/// when listing available ports.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::{USB, BLUETOOTH, PCI, UNKNOWN};
/// 
/// let port_type = USB;
/// match port_type {
///     USB => println!("USB serial port"),
///     BLUETOOTH => println!("Bluetooth serial port"),
///     PCI => println!("PCI serial port"),
///     _ => println!("Unknown port type"),
/// }
/// ```

/// Unknown port type
pub const UNKNOWN: &str = "Unknown";
/// USB serial port
pub const USB: &str = "USB";
/// Bluetooth serial port
pub const BLUETOOTH: &str = "Bluetooth";
/// PCI serial port
pub const PCI: &str = "PCI";

/// Number of bits per character for serial communication
/// 
/// This enum defines the number of data bits used in each character transmitted
/// over the serial port. Most modern applications use 8 data bits.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::DataBits;
/// 
/// let data_bits = DataBits::Eight; // Most common setting
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataBits {
    /// 5 bits per character (rarely used)
    Five,
    /// 6 bits per character (rarely used)
    Six,
    /// 7 bits per character (used with parity)
    Seven,
    /// 8 bits per character (most common)
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
    /// Converts the data bits enum to its numeric value
    /// 
    /// Returns the number of bits as a `u8` value.
    /// 
    /// # Returns
    /// 
    /// The number of data bits: 5, 6, 7, or 8.
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use tauri_plugin_serialplugin::state::DataBits;
    /// 
    /// let data_bits = DataBits::Eight;
    /// assert_eq!(data_bits.as_u8(), 8);
    /// ```
    pub fn as_u8(&self) -> u8 {
        match self {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        }
    }
}

/// Flow control modes for serial communication
/// 
/// Flow control prevents data loss by allowing the receiver to signal when it's
/// ready to receive more data.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::FlowControl;
/// 
/// let flow_control = FlowControl::None; // No flow control
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FlowControl {
    /// No flow control (most common for simple applications)
    None,
    /// Software flow control using XON/XOFF bytes
    Software,
    /// Hardware flow control using RTS/CTS signals
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
    /// Converts the flow control enum to its numeric value
    /// 
    /// Returns the flow control mode as a `u8` value.
    /// 
    /// # Returns
    /// 
    /// The flow control mode: 0 (None), 1 (Software), or 2 (Hardware).
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use tauri_plugin_serialplugin::state::FlowControl;
    /// 
    /// let flow_control = FlowControl::None;
    /// assert_eq!(flow_control.as_u8(), 0);
    /// ```
    pub fn as_u8(&self) -> u8 {
        match self {
            FlowControl::None => 0,
            FlowControl::Software => 1,
            FlowControl::Hardware => 2,
        }
    }
}

/// Parity checking modes for serial communication
/// 
/// Parity is an error detection method that adds an extra bit to each character
/// to ensure the total number of 1 bits is either odd or even.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::Parity;
/// 
/// let parity = Parity::None; // No parity checking (most common)
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Parity {
    /// No parity bit (most common for modern applications)
    None,
    /// Parity bit ensures odd number of 1 bits
    Odd,
    /// Parity bit ensures even number of 1 bits
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
    /// Converts the parity enum to its numeric value
    /// 
    /// Returns the parity mode as a `u8` value.
    /// 
    /// # Returns
    /// 
    /// The parity mode: 0 (None), 1 (Odd), or 2 (Even).
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use tauri_plugin_serialplugin::state::Parity;
    /// 
    /// let parity = Parity::None;
    /// assert_eq!(parity.as_u8(), 0);
    /// ```
    pub fn as_u8(&self) -> u8 {
        match self {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
        }
    }
}

/// Number of stop bits for serial communication
/// 
/// Stop bits are used to signal the end of a character transmission.
/// Most modern applications use one stop bit.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::StopBits;
/// 
/// let stop_bits = StopBits::One; // Most common setting
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum StopBits {
    /// One stop bit (most common)
    One,
    /// Two stop bits (used in some legacy systems)
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
    /// Converts the stop bits enum to its numeric value
    /// 
    /// Returns the number of stop bits as a `u8` value.
    /// 
    /// # Returns
    /// 
    /// The number of stop bits: 1 or 2.
    /// 
    /// # Example
    /// 
    /// ```rust
    /// use tauri_plugin_serialplugin::state::StopBits;
    /// 
    /// let stop_bits = StopBits::One;
    /// assert_eq!(stop_bits.as_u8(), 1);
    /// ```
    pub fn as_u8(&self) -> u8 {
        match self {
            StopBits::One => 1,
            StopBits::Two => 2,
        }
    }
}

/// Buffer types for clearing serial port buffers
/// 
/// Serial ports maintain input and output buffers to store data.
/// This enum allows you to specify which buffers to clear.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::ClearBuffer;
/// 
/// let buffer_type = ClearBuffer::All; // Clear both input and output buffers
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClearBuffer {
    /// Input buffer (received data waiting to be read)
    Input,
    /// Output buffer (transmitted data waiting to be sent)
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

/// Logging level for controlling plugin verbosity
/// 
/// This enum allows you to control how much logging output the plugin produces.
/// Use it to reduce noise in production environments or enable detailed logs for debugging.
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::LogLevel;
/// 
/// let log_level = LogLevel::Error; // Only show errors
/// ```
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum LogLevel {
    /// No logging output
    None,
    /// Only critical errors
    Error,
    /// Errors and warnings
    Warn,
    /// Errors, warnings, and general information
    Info,
    /// All logging including debug information
    Debug,
}

impl Default for LogLevel {
    fn default() -> Self {
        LogLevel::Info
    }
}

impl LogLevel {
    /// Checks if error messages should be logged at the current level
    pub fn should_log_error(&self) -> bool {
        matches!(self, LogLevel::Error | LogLevel::Warn | LogLevel::Info | LogLevel::Debug)
    }

    /// Checks if warning messages should be logged at the current level
    pub fn should_log_warn(&self) -> bool {
        matches!(self, LogLevel::Warn | LogLevel::Info | LogLevel::Debug)
    }

    /// Checks if info messages should be logged at the current level
    pub fn should_log_info(&self) -> bool {
        matches!(self, LogLevel::Info | LogLevel::Debug)
    }

    /// Checks if debug messages should be logged at the current level
    pub fn should_log_debug(&self) -> bool {
        matches!(self, LogLevel::Debug)
    }
}

/// Global log level state
static LOG_LEVEL: OnceLock<Mutex<LogLevel>> = OnceLock::new();

/// Gets or initializes the log level mutex
fn get_log_level_mutex() -> &'static Mutex<LogLevel> {
    LOG_LEVEL.get_or_init(|| Mutex::new(LogLevel::Info))
}

/// Sets the global log level for the plugin
/// 
/// # Arguments
/// 
/// * `level` - The new log level to set
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::{LogLevel, set_log_level};
/// 
/// set_log_level(LogLevel::Error);
/// ```
pub fn set_log_level(level: LogLevel) {
    if let Ok(mut log_level) = get_log_level_mutex().lock() {
        *log_level = level;
    }
}

/// Gets the current global log level
/// 
/// # Returns
/// 
/// The current log level
/// 
/// # Example
/// 
/// ```rust
/// use tauri_plugin_serialplugin::state::get_log_level;
/// 
/// let level = get_log_level();
/// ```
pub fn get_log_level() -> LogLevel {
    get_log_level_mutex().lock().unwrap_or_else(|e| {
        eprintln!("Failed to lock log level: {}", e);
        e.into_inner()
    }).clone()
}
