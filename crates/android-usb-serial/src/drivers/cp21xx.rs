//! Silicon Labs CP21xx driver.

use super::{Driver, EndpointPair, ModemStatus, WRITE_TIMEOUT_MS};
use crate::config::{
    DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits, CHAR_XOFF, CHAR_XON,
};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::transport::{ControlRequest, SharedTransport};

const REQTYPE_HOST_TO_DEVICE: u8 = 0x41;
const REQTYPE_DEVICE_TO_HOST: u8 = 0xC1;

const SILABSER_IFC_ENABLE_REQUEST_CODE: u8 = 0x00;
const SILABSER_SET_LINE_CTL_REQUEST_CODE: u8 = 0x03;
const SILABSER_SET_BREAK_REQUEST_CODE: u8 = 0x05;
const SILABSER_SET_MHS_REQUEST_CODE: u8 = 0x07;
const SILABSER_GET_MDMSTS_REQUEST_CODE: u8 = 0x08;
const SILABSER_SET_XON_REQUEST_CODE: u8 = 0x09;
const SILABSER_SET_XOFF_REQUEST_CODE: u8 = 0x0A;
const SILABSER_FLUSH_REQUEST_CODE: u8 = 0x12;
const SILABSER_SET_FLOW_REQUEST_CODE: u8 = 0x13;
const SILABSER_SET_CHARS_REQUEST_CODE: u8 = 0x19;
const SILABSER_SET_BAUDRATE_REQUEST_CODE: u8 = 0x1E;

const UART_ENABLE: u16 = 0x0001;
const UART_DISABLE: u16 = 0x0000;

const DTR_ENABLE: u16 = 0x0101;
const DTR_DISABLE: u16 = 0x0100;
const RTS_ENABLE: u16 = 0x0202;
const RTS_DISABLE: u16 = 0x0200;

const STATUS_CTS: u8 = 0x10;
const STATUS_DSR: u8 = 0x20;
const STATUS_RI: u8 = 0x40;
const STATUS_CD: u8 = 0x80;

pub struct Cp21xxDriver {
    port_index: usize,
    iface: u8,
    dtr: bool,
    rts: bool,
    is_restricted_port: bool,
    endpoints: Option<EndpointPair>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
}

impl Cp21xxDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            iface: port_index as u8,
            dtr: false,
            rts: false,
            is_restricted_port: false,
            endpoints: None,
            transport: None,
            reader: None,
        }
    }

    fn cfg_out(&self, request: u8, value: u16) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest {
                request_type: REQTYPE_HOST_TO_DEVICE,
                request,
                value,
                index: self.port_index as u16,
                data: vec![],
                timeout_ms: WRITE_TIMEOUT_MS,
            })?;
        Ok(())
    }

    fn cfg_out_data(&self, request: u8, value: u16, data: Vec<u8>) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest {
                request_type: REQTYPE_HOST_TO_DEVICE,
                request,
                value,
                index: self.port_index as u16,
                data,
                timeout_ms: WRITE_TIMEOUT_MS,
            })?;
        Ok(())
    }

    fn cfg_in(&self, request: u8, value: u16, length: usize) -> Result<Vec<u8>> {
        self.transport
            .as_ref()
            .unwrap()
            .control_in(&ControlRequest {
                request_type: REQTYPE_DEVICE_TO_HOST,
                request,
                value,
                index: self.port_index as u16,
                data: vec![0; length],
                timeout_ms: WRITE_TIMEOUT_MS,
            })
    }

    fn set_xon(&self, value: bool) -> Result<()> {
        self.cfg_out(
            if value {
                SILABSER_SET_XON_REQUEST_CODE
            } else {
                SILABSER_SET_XOFF_REQUEST_CODE
            },
            0,
        )
    }
}

impl Driver for Cp21xxDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        let iface_count = transport.interfaces().len();
        if self.port_index >= iface_count {
            return Err(UsbSerialError::Io(format!(
                "unknown port number {}",
                self.port_index
            )));
        }
        self.is_restricted_port = iface_count == 2 && self.port_index == 1;
        self.iface = self.port_index as u8;
        transport.claim_interface(self.iface)?;
        self.endpoints = Some(EndpointPair::open(transport, self.iface)?);
        self.cfg_out(SILABSER_IFC_ENABLE_REQUEST_CODE, UART_ENABLE)?;
        self.cfg_out(
            SILABSER_SET_MHS_REQUEST_CODE,
            (if self.dtr { DTR_ENABLE } else { DTR_DISABLE })
                | (if self.rts { RTS_ENABLE } else { RTS_DISABLE }),
        )?;
        self.set_flow_control(FlowControl::None)?;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
        let _ = self.cfg_out(SILABSER_IFC_ENABLE_REQUEST_CODE, UART_DISABLE);
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
        if cfg.baud_rate == 0 {
            return Err(UsbSerialError::Unsupported("invalid baud rate".into()));
        }
        self.cfg_out_data(
            SILABSER_SET_BAUDRATE_REQUEST_CODE,
            0,
            vec![
                (cfg.baud_rate & 0xff) as u8,
                ((cfg.baud_rate >> 8) & 0xff) as u8,
                ((cfg.baud_rate >> 16) & 0xff) as u8,
                ((cfg.baud_rate >> 24) & 0xff) as u8,
            ],
        )?;

        let mut bits = 0u16;
        match cfg.data_bits {
            DataBits::Five => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("data bits 5".into()));
                }
                bits |= 0x0500;
            }
            DataBits::Six => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("data bits 6".into()));
                }
                bits |= 0x0600;
            }
            DataBits::Seven => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("data bits 7".into()));
                }
                bits |= 0x0700;
            }
            DataBits::Eight => bits |= 0x0800,
        };
        match cfg.parity {
            Parity::None => {}
            Parity::Odd => bits |= 0x0010,
            Parity::Even => bits |= 0x0020,
            Parity::Mark => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("parity mark".into()));
                }
                bits |= 0x0030;
            }
            Parity::Space => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("parity space".into()));
                }
                bits |= 0x0040;
            }
        }
        match cfg.stop_bits {
            StopBits::One => {}
            StopBits::OnePointFive => {
                return Err(UsbSerialError::Unsupported("stop bits 1.5".into()));
            }
            StopBits::Two => {
                if self.is_restricted_port {
                    return Err(UsbSerialError::Unsupported("stop bits 2".into()));
                }
                bits |= 2;
            }
        }
        self.cfg_out(SILABSER_SET_LINE_CTL_REQUEST_CODE, bits)
    }

    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        if flow == FlowControl::XonXoffInline {
            return Err(UsbSerialError::Unsupported("xon/xoff inline".into()));
        }

        let mut data = vec![0u8; 16];
        if flow == FlowControl::RtsCts {
            data[4] |= 0b1000_0000;
            data[0] |= 0b0000_1000;
        } else if self.rts {
            data[4] |= 0b0100_0000;
        }
        if flow == FlowControl::DtrDsr {
            data[0] |= 0b0000_0010;
            data[0] |= 0b0001_0000;
        } else if self.dtr {
            data[0] |= 0b0000_0001;
        }
        if flow == FlowControl::XonXoff {
            self.cfg_out_data(
                SILABSER_SET_CHARS_REQUEST_CODE,
                0,
                vec![0, 0, 0, 0, CHAR_XON, CHAR_XOFF],
            )?;
            data[4] |= 0b0000_0011;
            data[7] |= 0b1000_0000;
            data[8] = 128;
            data[12] = 128;
        }
        self.cfg_out_data(SILABSER_SET_FLOW_REQUEST_CODE, 0, data)?;
        if flow == FlowControl::XonXoff {
            self.set_xon(true)?;
        }
        Ok(())
    }

    fn set_dtr(&mut self, value: bool) -> Result<()> {
        self.dtr = value;
        self.cfg_out(
            SILABSER_SET_MHS_REQUEST_CODE,
            if value { DTR_ENABLE } else { DTR_DISABLE },
        )
    }

    fn set_rts(&mut self, value: bool) -> Result<()> {
        self.rts = value;
        self.cfg_out(
            SILABSER_SET_MHS_REQUEST_CODE,
            if value { RTS_ENABLE } else { RTS_DISABLE },
        )
    }

    fn set_break(&mut self, enabled: bool) -> Result<()> {
        self.cfg_out(SILABSER_SET_BREAK_REQUEST_CODE, if enabled { 1 } else { 0 })
    }

    fn purge(&mut self, kind: PurgeKind) -> Result<()> {
        let v = match kind {
            PurgeKind::Rx => 0x0005,
            PurgeKind::Tx => 0x000a,
            PurgeKind::Both => 0x000f,
        };
        self.cfg_out(SILABSER_FLUSH_REQUEST_CODE, v)
    }

    fn modem_status(&mut self) -> Result<ModemStatus> {
        let buf = self.cfg_in(SILABSER_GET_MDMSTS_REQUEST_CODE, 0, 1)?;
        let status = buf[0];
        Ok(ModemStatus {
            cts: (status & STATUS_CTS) != 0,
            dsr: (status & STATUS_DSR) != 0,
            ri: (status & STATUS_RI) != 0,
            cd: (status & STATUS_CD) != 0,
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
