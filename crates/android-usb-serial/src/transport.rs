//! USB transport abstraction (nusb or fake).
//!
//! Drivers talk only through [`Transport`]. Production code uses
//! [`crate::NusbTransport`]; tests use [`crate::FakeTransport`] behind `fake-transport`.

use crate::error::{ReadOutcome, Result};
use std::sync::Arc;

/// USB request direction IN bit.
pub const USB_DIR_IN: u8 = 0x80;
/// USB request direction OUT.
pub const USB_DIR_OUT: u8 = 0x00;
/// bmRequestType type = class.
pub const USB_TYPE_CLASS: u8 = 0x20;
/// bmRequestType recipient = interface.
pub const USB_RECIP_INTERFACE: u8 = 0x01;
/// bmRequestType recipient = device.
pub const USB_RECIP_DEVICE: u8 = 0x00;

/// USB interface class/subclass/protocol summary used for probing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterfaceInfo {
    pub id: u8,
    pub class: u8,
    pub subclass: u8,
    pub protocol: u8,
}

/// Endpoint descriptor fields needed by drivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndpointInfo {
    pub address: u8,
    pub attributes: u8,
    pub max_packet_size: u16,
    pub interval: u8,
}

impl EndpointInfo {
    /// IN vs OUT from the address bit.
    pub fn direction(&self) -> u8 {
        self.address & USB_DIR_IN
    }

    pub fn is_bulk_in(&self) -> bool {
        self.direction() == USB_DIR_IN && (self.attributes & 0x03) == 2
    }

    pub fn is_bulk_out(&self) -> bool {
        self.direction() == USB_DIR_OUT && (self.attributes & 0x03) == 2
    }

    pub fn is_interrupt_in(&self) -> bool {
        self.direction() == USB_DIR_IN && (self.attributes & 0x03) == 3
    }
}

/// USB control transfer request.
#[derive(Debug, Clone)]
pub struct ControlRequest {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub data: Vec<u8>,
    pub timeout_ms: u32,
}

impl ControlRequest {
    /// Vendor OUT (host → device), `bmRequestType = 0x40`.
    pub fn vendor_out(request: u8, value: u16, index: u16, data: Vec<u8>) -> Self {
        Self {
            request_type: 0x40,
            request,
            value,
            index,
            data,
            timeout_ms: 5000,
        }
    }

    /// Vendor IN; `data` length is the wLength buffer size.
    pub fn vendor_in(request: u8, value: u16, index: u16, length: usize) -> Self {
        Self {
            request_type: 0xC0,
            request,
            value,
            index,
            data: vec![0; length],
            timeout_ms: 5000,
        }
    }

    /// Class OUT to interface.
    pub fn class_out(request: u8, value: u16, index: u16, data: Vec<u8>) -> Self {
        Self {
            request_type: USB_TYPE_CLASS | USB_RECIP_INTERFACE,
            request,
            value,
            index,
            data,
            timeout_ms: 5000,
        }
    }

    /// Class IN from interface.
    pub fn class_in(request: u8, value: u16, index: u16, length: usize) -> Self {
        Self {
            request_type: USB_TYPE_CLASS | USB_RECIP_INTERFACE | USB_DIR_IN,
            request,
            value,
            index,
            data: vec![0; length],
            timeout_ms: 5000,
        }
    }
}

/// Owned bulk (or interrupt) IN pipe.
pub trait BulkIn: Send {
    fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<ReadOutcome>;
    fn cancel_all(&mut self);
    fn clear_halt(&mut self) -> Result<()>;
}

/// Owned bulk OUT pipe.
pub trait BulkOut: Send {
    fn write(&mut self, data: &[u8], timeout_ms: u32) -> Result<usize>;
    fn clear_halt(&mut self) -> Result<()>;
}

/// USB device view used by all chip drivers.
pub trait Transport: Send + Sync {
    fn raw_device_descriptor(&self) -> [u8; 18];
    fn raw_descriptors(&self) -> Vec<u8>;
    fn device_class(&self) -> u8;
    fn interfaces(&self) -> Vec<InterfaceInfo>;
    fn endpoints(&self, interface: u8) -> Vec<EndpointInfo>;
    fn claim_interface(&self, interface: u8) -> Result<()>;
    fn release_interface(&self, interface: u8) -> Result<()>;
    fn control_out(&self, req: &ControlRequest) -> Result<usize>;
    fn control_in(&self, req: &ControlRequest) -> Result<Vec<u8>>;
    fn open_bulk_in(&self, endpoint: u8, max_packet_size: u16) -> Result<Box<dyn BulkIn>>;
    fn open_bulk_out(&self, endpoint: u8, max_packet_size: u16) -> Result<Box<dyn BulkOut>>;
    fn open_interrupt_in(&self, endpoint: u8, max_packet_size: u16) -> Result<Box<dyn BulkIn>>;
}

/// Shared ownership of a [`Transport`] (typically wrapped once per open).
pub type SharedTransport = Arc<dyn Transport>;

/// Parse USB control setup fields from `request_type`.
pub fn parse_control_recipient(request_type: u8) -> (u8, bool) {
    let direction_in = request_type & 0x80 != 0;
    let recipient = request_type & 0x1f;
    (recipient, direction_in)
}

/// Device recipient in `bmRequestType`.
pub fn is_device_recipient(request_type: u8) -> bool {
    (request_type & 0x1f) == USB_RECIP_DEVICE
}

/// Interface recipient in `bmRequestType`.
pub fn is_interface_recipient(request_type: u8) -> bool {
    (request_type & 0x1f) == USB_RECIP_INTERFACE
}
