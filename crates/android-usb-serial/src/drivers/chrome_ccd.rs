//! Google Chrome OS CCD (3 ports — interface N = port N, no CDC init).

use super::{Driver, EndpointPair, ModemStatus};
use crate::config::{FlowControl, LineConfig, PurgeKind};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::transport::SharedTransport;

pub struct ChromeCcdDriver {
    port_index: usize,
    iface: u8,
    endpoints: Option<EndpointPair>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
}

impl ChromeCcdDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            iface: port_index as u8,
            endpoints: None,
            transport: None,
            reader: None,
        }
    }
}

impl Driver for ChromeCcdDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        self.iface = self.port_index as u8;
        transport.claim_interface(self.iface)?;
        self.endpoints = Some(EndpointPair::open(transport, self.iface)?);
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
        if let Some(t) = &self.transport {
            let _ = t.release_interface(self.iface);
        }
        self.endpoints = None;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<usize> {
        let t = self.transport.as_ref().unwrap();
        self.endpoints.as_mut().unwrap().write(t, data)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(reader) = &mut self.reader {
            return reader.try_read(buf);
        }
        Ok(0)
    }

    fn set_line_config(&mut self, _cfg: LineConfig) -> Result<()> {
        Err(UsbSerialError::Unsupported("line config".into()))
    }

    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        if flow != FlowControl::None {
            return Err(UsbSerialError::Unsupported("flow control".into()));
        }
        Ok(())
    }

    fn set_dtr(&mut self, _value: bool) -> Result<()> {
        Err(UsbSerialError::Unsupported("dtr".into()))
    }

    fn set_rts(&mut self, _value: bool) -> Result<()> {
        Err(UsbSerialError::Unsupported("rts".into()))
    }

    fn set_break(&mut self, _enabled: bool) -> Result<()> {
        Err(UsbSerialError::Unsupported("break".into()))
    }

    fn purge(&mut self, _kind: PurgeKind) -> Result<()> {
        Err(UsbSerialError::Unsupported("purge".into()))
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
