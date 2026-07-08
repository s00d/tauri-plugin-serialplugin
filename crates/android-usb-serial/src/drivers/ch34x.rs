//! WCH CH34x driver.

use super::{Driver, EndpointPair, ModemStatus, WRITE_TIMEOUT_MS};
use crate::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::transport::{ControlRequest, SharedTransport};

const LCR_ENABLE_RX: u16 = 0x80;
const LCR_ENABLE_TX: u16 = 0x40;
const LCR_MARK_SPACE: u16 = 0x20;
const LCR_PAR_EVEN: u16 = 0x10;
const LCR_ENABLE_PAR: u16 = 0x08;
const LCR_STOP_BITS_2: u16 = 0x04;
const LCR_CS8: u16 = 0x03;
const LCR_CS7: u16 = 0x02;
const LCR_CS6: u16 = 0x01;
const LCR_CS5: u16 = 0x00;

const GCL_CTS: u8 = 0x01;
const GCL_DSR: u8 = 0x02;
const GCL_RI: u8 = 0x04;
const GCL_CD: u8 = 0x08;
const SCL_DTR: u16 = 0x20;
const SCL_RTS: u16 = 0x40;

const DEFAULT_BAUD_RATE: u32 = 9600;

pub struct Ch34xDriver {
    #[allow(dead_code)]
    port_index: usize,
    iface: u8,
    dtr: bool,
    rts: bool,
    endpoints: Option<EndpointPair>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
}

impl Ch34xDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            iface: 0,
            dtr: false,
            rts: false,
            endpoints: None,
            transport: None,
            reader: None,
        }
    }

    fn vendor_out(&self, request: u8, value: u16, index: u16) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest {
                request_type: 0x40,
                request,
                value,
                index,
                data: vec![],
                timeout_ms: WRITE_TIMEOUT_MS,
            })
            .map(|_| ())
    }

    fn vendor_in(&self, request: u8, value: u16, index: u16, len: usize) -> Result<Vec<u8>> {
        self.transport
            .as_ref()
            .unwrap()
            .control_in(&ControlRequest::vendor_in(request, value, index, len))
    }

    fn check_state(&self, request: u8, value: u16, expected: &[Option<u8>]) -> Result<()> {
        let buf = self.vendor_in(request, value, 0, expected.len())?;
        if buf.len() != expected.len() {
            return Err(UsbSerialError::Io(format!(
                "checkState: expected {} bytes, got {}",
                expected.len(),
                buf.len()
            )));
        }
        for (i, exp) in expected.iter().enumerate() {
            if let Some(want) = exp {
                if buf[i] != *want {
                    return Err(UsbSerialError::Io(format!(
                        "checkState byte[{i}]: expected 0x{want:02x}, got 0x{:02x}",
                        buf[i]
                    )));
                }
            }
        }
        Ok(())
    }

    fn get_status(&self) -> Result<u8> {
        let buf = self.vendor_in(0x95, 0x0706, 0, 2)?;
        Ok(buf[0])
    }
}

impl Driver for Ch34xDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        for iface in transport.interfaces() {
            transport.claim_interface(iface.id)?;
        }
        self.iface = transport.interfaces().last().map(|i| i.id).unwrap_or(0);
        self.endpoints = Some(EndpointPair::open(transport, self.iface)?);
        self.initialize()?;
        self.set_baud(DEFAULT_BAUD_RATE)?;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
        if let Some(t) = &self.transport {
            for iface in t.interfaces() {
                let _ = t.release_interface(iface.id);
            }
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
        if cfg.baud_rate == 0 {
            return Err(UsbSerialError::Unsupported("invalid baud rate".into()));
        }
        self.set_baud(cfg.baud_rate)?;

        let mut lcr = LCR_ENABLE_RX | LCR_ENABLE_TX;
        lcr |= match cfg.data_bits {
            DataBits::Five => LCR_CS5,
            DataBits::Six => LCR_CS6,
            DataBits::Seven => LCR_CS7,
            DataBits::Eight => LCR_CS8,
        };
        if cfg.stop_bits == StopBits::Two {
            lcr |= LCR_STOP_BITS_2;
        } else if cfg.stop_bits == StopBits::OnePointFive {
            return Err(UsbSerialError::Unsupported("stop bits 1.5".into()));
        }
        lcr |= match cfg.parity {
            Parity::None => 0,
            Parity::Odd => LCR_ENABLE_PAR,
            Parity::Even => LCR_ENABLE_PAR | LCR_PAR_EVEN,
            Parity::Mark => LCR_ENABLE_PAR | LCR_MARK_SPACE,
            Parity::Space => LCR_ENABLE_PAR | LCR_MARK_SPACE | LCR_PAR_EVEN,
        };
        self.vendor_out(0x9A, 0x2518, lcr)
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
        self.set_control_lines()
    }

    fn set_rts(&mut self, value: bool) -> Result<()> {
        self.rts = value;
        self.set_control_lines()
    }

    fn set_break(&mut self, enabled: bool) -> Result<()> {
        let mut req = self.vendor_in(0x95, 0x1805, 0, 2)?;
        if enabled {
            req[0] &= !1;
            req[1] &= !0x40;
        } else {
            req[0] |= 1;
            req[1] |= 0x40;
        }
        let val = ((req[1] as u16) << 8) | (req[0] as u16);
        self.vendor_out(0x9A, 0x1805, val)
    }

    fn purge(&mut self, _kind: PurgeKind) -> Result<()> {
        Ok(())
    }

    fn modem_status(&mut self) -> Result<ModemStatus> {
        let status = self.get_status()?;
        Ok(ModemStatus {
            cts: (status & GCL_CTS) == 0,
            dsr: (status & GCL_DSR) == 0,
            ri: (status & GCL_RI) == 0,
            cd: (status & GCL_CD) == 0,
        })
    }

    fn bulk_in_mps(&self) -> u16 {
        self.endpoints.as_ref().map(|e| e.mps).unwrap_or(64)
    }

    fn take_bulk_in(&mut self) -> Option<Box<dyn crate::transport::BulkIn>> {
        let transport = self.transport.as_ref()?;
        self.endpoints.as_mut()?.take_in(transport)
    }
}

impl Ch34xDriver {
    fn initialize(&self) -> Result<()> {
        self.check_state(0x5F, 0, &[None, Some(0x00)])?;
        self.vendor_out(0xA1, 0, 0)?;
        self.set_baud(DEFAULT_BAUD_RATE)?;
        self.check_state(0x95, 0x2518, &[None, Some(0x00)])?;
        self.vendor_out(0x9A, 0x2518, LCR_ENABLE_RX | LCR_ENABLE_TX | LCR_CS8)?;
        self.check_state(0x95, 0x0706, &[None, None])?;
        self.vendor_out(0xA1, 0x501F, 0xD90A)?;
        self.set_baud(DEFAULT_BAUD_RATE)?;
        self.set_control_lines()?;
        self.check_state(0x95, 0x0706, &[None, None])?;
        Ok(())
    }

    fn set_baud(&self, baud: u32) -> Result<()> {
        let (val1, val2) = if baud == 921_600 {
            let divisor = 7u16 | 0x0080;
            let factor = 0xF300u16;
            let val1 = (factor & 0xFF00) | divisor;
            let val2 = factor & 0xFF;
            (val1, val2)
        } else {
            const BAUDBASE_FACTOR: u64 = 1_532_620_800;
            const BAUDBASE_DIVMAX: u32 = 3;

            let mut factor = BAUDBASE_FACTOR / baud as u64;
            let mut divisor = BAUDBASE_DIVMAX;
            while factor > 0xfff0 && divisor > 0 {
                factor >>= 3;
                divisor -= 1;
            }
            if factor > 0xfff0 {
                return Err(UsbSerialError::Unsupported(format!(
                    "unsupported baud rate: {baud}"
                )));
            }
            factor = 0x10000 - factor;
            let divisor = divisor | 0x0080;
            let val1 = ((factor & 0xff00) as u16) | divisor as u16;
            let val2 = (factor & 0xff) as u16;
            (val1, val2)
        };
        self.vendor_out(0x9A, 0x1312, val1)?;
        self.vendor_out(0x9A, 0x0F2C, val2)
    }

    fn set_control_lines(&self) -> Result<()> {
        let mut value = 0u16;
        if self.dtr {
            value |= SCL_DTR;
        }
        if self.rts {
            value |= SCL_RTS;
        }
        self.vendor_out(0xA4, !value, 0)
    }
}
