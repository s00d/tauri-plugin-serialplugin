#[cfg(desktop)]
use crate::cmux::CmuxSession;
#[cfg(target_os = "android")]
use crate::cmux::CmuxSession;
#[cfg(target_os = "android")]
use crate::hub::mobile::MobileRxHub;
#[cfg(desktop)]
use crate::hub::PortRxHub;
use crate::port::tx_queue::PortTxQueue;
#[cfg(desktop)]
use serialport::{self, SerialPort};
use std::sync::Arc;
use std::sync::{atomic::AtomicBool, Mutex};

/// Cloneable Arc handles for I/O without holding the global port map lock.
#[cfg(desktop)]
#[derive(Clone)]
pub struct ConnectedPortHandle {
    pub port: Arc<Mutex<Box<dyn SerialPort>>>,
    pub rx_hub: Arc<Mutex<Option<Arc<PortRxHub>>>>,
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
    pub rx_hub: Arc<Mutex<Option<Arc<PortRxHub>>>>,
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
#[derive(Clone)]
pub struct VirtualPortRef {
    pub physical_path: String,
    pub dlci: u8,
    pub exchange_cancel: Arc<AtomicBool>,
    pub tx_queue: Arc<PortTxQueue>,
}

#[cfg(target_os = "android")]
pub type MobileVirtualPortRef = VirtualPortRef;

#[cfg(desktop)]
pub type PhysicalPortRef = ConnectedPort;
#[cfg(target_os = "android")]
pub type PhysicalPortRef = MobileConnectedPort;

#[cfg(desktop)]
pub type PhysicalPortHandle = ConnectedPortHandle;
#[cfg(target_os = "android")]
pub type PhysicalPortHandle = MobileConnectedPortHandle;

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
        }
    }

    pub fn handle(&self) -> MobileConnectedPortHandle {
        MobileConnectedPortHandle {
            path: self.path.clone(),
            rx_hub: self.rx_hub.clone(),
            mux: self.mux.clone(),
            exchange_cancel: self.exchange_cancel.clone(),
            tx_queue: self.tx_queue.clone(),
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
