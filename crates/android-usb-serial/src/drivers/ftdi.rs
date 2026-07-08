//! FTDI driver (vendor control + 2-byte RX header strip).

use super::{Driver, EndpointPair, ModemStatus};
use crate::config::{
    DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits, CHAR_XOFF, CHAR_XON,
};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::rx_filter::{FtdiHeaderFilter, XonXoffRxFilter};
use crate::transport::{ControlRequest, SharedTransport};

pub struct FtdiDriver {
    port_index: usize,
    iface: u8,
    dtr: bool,
    rts: bool,
    baud_with_port: bool,
    flow: FlowControl,
    break_enabled: bool,
    data_config: u16,
    endpoints: Option<EndpointPair>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
}

impl FtdiDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            iface: port_index as u8,
            dtr: false,
            rts: false,
            baud_with_port: false,
            flow: FlowControl::None,
            break_enabled: false,
            data_config: 0,
            endpoints: None,
            transport: None,
            reader: None,
        }
    }

    fn w_index(&self) -> u16 {
        (self.port_index + 1) as u16
    }

    fn vendor_out(&self, request: u8, value: u16, index: u16) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest::vendor_out(request, value, index, vec![]))?;
        Ok(())
    }

    fn vendor_in(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>> {
        self.transport
            .as_ref()
            .unwrap()
            .control_in(&ControlRequest::vendor_in(request, value, index, len))
    }
}

impl Driver for FtdiDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        self.iface = self.port_index as u8;
        transport.claim_interface(self.iface)?;
        self.endpoints = Some(EndpointPair::open(transport, self.iface)?);
        self.vendor_out(0, 0, self.w_index())?;
        let modem = if self.dtr { 0x0101 } else { 0x0100 } | if self.rts { 0x0202 } else { 0x0200 };
        self.vendor_out(1, modem, self.w_index())?;
        self.set_flow_control(FlowControl::None)?;
        self.baud_with_port = transport.interfaces().len() > 1;
        let raw = transport.raw_descriptors();
        if raw.len() >= 14 {
            let device_type = raw[13];
            self.baud_with_port =
                self.baud_with_port || device_type == 7 || device_type == 8 || device_type == 9;
        }
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

    fn set_line_config(&mut self, cfg: LineConfig) -> Result<()> {
        self.set_baud(cfg.baud_rate)?;
        match cfg.data_bits {
            DataBits::Seven | DataBits::Eight => {}
            _ => return Err(UsbSerialError::Unsupported("data bits".into())),
        }
        if cfg.stop_bits == StopBits::OnePointFive {
            return Err(UsbSerialError::Unsupported("stop bits".into()));
        }
        self.data_config = ftdi_line_config_value(cfg, self.break_enabled);
        self.vendor_out(4, self.data_config, self.w_index())
    }

    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        self.flow = flow;
        let mut value = 0u16;
        let mut index = self.w_index();
        match flow {
            FlowControl::None => {}
            FlowControl::RtsCts => index |= 0x100,
            FlowControl::DtrDsr => index |= 0x200,
            FlowControl::XonXoffInline => {
                value = u16::from(CHAR_XON) | (u16::from(CHAR_XOFF) << 8);
                index |= 0x400;
            }
            _ => return Err(UsbSerialError::Unsupported("flow control".into())),
        }
        self.vendor_out(2, value, index)
    }

    fn set_dtr(&mut self, value: bool) -> Result<()> {
        self.dtr = value;
        let v = if value { 0x0101 } else { 0x0100 };
        self.vendor_out(1, v, self.w_index())
    }

    fn set_rts(&mut self, value: bool) -> Result<()> {
        self.rts = value;
        let v = if value { 0x0202 } else { 0x0200 };
        self.vendor_out(1, v, self.w_index())
    }

    fn set_break(&mut self, enabled: bool) -> Result<()> {
        self.break_enabled = enabled;
        if enabled {
            self.data_config |= 0x4000;
        } else {
            self.data_config &= !0x4000;
        }
        self.vendor_out(4, self.data_config, self.w_index())
    }

    fn purge(&mut self, kind: PurgeKind) -> Result<()> {
        match kind {
            PurgeKind::Rx => self.vendor_out(0, 1, self.w_index()),
            PurgeKind::Tx => self.vendor_out(0, 2, self.w_index()),
            PurgeKind::Both => {
                self.vendor_out(0, 1, self.w_index())?;
                self.vendor_out(0, 2, self.w_index())
            }
        }
    }

    fn modem_status(&mut self) -> Result<ModemStatus> {
        let data = self.vendor_in(5, 0, self.w_index(), 2)?;
        let status = data.first().copied().unwrap_or(0);
        Ok(ModemStatus {
            cts: status & 0x10 != 0,
            dsr: status & 0x20 != 0,
            ri: status & 0x40 != 0,
            cd: status & 0x80 != 0,
        })
    }

    fn bulk_in_mps(&self) -> u16 {
        self.endpoints.as_ref().map(|e| e.mps).unwrap_or(64)
    }

    fn take_bulk_in(&mut self) -> Option<Box<dyn crate::transport::BulkIn>> {
        let transport = self.transport.as_ref()?;
        self.endpoints.as_mut()?.take_in(transport)
    }

    fn rx_filters(&self) -> Vec<Box<dyn crate::rx_filter::RxFilter>> {
        let mut filters: Vec<Box<dyn crate::rx_filter::RxFilter>> =
            vec![Box::new(FtdiHeaderFilter::new(self.bulk_in_mps()))];
        if self.flow == FlowControl::XonXoffInline {
            filters.push(Box::new(XonXoffRxFilter::new(true)));
        }
        filters
    }
}

impl FtdiDriver {
    fn set_baud(&self, baud: u32) -> Result<()> {
        if baud > 3_500_000 {
            return Err(UsbSerialError::Unsupported("baud too high".into()));
        }
        let (value, index) = ftdi_baud_encoding(baud, self.baud_with_port, self.port_index)?;
        self.vendor_out(3, value, index)
    }
}

fn ftdi_line_config_value(cfg: LineConfig, break_enabled: bool) -> u16 {
    let mut config = match cfg.data_bits {
        DataBits::Seven => 7,
        DataBits::Eight => 8,
        _ => 0,
    };
    config |= match cfg.parity {
        Parity::None => 0,
        Parity::Odd => 0x100,
        Parity::Even => 0x200,
        Parity::Mark => 0x300,
        Parity::Space => 0x400,
    };
    if cfg.stop_bits == StopBits::Two {
        config |= 0x1000;
    }
    if break_enabled {
        config |= 0x4000;
    }
    config
}

/// FTDI baud encoding (ported from FtdiSerialDriver.java).
pub fn ftdi_baud_encoding(
    baud: u32,
    baud_with_port: bool,
    port_index: usize,
) -> Result<(u16, u16)> {
    let (divisor, subdivisor, _) = if baud >= 2_500_000 {
        (0u32, 0u32, 3_000_000u32)
    } else if baud >= 1_750_000 {
        (1, 0, 2_000_000)
    } else {
        let mut d = (24_000_000u32 << 1) / baud;
        d = (d + 1) >> 1;
        let sub = d & 0x07;
        d >>= 3;
        if d > 0x3fff {
            return Err(UsbSerialError::Unsupported("baud too low".into()));
        }
        let effective = (24_000_000u32 << 1) / ((d << 3) + sub);
        let effective = (effective + 1) >> 1;
        let err = (1.0 - (effective as f64 / baud as f64)).abs();
        if err >= 0.031 {
            return Err(UsbSerialError::Unsupported(format!(
                "baud deviation {:.1}%",
                err * 100.0
            )));
        }
        (d, sub, effective)
    };
    let mut value = divisor as u16;
    let mut index = 0u16;
    match subdivisor {
        0 => {}
        4 => value |= 0x4000,
        2 => value |= 0x8000,
        1 => value |= 0xc000,
        3 => index |= 1,
        5 => {
            value |= 0x4000;
            index |= 1;
        }
        6 => {
            value |= 0x8000;
            index |= 1;
        }
        7 => {
            value |= 0xc000;
            index |= 1;
        }
        _ => {}
    }
    if baud_with_port {
        index = (index << 8) | (port_index as u16 + 1);
    }
    Ok((value, index))
}
