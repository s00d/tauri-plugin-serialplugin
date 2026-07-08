//! CDC ACM driver.

use super::{line_coding_bytes, Driver, EndpointPair, ModemStatus, WRITE_TIMEOUT_MS};
use crate::config::{FlowControl, LineConfig, PurgeKind};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::transport::{ControlRequest, SharedTransport, USB_RECIP_INTERFACE, USB_TYPE_CLASS};

const USB_SUBCLASS_ACM: u8 = 2;
const SET_LINE_CODING: u8 = 0x20;
const SET_CONTROL_LINE_STATE: u8 = 0x22;
const SEND_BREAK: u8 = 0x23;

pub struct CdcAcmDriver {
    port_index: usize,
    control_index: u8,
    control_iface: u8,
    data_iface: u8,
    dtr: bool,
    rts: bool,
    endpoints: Option<EndpointPair>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
}

impl CdcAcmDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            control_index: 0,
            control_iface: 0,
            data_iface: 0,
            dtr: false,
            rts: false,
            endpoints: None,
            transport: None,
            reader: None,
        }
    }

    fn acm_control(&self, request: u8, value: u16, data: Vec<u8>) -> Result<()> {
        let transport = self.transport.as_ref().unwrap();
        let req = ControlRequest {
            request_type: USB_TYPE_CLASS | USB_RECIP_INTERFACE,
            request,
            value,
            index: self.control_index as u16,
            data,
            timeout_ms: WRITE_TIMEOUT_MS,
        };
        transport.control_out(&req)?;
        Ok(())
    }

    fn resolve_interfaces(&mut self, transport: &SharedTransport) -> Result<()> {
        let ifaces = transport.interfaces();
        let desc = transport.raw_device_descriptor();
        let is_iad = desc.len() >= 7 && desc[4] == 0xEF && desc[5] == 0x02 && desc[6] == 0x01;
        if is_iad {
            if let Some((ctrl, data)) = resolve_iad_pair(transport, self.port_index) {
                self.control_iface = ctrl;
                self.data_iface = data;
                self.control_index = ctrl;
                return Ok(());
            }
        }
        let comm: Vec<u8> = ifaces
            .iter()
            .filter(|i| i.class == 2 && i.subclass == USB_SUBCLASS_ACM)
            .map(|i| i.id)
            .collect();
        let data: Vec<u8> = ifaces
            .iter()
            .filter(|i| i.class == 10)
            .map(|i| i.id)
            .collect();
        if comm.is_empty() && data.is_empty() {
            // single-interface castrated ACM
            if let Some(iface) = ifaces.first() {
                self.control_iface = iface.id;
                self.data_iface = iface.id;
                self.control_index = iface.id;
                return Ok(());
            }
            return Err(UsbSerialError::ProbeFailed("no CDC interfaces".into()));
        }
        if comm.is_empty() {
            return Err(UsbSerialError::ProbeFailed("no CDC comm interfaces".into()));
        }
        let idx = self.port_index.min(comm.len() - 1);
        self.control_iface = comm[idx];
        self.data_iface = data.get(idx).copied().unwrap_or(comm[idx]);
        self.control_index = self.control_iface;
        Ok(())
    }
}

fn resolve_iad_pair(transport: &SharedTransport, port_index: usize) -> Option<(u8, u8)> {
    let raw = transport.raw_descriptors();
    let mut iad_ports: Vec<(u8, u8)> = Vec::new();
    let mut pos = 0usize;
    while pos + 2 <= raw.len() {
        let len = raw[pos] as usize;
        if len < 2 || pos + len > raw.len() {
            break;
        }
        if raw[pos + 1] == 0x0B && len >= 8 && raw[pos + 4] == 2 && raw[pos + 5] == 2 {
            let first = raw[pos + 2];
            let count = raw[pos + 3];
            if count >= 2 {
                iad_ports.push((first, first + 1));
            }
        }
        pos += len;
    }
    iad_ports.get(port_index).copied()
}

impl Driver for CdcAcmDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        self.resolve_interfaces(transport)?;
        transport.claim_interface(self.control_iface)?;
        if self.data_iface != self.control_iface {
            transport.claim_interface(self.data_iface)?;
        }
        self.endpoints = Some(EndpointPair::open(transport, self.data_iface)?);
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
        if let Some(t) = &self.transport {
            let _ = t.release_interface(self.data_iface);
            if self.control_iface != self.data_iface {
                let _ = t.release_interface(self.control_iface);
            }
        }
        self.endpoints = None;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<usize> {
        let transport = self.transport.as_ref().unwrap();
        self.endpoints.as_mut().unwrap().write(transport, data)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(reader) = &mut self.reader {
            return reader.try_read(buf);
        }
        Ok(0)
    }

    fn set_line_config(&mut self, cfg: LineConfig) -> Result<()> {
        self.acm_control(SET_LINE_CODING, 0, line_coding_bytes(&cfg).to_vec())
    }

    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        if flow == FlowControl::None {
            Ok(())
        } else {
            Err(UsbSerialError::Unsupported("flow control".into()))
        }
    }

    fn set_dtr(&mut self, value: bool) -> Result<()> {
        self.dtr = value;
        let v = (self.rts as u16) << 1 | (self.dtr as u16);
        self.acm_control(SET_CONTROL_LINE_STATE, v, vec![])
    }

    fn set_rts(&mut self, value: bool) -> Result<()> {
        self.rts = value;
        let v = (self.rts as u16) << 1 | (self.dtr as u16);
        self.acm_control(SET_CONTROL_LINE_STATE, v, vec![])
    }

    fn set_break(&mut self, enabled: bool) -> Result<()> {
        self.acm_control(SEND_BREAK, if enabled { 0xffff } else { 0 }, vec![])
    }

    fn purge(&mut self, _kind: PurgeKind) -> Result<()> {
        Ok(())
    }

    fn modem_status(&mut self) -> Result<ModemStatus> {
        Ok(ModemStatus::default())
    }

    fn bulk_in_mps(&self) -> u16 {
        self.endpoints.as_ref().map(|e| e.mps).unwrap_or(64)
    }

    fn take_bulk_in(&mut self) -> Option<Box<dyn crate::transport::BulkIn>> {
        let transport = self.transport.as_ref()?;
        self.endpoints.as_mut()?.take_in(transport)
    }
}
