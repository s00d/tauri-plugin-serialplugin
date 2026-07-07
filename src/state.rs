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

#[cfg(desktop)]
use crate::cmux::CmuxSession;
#[cfg(target_os = "android")]
use crate::cmux::CmuxSession;
#[cfg(target_os = "android")]
use crate::mobile_rx_hub::MobileRxHub;
#[cfg(desktop)]
use crate::port_rx_hub::PortRxHub;
#[cfg(desktop)]
use crate::port_tx_queue::PortTxQueue;
#[cfg(target_os = "android")]
use crate::port_tx_queue::PortTxQueue;
use serde::{Deserialize, Serialize};
#[cfg(desktop)]
use serialport::{self, SerialPort};
#[cfg(desktop)]
use serialport::{
    ClearBuffer as SerialClearBuffer, DataBits as SerialDataBits, FlowControl as SerialFlowControl,
    Parity as SerialParity, StopBits as SerialStopBits,
};
#[cfg(desktop)]
use std::sync::atomic::AtomicBool;
#[cfg(target_os = "android")]
use std::sync::atomic::AtomicBool;
#[cfg(desktop)]
use std::sync::Arc;
#[cfg(target_os = "android")]
use std::sync::Arc;
use std::sync::{Mutex, OnceLock};

/// Cloneable Arc handles for I/O without holding the global port map lock.
#[cfg(desktop)]
#[derive(Clone)]
pub struct ConnectedPortHandle {
    pub port: Arc<Mutex<Box<dyn SerialPort>>>,
    pub rx_hub: Arc<Mutex<Option<PortRxHub>>>,
    pub mux: Arc<Mutex<Option<Arc<CmuxSession>>>>,
    pub virtual_dlci: Option<u8>,
    pub physical_path: Option<String>,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
}

/// Open serial port with optional background read thread handles (desktop).
#[cfg(desktop)]
pub struct ConnectedPort {
    /// Underlying serial device (shared with the RX hub thread).
    pub port: Arc<Mutex<Box<dyn SerialPort>>>,
    /// Single RX consumer on the main fd (lazy-started).
    pub rx_hub: Arc<Mutex<Option<PortRxHub>>>,
    /// Active GSM 07.10 CMUX session (physical port only).
    pub mux: Arc<Mutex<Option<Arc<CmuxSession>>>>,
    /// When set, this managed port is a virtual CMUX channel (legacy; prefer [`VirtualPortRef`] map).
    pub virtual_dlci: Option<u8>,
    /// Physical path when `virtual_dlci` is set.
    pub physical_path: Option<String>,
    /// Cancel flag for an in-flight exchange.
    pub exchange_cancel: Arc<AtomicBool>,
    /// FIFO turnstile for read-until transactions on this port.
    pub tx_queue: Arc<PortTxQueue>,
}

/// Lightweight CMUX virtual channel handle (no duplicate RX hub).
#[cfg(desktop)]
#[derive(Clone)]
pub struct VirtualPortRef {
    pub physical_path: String,
    pub dlci: u8,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
}

/// Lifecycle state for a managed port (desktop).
#[cfg(desktop)]
pub enum PortState {
    /// Slot unused or released (only kept for tests / explicit transitions)
    Closed,
    /// `open()` is in progress — I/O must wait
    Opening,
    /// Port is ready for read/write/settings
    Connected(ConnectedPort),
}

#[cfg(desktop)]
impl PortState {
    /// Human-readable reason when [`PortState::Connected`] is required but state differs.
    pub fn not_connected_reason(&self) -> String {
        match self {
            PortState::Closed => "Port is closed".to_string(),
            PortState::Opening => "Port is still opening".to_string(),
            PortState::Connected(_) => "Port is connected".to_string(),
        }
    }
}

/// Per-port state container (desktop).
#[cfg(desktop)]
pub struct SerialportInfo {
    /// Current lifecycle state (desktop).
    pub state: PortState,
}

#[cfg(desktop)]
impl SerialportInfo {
    /// Creates a new `SerialportInfo` in [`PortState::Connected`] with no listener thread.
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self {
            state: PortState::Connected(ConnectedPort {
                port: Arc::new(Mutex::new(port)),
                rx_hub: Arc::new(Mutex::new(None)),
                mux: Arc::new(Mutex::new(None)),
                virtual_dlci: None,
                physical_path: None,
                exchange_cancel: Arc::new(AtomicBool::new(false)),
                tx_queue: Arc::new(PortTxQueue::new()),
            }),
        }
    }

    /// Mutable access to [`ConnectedPort`] when state is [`PortState::Connected`].
    #[cfg(test)]
    pub(crate) fn connected_port_mut(&mut self) -> Option<&mut ConnectedPort> {
        match &mut self.state {
            PortState::Connected(cp) => Some(cp),
            _ => None,
        }
    }
}

#[cfg(desktop)]
impl ConnectedPort {
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self {
            port: Arc::new(Mutex::new(port)),
            rx_hub: Arc::new(Mutex::new(None)),
            mux: Arc::new(Mutex::new(None)),
            virtual_dlci: None,
            physical_path: None,
            exchange_cancel: Arc::new(AtomicBool::new(false)),
            tx_queue: Arc::new(PortTxQueue::new()),
        }
    }

    /// Clone Arc-backed fields for use after releasing the global port map lock.
    pub fn handle(&self) -> ConnectedPortHandle {
        ConnectedPortHandle {
            port: self.port.clone(),
            rx_hub: self.rx_hub.clone(),
            mux: self.mux.clone(),
            virtual_dlci: self.virtual_dlci,
            physical_path: self.physical_path.clone(),
            exchange_cancel: self.exchange_cancel.clone(),
            tx_queue: self.tx_queue.clone(),
        }
    }
}

#[cfg(all(desktop, test))]
impl ConnectedPort {
    pub fn test_port_mut(&self) -> std::sync::MutexGuard<'_, Box<dyn SerialPort>> {
        self.port.lock().unwrap()
    }
}

/// Open Android USB port with Rust-side orchestration (RX hub, queue, CMUX).
#[cfg(target_os = "android")]
pub struct MobileConnectedPort {
    pub path: String,
    pub rx_hub: Arc<Mutex<Option<Arc<MobileRxHub>>>>,
    pub mux: Arc<Mutex<Option<Arc<CmuxSession>>>>,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
    pub listening: Arc<AtomicBool>,
}

#[cfg(target_os = "android")]
impl MobileConnectedPort {
    pub fn new(path: String) -> Self {
        Self {
            path,
            rx_hub: Arc::new(Mutex::new(None)),
            mux: Arc::new(Mutex::new(None)),
            exchange_cancel: Arc::new(AtomicBool::new(false)),
            tx_queue: Arc::new(PortTxQueue::new()),
            listening: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn handle(&self) -> MobileConnectedPortHandle {
        MobileConnectedPortHandle {
            path: self.path.clone(),
            rx_hub: self.rx_hub.clone(),
            mux: self.mux.clone(),
            exchange_cancel: self.exchange_cancel.clone(),
            tx_queue: self.tx_queue.clone(),
            listening: self.listening.clone(),
        }
    }
}

#[cfg(target_os = "android")]
#[derive(Clone)]
pub struct MobileConnectedPortHandle {
    pub path: String,
    pub rx_hub: Arc<Mutex<Option<Arc<MobileRxHub>>>>,
    pub mux: Arc<Mutex<Option<Arc<CmuxSession>>>>,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
    pub listening: Arc<AtomicBool>,
}

#[cfg(target_os = "android")]
#[derive(Clone)]
pub struct MobileVirtualPortRef {
    pub physical_path: String,
    pub dlci: u8,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
}

#[cfg(target_os = "android")]
pub enum MobilePortState {
    Closed,
    Opening,
    Connected(MobileConnectedPort),
}

#[cfg(target_os = "android")]
impl MobilePortState {
    pub fn not_connected_reason(&self) -> String {
        match self {
            MobilePortState::Closed => "Port is closed".to_string(),
            MobilePortState::Opening => "Port is still opening".to_string(),
            MobilePortState::Connected(_) => "Port is connected".to_string(),
        }
    }
}

#[cfg(target_os = "android")]
pub struct MobileSerialportInfo {
    pub state: MobilePortState,
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

#[cfg(desktop)]
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

#[cfg(desktop)]
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

#[cfg(desktop)]
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

#[cfg(desktop)]
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

#[cfg(desktop)]
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
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum LogLevel {
    /// No logging output
    None,
    /// Only critical errors
    Error,
    /// Errors and warnings
    Warn,
    /// Errors, warnings, and general information
    #[default]
    Info,
    /// All logging including debug information
    Debug,
}

impl LogLevel {
    /// Checks if error messages should be logged at the current level
    pub fn should_log_error(&self) -> bool {
        matches!(
            self,
            LogLevel::Error | LogLevel::Warn | LogLevel::Info | LogLevel::Debug
        )
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
    *get_log_level_mutex().lock().unwrap_or_else(|e| {
        eprintln!("Failed to lock log level: {}", e);
        e.into_inner()
    })
}
