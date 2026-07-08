use crate::cmux::CmuxSession;
use crate::hub::PortRxHub;
use crate::port::tx_queue::PortTxQueue;
use serialport::SerialPort;
use std::sync::Arc;
use std::sync::{atomic::AtomicBool, Mutex};

/// Cloneable Arc handles for I/O without holding the global port map lock.
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

/// Open serial port with optional background RX hub (poll loop on all platforms).
pub struct ConnectedPort {
    pub port: Arc<Mutex<Box<dyn SerialPort>>>,
    pub rx_hub: Arc<Mutex<Option<Arc<PortRxHub>>>>,
    pub mux: Arc<Mutex<Option<Arc<CmuxSession>>>>,
    pub virtual_dlci: Option<u8>,
    pub physical_path: Option<String>,
    pub exchange_cancel: Arc<AtomicBool>,
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

pub type PhysicalPortRef = ConnectedPort;
pub type PhysicalPortHandle = ConnectedPortHandle;

/// Lifecycle state for a managed port.
pub enum PortState {
    Closed,
    Opening,
    Connected(ConnectedPort),
}

impl PortState {
    pub fn not_connected_reason(&self) -> String {
        match self {
            PortState::Closed => "Port is closed".to_string(),
            PortState::Opening => "Port is still opening".to_string(),
            PortState::Connected(_) => "Port is connected".to_string(),
        }
    }
}

/// Per-port state container.
pub struct SerialportInfo {
    pub state: PortState,
}

impl SerialportInfo {
    pub fn new(port: Box<dyn SerialPort>) -> Self {
        Self {
            state: PortState::Connected(ConnectedPort::new(port)),
        }
    }

    #[cfg(test)]
    pub(crate) fn connected_port_mut(&mut self) -> Option<&mut ConnectedPort> {
        match &mut self.state {
            PortState::Connected(cp) => Some(cp),
            _ => None,
        }
    }
}

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

#[cfg(test)]
impl ConnectedPort {
    pub fn test_port_mut(&self) -> std::sync::MutexGuard<'_, Box<dyn SerialPort>> {
        self.port.lock().unwrap()
    }
}
