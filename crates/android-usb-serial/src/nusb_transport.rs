//! nusb-backed transport (Linux / Android).

use crate::error::{ReadOutcome, Result, TransferError, UsbSerialError};
use crate::transport::{
    parse_control_recipient, BulkIn, BulkOut, ControlRequest, EndpointInfo, InterfaceInfo,
    Transport,
};
use nusb::io::EndpointRead;
use nusb::transfer::{
    Bulk, ControlIn, ControlOut, ControlType, In, Interrupt, Out, Recipient,
    TransferError as NusbXfer,
};
use nusb::{Device, Interface, MaybeFuture};
use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
use std::sync::Mutex;
use std::time::Duration;

/// Fewer in-flight bulk IN URBs on Android OTG reduces EPROTO/detach on weak ports (CH340).
const IN_FLIGHT_TRANSFERS: usize = 2;

/// Duplicate an Android `UsbDeviceConnection` fd and open via nusb.
pub fn from_raw_fd(fd: RawFd) -> Result<Device> {
    let dup_fd = unsafe { libc::dup(fd) };
    if dup_fd < 0 {
        return Err(UsbSerialError::Io(format!(
            "dup failed: {}",
            std::io::Error::last_os_error()
        )));
    }
    let owned = unsafe { OwnedFd::from_raw_fd(dup_fd) };
    Device::from_fd(owned)
        .wait()
        .map_err(|e| UsbSerialError::Io(e.to_string()))
}

pub struct NusbTransport {
    device: Device,
    claimed: Mutex<HashMap<u8, Interface>>,
    interfaces: Vec<InterfaceInfo>,
    endpoints: HashMap<u8, Vec<EndpointInfo>>,
    device_descriptor: [u8; 18],
    raw_config_descriptors: Vec<u8>,
}

impl NusbTransport {
    pub fn from_device(device: Device) -> Result<Self> {
        let config = device
            .active_configuration()
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        let raw_config_descriptors = config.as_bytes().to_vec();
        let mut interfaces = Vec::new();
        let mut endpoints = HashMap::new();
        for iface_group in config.interfaces() {
            let info = iface_group.first_alt_setting();
            let id = iface_group.interface_number();
            interfaces.push(InterfaceInfo {
                id,
                class: info.class(),
                subclass: info.subclass(),
                protocol: info.protocol(),
            });
            let eps: Vec<EndpointInfo> = info
                .endpoints()
                .map(|ep| EndpointInfo {
                    address: ep.address(),
                    attributes: ep.attributes(),
                    max_packet_size: ep.max_packet_size() as u16,
                    interval: ep.interval(),
                })
                .collect();
            endpoints.insert(id, eps);
        }
        let mut device_descriptor = [0u8; 18];
        let desc = device.device_descriptor();
        let raw = desc.as_bytes();
        let n = raw.len().min(18);
        device_descriptor[..n].copy_from_slice(&raw[..n]);
        Ok(Self {
            device,
            claimed: Mutex::new(HashMap::new()),
            interfaces,
            endpoints,
            device_descriptor,
            raw_config_descriptors,
        })
    }

    fn interface(&self, interface: u8) -> Result<Interface> {
        let mut claimed = self.claimed.lock().unwrap();
        if let Some(iface) = claimed.get(&interface) {
            return Ok(iface.clone());
        }
        let iface = self
            .device
            .detach_and_claim_interface(interface)
            .wait()
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        claimed.insert(interface, iface.clone());
        Ok(iface)
    }
}

struct NusbPipelinedIn<EpType: nusb::transfer::BulkOrInterrupt> {
    reader: Mutex<Option<EndpointRead<EpType>>>,
    bufsize: usize,
    default_timeout_ms: u32,
}

impl<EpType: nusb::transfer::BulkOrInterrupt> NusbPipelinedIn<EpType> {
    fn new(endpoint: nusb::Endpoint<EpType, In>, bufsize: usize, timeout_ms: u32) -> Self {
        let reader = endpoint
            .reader(bufsize)
            .with_num_transfers(IN_FLIGHT_TRANSFERS)
            .with_read_timeout(Duration::from_millis(timeout_ms as u64));
        Self {
            reader: Mutex::new(Some(reader)),
            bufsize,
            default_timeout_ms: timeout_ms,
        }
    }

    fn read_inner(&self, buf: &mut [u8], timeout_ms: u32) -> Result<ReadOutcome> {
        let mut guard = self.reader.lock().unwrap();
        let reader = guard
            .as_mut()
            .ok_or_else(|| UsbSerialError::Io("endpoint closed".into()))?;
        reader.set_read_timeout(Duration::from_millis(timeout_ms as u64));
        match reader.read(buf) {
            Ok(0) => Ok(ReadOutcome::TimedOut),
            Ok(n) => Ok(ReadOutcome::Data(buf[..n].to_vec())),
            Err(e) if e.kind() == ErrorKind::TimedOut => Ok(ReadOutcome::TimedOut),
            Err(e) => Err(map_io_err(&e)),
        }
    }

    fn cancel_all(&self) {
        if let Ok(mut guard) = self.reader.lock() {
            if let Some(reader) = guard.as_mut() {
                reader.cancel_all();
            }
        }
    }

    fn clear_halt(&self) -> Result<()> {
        let mut guard = self.reader.lock().unwrap();
        let reader = guard
            .take()
            .ok_or_else(|| UsbSerialError::Io("endpoint closed".into()))?;
        let mut ep = reader.into_inner();
        ep.clear_halt()
            .wait()
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        let new_reader = ep
            .reader(self.bufsize)
            .with_num_transfers(IN_FLIGHT_TRANSFERS)
            .with_read_timeout(Duration::from_millis(self.default_timeout_ms as u64));
        *guard = Some(new_reader);
        Ok(())
    }
}

struct NusbBulkIn {
    inner: NusbPipelinedIn<Bulk>,
}

impl BulkIn for NusbBulkIn {
    fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<ReadOutcome> {
        self.inner.read_inner(buf, timeout_ms)
    }

    fn cancel_all(&mut self) {
        self.inner.cancel_all();
    }

    fn clear_halt(&mut self) -> Result<()> {
        self.inner.clear_halt()
    }
}

struct NusbInterruptIn {
    inner: NusbPipelinedIn<Interrupt>,
}

impl BulkIn for NusbInterruptIn {
    fn read(&mut self, buf: &mut [u8], timeout_ms: u32) -> Result<ReadOutcome> {
        self.inner.read_inner(buf, timeout_ms)
    }

    fn cancel_all(&mut self) {
        self.inner.cancel_all();
    }

    fn clear_halt(&mut self) -> Result<()> {
        self.inner.clear_halt()
    }
}

struct NusbBulkOut {
    ep: Mutex<nusb::Endpoint<Bulk, Out>>,
}

impl BulkOut for NusbBulkOut {
    fn write(&mut self, data: &[u8], timeout_ms: u32) -> Result<usize> {
        let mut ep = self.ep.lock().unwrap();
        let timeout = Duration::from_millis(timeout_ms as u64);
        let comp = ep.transfer_blocking(data.to_vec().into(), timeout);
        comp.status.map_err(|e| map_transfer_err(&e))?;
        Ok(comp.actual_len)
    }

    fn clear_halt(&mut self) -> Result<()> {
        let mut ep = self.ep.lock().unwrap();
        ep.clear_halt()
            .wait()
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        Ok(())
    }
}

fn map_transfer_err(e: &NusbXfer) -> UsbSerialError {
    match e {
        NusbXfer::Cancelled => UsbSerialError::from(TransferError::Cancelled),
        NusbXfer::Stall => UsbSerialError::from(TransferError::Stall),
        NusbXfer::Disconnected => UsbSerialError::from(TransferError::Disconnected),
        _ => UsbSerialError::Io(e.to_string()),
    }
}

fn map_io_err(e: &std::io::Error) -> UsbSerialError {
    if e.kind() == ErrorKind::TimedOut {
        return UsbSerialError::TimedOut;
    }
    let msg = e.to_string().to_lowercase();
    if msg.contains("stall") {
        return UsbSerialError::from(TransferError::Stall);
    }
    if msg.contains("disconnect") {
        return UsbSerialError::from(TransferError::Disconnected);
    }
    if msg.contains("cancel") {
        return UsbSerialError::from(TransferError::Cancelled);
    }
    UsbSerialError::Io(e.to_string())
}

/// Parse USB control setup fields (exported for unit tests on linux/android).
pub fn parse_control(req: &ControlRequest) -> (ControlType, Recipient, bool) {
    let (recipient_bits, direction_in) = parse_control_recipient(req.request_type);
    let control_type = match req.request_type & 0x60 {
        0x00 => ControlType::Standard,
        0x20 => ControlType::Class,
        _ => ControlType::Vendor,
    };
    let recipient = match recipient_bits {
        0x00 => Recipient::Device,
        0x01 => Recipient::Interface,
        0x02 => Recipient::Endpoint,
        _ => Recipient::Other,
    };
    (control_type, recipient, direction_in)
}

impl Transport for NusbTransport {
    fn raw_device_descriptor(&self) -> [u8; 18] {
        self.device_descriptor
    }

    fn raw_descriptors(&self) -> Vec<u8> {
        self.raw_config_descriptors.clone()
    }

    fn device_class(&self) -> u8 {
        self.device_descriptor[4]
    }

    fn interfaces(&self) -> Vec<InterfaceInfo> {
        self.interfaces.clone()
    }

    fn endpoints(&self, interface: u8) -> Vec<EndpointInfo> {
        self.endpoints.get(&interface).cloned().unwrap_or_default()
    }

    fn claim_interface(&self, interface: u8) -> Result<()> {
        let _ = self.interface(interface)?;
        Ok(())
    }

    fn release_interface(&self, interface: u8) -> Result<()> {
        self.claimed.lock().unwrap().remove(&interface);
        Ok(())
    }

    fn control_out(&self, req: &ControlRequest) -> Result<usize> {
        let (control_type, recipient, _) = parse_control(req);
        let timeout = Duration::from_millis(req.timeout_ms as u64);
        let data = ControlOut {
            control_type,
            recipient,
            request: req.request,
            value: req.value,
            index: req.index,
            data: &req.data,
        };
        match recipient {
            Recipient::Device => self
                .device
                .control_out(data, timeout)
                .wait()
                .map_err(|e| UsbSerialError::Io(e.to_string()))?,
            Recipient::Interface => {
                let iface_num = (req.index & 0xff) as u8;
                self.interface(iface_num)?
                    .control_out(data, timeout)
                    .wait()
                    .map_err(|e| UsbSerialError::Io(e.to_string()))?;
            }
            _ => {
                let iface_num = endpoint_interface(&self.endpoints, (req.index & 0xff) as u8)
                    .or(Ok::<u8, UsbSerialError>(0))?;
                self.interface(iface_num)?
                    .control_out(data, timeout)
                    .wait()
                    .map_err(|e| UsbSerialError::Io(e.to_string()))?;
            }
        }
        Ok(req.data.len())
    }

    fn control_in(&self, req: &ControlRequest) -> Result<Vec<u8>> {
        let (control_type, recipient, _) = parse_control(req);
        let timeout = Duration::from_millis(req.timeout_ms as u64);
        let length = req.data.len().min(u16::MAX as usize) as u16;
        let data = ControlIn {
            control_type,
            recipient,
            request: req.request,
            value: req.value,
            index: req.index,
            length,
        };
        let bytes = match recipient {
            Recipient::Device => self
                .device
                .control_in(data, timeout)
                .wait()
                .map_err(|e| UsbSerialError::Io(e.to_string()))?,
            Recipient::Interface => self
                .interface((req.index & 0xff) as u8)?
                .control_in(data, timeout)
                .wait()
                .map_err(|e| UsbSerialError::Io(e.to_string()))?,
            _ => self
                .interface(
                    endpoint_interface(&self.endpoints, (req.index & 0xff) as u8).unwrap_or(0),
                )?
                .control_in(data, timeout)
                .wait()
                .map_err(|e| UsbSerialError::Io(e.to_string()))?,
        };
        Ok(bytes)
    }

    fn open_bulk_in(&self, endpoint: u8, max_packet_size: u16) -> Result<Box<dyn BulkIn>> {
        let iface_num = endpoint_interface(&self.endpoints, endpoint)?;
        let iface = self.interface(iface_num)?;
        let ep = iface
            .endpoint::<Bulk, In>(endpoint)
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        let bufsize = (max_packet_size as usize).saturating_mul(4).max(64);
        Ok(Box::new(NusbBulkIn {
            inner: NusbPipelinedIn::new(ep, bufsize, 200),
        }))
    }

    fn open_bulk_out(&self, endpoint: u8, _max_packet_size: u16) -> Result<Box<dyn BulkOut>> {
        let iface_num = endpoint_interface(&self.endpoints, endpoint)?;
        let iface = self.interface(iface_num)?;
        let ep = iface
            .endpoint::<Bulk, Out>(endpoint)
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        Ok(Box::new(NusbBulkOut { ep: Mutex::new(ep) }))
    }

    fn open_interrupt_in(&self, endpoint: u8, max_packet_size: u16) -> Result<Box<dyn BulkIn>> {
        let iface_num = endpoint_interface(&self.endpoints, endpoint)?;
        let iface = self.interface(iface_num)?;
        let ep = iface
            .endpoint::<Interrupt, In>(endpoint)
            .map_err(|e| UsbSerialError::Io(e.to_string()))?;
        let bufsize = (max_packet_size as usize).max(8);
        Ok(Box::new(NusbInterruptIn {
            inner: NusbPipelinedIn::new(ep, bufsize, 200),
        }))
    }
}

fn endpoint_interface(endpoints: &HashMap<u8, Vec<EndpointInfo>>, ep_addr: u8) -> Result<u8> {
    for (iface, eps) in endpoints {
        if eps.iter().any(|e| e.address == ep_addr) {
            return Ok(*iface);
        }
    }
    Err(UsbSerialError::Io(format!(
        "endpoint {ep_addr:#x} not found"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::USB_RECIP_DEVICE;

    #[test]
    fn parse_vendor_device_out() {
        let req = ControlRequest::vendor_out(0x01, 0, 0, vec![]);
        let (_, recipient, dir_in) = parse_control(&req);
        assert!(!dir_in);
        assert_eq!(recipient, Recipient::Device);
        assert_eq!(req.request_type & USB_RECIP_DEVICE, 0);
    }

    #[test]
    fn parse_class_interface_in() {
        let req = ControlRequest::class_in(0x20, 0, 0, 7);
        let (ty, recipient, dir_in) = parse_control(&req);
        assert!(dir_in);
        assert_eq!(recipient, Recipient::Interface);
        assert_eq!(ty, ControlType::Class);
    }
}
