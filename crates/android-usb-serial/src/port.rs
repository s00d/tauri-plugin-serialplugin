//! High-level serial port handle.

use crate::config::{FlowControl, LineConfig, PurgeKind};
use crate::drivers::{Driver, ModemStatus};
use crate::error::Result;
use crate::reader::SerialReader;
use crate::transport::SharedTransport;

pub struct SerialPortHandle {
    transport: SharedTransport,
    driver: Box<dyn Driver>,
    reader: Option<SerialReader>,
    closed: bool,
}

impl SerialPortHandle {
    pub(crate) fn new(transport: SharedTransport, driver: Box<dyn Driver>) -> Self {
        Self {
            transport,
            driver,
            reader: None,
            closed: false,
        }
    }

    pub fn write(&mut self, data: &[u8]) -> Result<usize> {
        self.driver.write(data)
    }

    pub fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        self.driver.read(buf)
    }

    pub fn set_line_config(&mut self, cfg: LineConfig) -> Result<()> {
        self.driver.set_line_config(cfg)
    }

    pub fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        self.driver.set_flow_control(flow)
    }

    pub fn set_dtr(&mut self, value: bool) -> Result<()> {
        self.driver.set_dtr(value)
    }

    pub fn set_rts(&mut self, value: bool) -> Result<()> {
        self.driver.set_rts(value)
    }

    pub fn set_break(&mut self, enabled: bool) -> Result<()> {
        self.driver.set_break(enabled)
    }

    /// Clear RX and/or TX driver buffers (host-side purge).
    pub fn purge(&mut self, kind: PurgeKind) -> Result<()> {
        self.driver.purge(kind)
    }

    /// Alias for [`Self::purge`] (clear input/output buffers).
    pub fn clear(&mut self, kind: PurgeKind) -> Result<()> {
        self.purge(kind)
    }

    pub fn modem_status(&mut self) -> Result<ModemStatus> {
        self.driver.modem_status()
    }

    /// Stop the bulk-IN reader, close the driver, then mark closed (idempotent).
    pub fn close(&mut self) {
        if self.closed {
            return;
        }
        self.stop_reader();
        let _ = self.driver.close();
        self.closed = true;
    }

    pub fn start_reader(&mut self) -> Result<()> {
        if self.reader.is_some() {
            return Ok(());
        }
        let reader = self.driver.start_reader()?;
        self.reader = Some(reader);
        Ok(())
    }

    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(reader) = &mut self.reader {
            return reader.try_read(buf);
        }
        self.read(buf)
    }

    pub fn stop_reader(&mut self) {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
    }

    pub fn transport(&self) -> &SharedTransport {
        &self.transport
    }
}

impl Drop for SerialPortHandle {
    fn drop(&mut self) {
        self.close();
    }
}
