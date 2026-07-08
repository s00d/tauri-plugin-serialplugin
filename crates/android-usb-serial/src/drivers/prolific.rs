//! Prolific PL2303 driver (ported from ProlificSerialDriver.java).

use super::{line_coding_bytes, Driver, ModemStatus, WRITE_TIMEOUT_MS};
use crate::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use crate::error::{Result, UsbSerialError};
use crate::reader::SerialReader;
use crate::rx_filter::XonXoffRxFilter;
use crate::transport::{BulkIn, ControlRequest, EndpointInfo, SharedTransport};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

const WRITE_ENDPOINT: u8 = 0x02;
const READ_ENDPOINT: u8 = 0x83;
const INTERRUPT_ENDPOINT: u8 = 0x81;

const VENDOR_READ_REQUEST: u8 = 0x01;
const VENDOR_WRITE_REQUEST: u8 = 0x01;
const VENDOR_READ_HXN_REQUEST: u8 = 0x81;
const VENDOR_WRITE_HXN_REQUEST: u8 = 0x80;

const RESET_HXN_REQUEST: u8 = 0x07;
const FLUSH_RX_REQUEST: u8 = 0x08;
const FLUSH_TX_REQUEST: u8 = 0x09;
const SET_LINE_REQUEST: u8 = 0x20;
const SET_CONTROL_REQUEST: u8 = 0x22;
const SEND_BREAK_REQUEST: u8 = 0x23;
const GET_CONTROL_HXN_REQUEST: u8 = 0x80;
const GET_CONTROL_REQUEST: u8 = 0x87;
const STATUS_NOTIFICATION: u8 = 0xa1;

const RESET_HXN_RX_PIPE: u16 = 1;
const RESET_HXN_TX_PIPE: u16 = 2;

const CONTROL_DTR: u16 = 0x01;
const CONTROL_RTS: u16 = 0x02;

const GET_CONTROL_FLAG_CD: u8 = 0x02;
const GET_CONTROL_FLAG_DSR: u8 = 0x04;
const GET_CONTROL_FLAG_RI: u8 = 0x01;
const GET_CONTROL_FLAG_CTS: u8 = 0x08;

const GET_CONTROL_HXN_FLAG_CD: u8 = 0x40;
const GET_CONTROL_HXN_FLAG_DSR: u8 = 0x20;
const GET_CONTROL_HXN_FLAG_RI: u8 = 0x80;
const GET_CONTROL_HXN_FLAG_CTS: u8 = 0x08;

const STATUS_FLAG_CD: u8 = 0x01;
const STATUS_FLAG_DSR: u8 = 0x02;
const STATUS_FLAG_RI: u8 = 0x08;
const STATUS_FLAG_CTS: u8 = 0x80;

const STATUS_BUFFER_SIZE: usize = 10;
const STATUS_BYTE_IDX: usize = 8;

const STANDARD_BAUD_RATES: &[u32] = &[
    75, 150, 300, 600, 1200, 1800, 2400, 3600, 4800, 7200, 9600, 14400, 19200, 28800, 38400, 57600,
    115200, 128000, 134400, 161280, 201600, 230400, 268800, 403200, 460800, 614400, 806400, 921600,
    1228800, 2457600, 3000000, 6000000,
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeviceType {
    Type01,
    TypeT,
    TypeHx,
    TypeHxn,
}

struct ProlificEndpoints {
    data: super::EndpointPair,
    interrupt_ep: u8,
    interrupt_mps: u16,
}

pub struct ProlificDriver {
    #[allow(dead_code)]
    port_index: usize,
    iface: u8,
    device_type: DeviceType,
    control_lines: u16,
    flow: FlowControl,
    baud_rate: i32,
    data_bits: i32,
    stop_bits: i32,
    parity: i32,
    endpoints: Option<ProlificEndpoints>,
    transport: Option<SharedTransport>,
    reader: Option<SerialReader>,
    status: Arc<AtomicU8>,
    status_thread: Option<JoinHandle<()>>,
    stop_status_thread: Arc<AtomicBool>,
    status_error: Arc<Mutex<Option<String>>>,
    status_started: bool,
}

impl ProlificDriver {
    pub fn new(port_index: usize) -> Self {
        Self {
            port_index,
            iface: 0,
            device_type: DeviceType::TypeHx,
            control_lines: 0,
            flow: FlowControl::None,
            baud_rate: -1,
            data_bits: -1,
            stop_bits: -1,
            parity: -1,
            endpoints: None,
            transport: None,
            reader: None,
            status: Arc::new(AtomicU8::new(0)),
            status_thread: None,
            stop_status_thread: Arc::new(AtomicBool::new(false)),
            status_error: Arc::new(Mutex::new(None)),
            status_started: false,
        }
    }

    fn vendor_read_request(&self) -> u8 {
        if self.device_type == DeviceType::TypeHxn {
            VENDOR_READ_HXN_REQUEST
        } else {
            VENDOR_READ_REQUEST
        }
    }

    fn vendor_write_request(&self) -> u8 {
        if self.device_type == DeviceType::TypeHxn {
            VENDOR_WRITE_HXN_REQUEST
        } else {
            VENDOR_WRITE_REQUEST
        }
    }

    fn vendor_in(&self, value: u16, index: u16, length: usize) -> Result<Vec<u8>> {
        self.transport
            .as_ref()
            .unwrap()
            .control_in(&ControlRequest {
                request_type: 0xC0,
                request: self.vendor_read_request(),
                value,
                index,
                data: vec![0; length],
                timeout_ms: WRITE_TIMEOUT_MS,
            })
    }

    fn vendor_out(&self, value: u16, index: u16, data: Vec<u8>) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest {
                request_type: 0x40,
                request: self.vendor_write_request(),
                value,
                index,
                data,
                timeout_ms: WRITE_TIMEOUT_MS,
            })?;
        Ok(())
    }

    fn ctrl_out(&self, request: u8, value: u16, index: u16, data: Vec<u8>) -> Result<()> {
        self.transport
            .as_ref()
            .unwrap()
            .control_out(&ControlRequest::class_out(request, value, index, data))?;
        Ok(())
    }

    fn detect_device_type(&mut self, transport: &SharedTransport) -> Result<()> {
        let raw = transport.raw_descriptors();
        let desc = if raw.len() >= 14 {
            raw
        } else {
            transport.raw_device_descriptor().to_vec()
        };
        if desc.len() < 14 {
            return Err(UsbSerialError::Io(
                "could not get device descriptors".into(),
            ));
        }
        let usb_version = u16::from_le_bytes([desc[2], desc[3]]);
        let device_version = u16::from_le_bytes([desc[12], desc[13]]);
        let max_packet_size0 = desc[7];
        let device_class = transport.device_class();

        self.device_type = if device_class == 0x02 || max_packet_size0 != 64 {
            DeviceType::Type01
        } else if usb_version == 0x200 {
            if (device_version == 0x300 || device_version == 0x500) && self.test_hx_status() {
                DeviceType::TypeT
            } else {
                DeviceType::TypeHxn
            }
        } else {
            DeviceType::TypeHx
        };
        Ok(())
    }

    fn test_hx_status(&self) -> bool {
        self.transport
            .as_ref()
            .unwrap()
            .control_in(&ControlRequest {
                request_type: 0xC0,
                request: VENDOR_READ_REQUEST,
                value: 0x8080,
                index: 0,
                data: vec![0; 1],
                timeout_ms: WRITE_TIMEOUT_MS,
            })
            .is_ok()
    }

    fn reset_device(&self) -> Result<()> {
        self.purge_hw_buffers(true, true)
    }

    fn purge_hw_buffers(&self, purge_write: bool, purge_read: bool) -> Result<()> {
        if self.device_type == DeviceType::TypeHxn {
            let mut index = 0u16;
            if purge_write {
                index |= RESET_HXN_RX_PIPE;
            }
            if purge_read {
                index |= RESET_HXN_TX_PIPE;
            }
            if index != 0 {
                self.vendor_out(RESET_HXN_REQUEST as u16, index, vec![])?;
            }
        } else {
            if purge_write {
                self.vendor_out(FLUSH_RX_REQUEST as u16, 0, vec![])?;
            }
            if purge_read {
                self.vendor_out(FLUSH_TX_REQUEST as u16, 0, vec![])?;
            }
        }
        Ok(())
    }

    fn do_black_magic(&self) -> Result<()> {
        if self.device_type == DeviceType::TypeHxn {
            return Ok(());
        }
        self.vendor_in(0x8484, 0, 1)?;
        self.vendor_out(0x0404, 0, vec![])?;
        self.vendor_in(0x8484, 0, 1)?;
        self.vendor_in(0x8383, 0, 1)?;
        self.vendor_in(0x8484, 0, 1)?;
        self.vendor_out(0x0404, 1, vec![])?;
        self.vendor_in(0x8484, 0, 1)?;
        self.vendor_in(0x8383, 0, 1)?;
        self.vendor_out(0, 1, vec![])?;
        self.vendor_out(1, 0, vec![])?;
        let magic = if self.device_type == DeviceType::Type01 {
            0x24u16
        } else {
            0x44
        };
        self.vendor_out(2, magic, vec![])
    }

    fn set_control_lines(&mut self, value: u16) -> Result<()> {
        self.ctrl_out(SET_CONTROL_REQUEST, value, 0, vec![])?;
        self.control_lines = value;
        Ok(())
    }

    fn open_endpoints(transport: &SharedTransport, iface: u8) -> Result<ProlificEndpoints> {
        let eps = transport.endpoints(iface);
        let find = |addr: u8| -> Result<EndpointInfo> {
            eps.iter()
                .find(|e| e.address == addr)
                .copied()
                .ok_or_else(|| {
                    UsbSerialError::ProbeFailed(format!("missing endpoint 0x{addr:02x}"))
                })
        };
        let in_ep = find(READ_ENDPOINT)?;
        let out_ep = find(WRITE_ENDPOINT)?;
        let int_ep = find(INTERRUPT_ENDPOINT)?;
        Ok(ProlificEndpoints {
            data: super::EndpointPair::from_addresses(
                in_ep.address,
                out_ep.address,
                in_ep.max_packet_size,
            ),
            interrupt_ep: int_ep.address,
            interrupt_mps: int_ep.max_packet_size,
        })
    }

    fn ensure_status_thread(&mut self) -> Result<()> {
        if self.status_started {
            return Ok(());
        }
        self.status.store(0, Ordering::Relaxed);

        let initial = if self.device_type == DeviceType::TypeHxn {
            let data = self.vendor_in(GET_CONTROL_HXN_REQUEST as u16, 0, 1)?;
            decode_vendor_status_hxn(data[0])
        } else {
            let data = self.vendor_in(GET_CONTROL_REQUEST as u16, 0, 1)?;
            decode_vendor_status(data[0])
        };
        self.status.store(initial, Ordering::Relaxed);

        let transport = self.transport.as_ref().unwrap().clone();
        let interrupt_ep = self.endpoints.as_ref().unwrap().interrupt_ep;
        let interrupt_mps = self.endpoints.as_ref().unwrap().interrupt_mps;
        let stop = self.stop_status_thread.clone();
        let status = self.status.clone();
        let status_error = self.status_error.clone();

        stop.store(false, Ordering::Relaxed);
        let handle = thread::spawn(move || {
            let mut interrupt = match transport.open_interrupt_in(interrupt_ep, interrupt_mps) {
                Ok(ep) => ep,
                Err(e) => {
                    *status_error.lock().unwrap() = Some(e.to_string());
                    return;
                }
            };
            let mut buffer = vec![0u8; STATUS_BUFFER_SIZE];
            while !stop.load(Ordering::Relaxed) {
                match interrupt.read(&mut buffer, 500) {
                    Ok(crate::error::ReadOutcome::Data(data)) => {
                        if data.len() != STATUS_BUFFER_SIZE {
                            *status_error.lock().unwrap() = Some(format!(
                                "invalid status notification, expected {STATUS_BUFFER_SIZE} bytes, got {}",
                                data.len()
                            ));
                            break;
                        }
                        if data[0] != STATUS_NOTIFICATION {
                            *status_error.lock().unwrap() = Some(format!(
                                "invalid status notification, expected 0x{STATUS_NOTIFICATION:02x}, got 0x{:02x}",
                                data[0]
                            ));
                            break;
                        }
                        status.store(data[STATUS_BYTE_IDX], Ordering::Relaxed);
                    }
                    Ok(crate::error::ReadOutcome::TimedOut)
                    | Ok(crate::error::ReadOutcome::Cancelled) => {}
                    Err(e) => {
                        if !stop.load(Ordering::Relaxed) {
                            *status_error.lock().unwrap() = Some(e.to_string());
                        }
                        break;
                    }
                }
            }
        });
        self.status_thread = Some(handle);
        self.status_started = true;
        Ok(())
    }

    fn get_status(&mut self) -> Result<u8> {
        self.ensure_status_thread()?;
        if let Some(msg) = self.status_error.lock().unwrap().take() {
            return Err(UsbSerialError::Io(msg));
        }
        Ok(self.status.load(Ordering::Relaxed))
    }

    fn stop_status_thread(&mut self) {
        self.stop_status_thread.store(true, Ordering::Relaxed);
        if let Some(handle) = self.status_thread.take() {
            let _ = handle.join();
        }
        self.stop_status_thread.store(false, Ordering::Relaxed);
        self.status_started = false;
        *self.status_error.lock().unwrap() = None;
    }

    fn filter_baud_rate(&self, baud_rate: u32) -> Result<u32> {
        if baud_rate == 0 {
            return Err(UsbSerialError::Unsupported(format!(
                "invalid baud rate: {baud_rate}"
            )));
        }
        if self.device_type == DeviceType::TypeHxn {
            return Ok(baud_rate);
        }
        if STANDARD_BAUD_RATES.contains(&baud_rate) {
            return Ok(baud_rate);
        }

        let baseline = 12000000u64 * 32;
        let mut mantissa = (baseline / baud_rate as u64) as u32;
        if mantissa == 0 {
            return Err(UsbSerialError::Unsupported("baud rate too high".into()));
        }

        let (buf, effective) = if self.device_type == DeviceType::TypeT {
            let mut exponent = 0u32;
            while mantissa >= 2048 {
                if exponent < 15 {
                    mantissa >>= 1;
                    exponent += 1;
                } else {
                    return Err(UsbSerialError::Unsupported("baud rate too low".into()));
                }
            }
            let buf = mantissa + ((exponent & !1) << 12) + ((exponent & 1) << 16) + (1 << 31);
            let effective = (baseline / mantissa as u64) >> exponent;
            (buf, effective)
        } else {
            let mut exponent = 0u32;
            while mantissa >= 512 {
                if exponent < 7 {
                    mantissa >>= 2;
                    exponent += 1;
                } else {
                    return Err(UsbSerialError::Unsupported("baud rate too low".into()));
                }
            }
            let buf = mantissa + (exponent << 9) + (1 << 31);
            let effective = (baseline / mantissa as u64) >> (exponent * 2);
            (buf, effective)
        };

        let error = (1.0 - (effective as f64 / baud_rate as f64)).abs();
        if error >= 0.031 {
            return Err(UsbSerialError::Unsupported(format!(
                "baud rate deviation {:.1}% is higher than allowed 3%",
                error * 100.0
            )));
        }
        Ok(buf)
    }

    fn line_request_data(&self, cfg: &LineConfig, baud: u32) -> Result<Vec<u8>> {
        let stop = match cfg.stop_bits {
            StopBits::One => 0u8,
            StopBits::OnePointFive => 1,
            StopBits::Two => 2,
        };
        let parity = match cfg.parity {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
            Parity::Mark => 3,
            Parity::Space => 4,
        };
        let data_bits = match cfg.data_bits {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        };
        let mut line = line_coding_bytes(cfg);
        line[0] = (baud & 0xff) as u8;
        line[1] = ((baud >> 8) & 0xff) as u8;
        line[2] = ((baud >> 16) & 0xff) as u8;
        line[3] = ((baud >> 24) & 0xff) as u8;
        line[4] = stop;
        line[5] = parity;
        line[6] = data_bits;
        Ok(line.to_vec())
    }
}

fn decode_vendor_status(byte: u8) -> u8 {
    let mut status = 0u8;
    if byte & GET_CONTROL_FLAG_CTS == 0 {
        status |= STATUS_FLAG_CTS;
    }
    if byte & GET_CONTROL_FLAG_DSR == 0 {
        status |= STATUS_FLAG_DSR;
    }
    if byte & GET_CONTROL_FLAG_CD == 0 {
        status |= STATUS_FLAG_CD;
    }
    if byte & GET_CONTROL_FLAG_RI == 0 {
        status |= STATUS_FLAG_RI;
    }
    status
}

fn decode_vendor_status_hxn(byte: u8) -> u8 {
    let mut status = 0u8;
    if byte & GET_CONTROL_HXN_FLAG_CTS == 0 {
        status |= STATUS_FLAG_CTS;
    }
    if byte & GET_CONTROL_HXN_FLAG_DSR == 0 {
        status |= STATUS_FLAG_DSR;
    }
    if byte & GET_CONTROL_HXN_FLAG_CD == 0 {
        status |= STATUS_FLAG_CD;
    }
    if byte & GET_CONTROL_HXN_FLAG_RI == 0 {
        status |= STATUS_FLAG_RI;
    }
    status
}

fn status_flag_to_modem(status: u8) -> ModemStatus {
    ModemStatus {
        cts: status & STATUS_FLAG_CTS != 0,
        dsr: status & STATUS_FLAG_DSR != 0,
        ri: status & STATUS_FLAG_RI != 0,
        cd: status & STATUS_FLAG_CD != 0,
    }
}

impl Driver for ProlificDriver {
    fn open(&mut self, transport: &SharedTransport) -> Result<()> {
        self.transport = Some(transport.clone());
        self.iface = 0;
        transport.claim_interface(self.iface)?;
        self.endpoints = Some(Self::open_endpoints(transport, self.iface)?);
        self.detect_device_type(transport)?;
        self.reset_device()?;
        self.do_black_magic()?;
        self.set_control_lines(self.control_lines)?;
        self.set_flow_control(self.flow)?;
        Ok(())
    }

    fn close(&mut self) -> Result<()> {
        if let Some(mut r) = self.reader.take() {
            r.stop();
        }
        self.stop_status_thread();
        let _ = self.reset_device();
        if let Some(t) = &self.transport {
            let _ = t.release_interface(self.iface);
        }
        self.endpoints = None;
        Ok(())
    }

    fn write(&mut self, data: &[u8]) -> Result<usize> {
        let t = self.transport.as_ref().unwrap();
        self.endpoints.as_mut().unwrap().data.write(t, data)
    }

    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(reader) = &mut self.reader {
            return reader.try_read(buf);
        }
        Ok(0)
    }

    fn set_line_config(&mut self, cfg: LineConfig) -> Result<()> {
        let baud = self.filter_baud_rate(cfg.baud_rate)?;
        let data_bits = match cfg.data_bits {
            DataBits::Five => 5,
            DataBits::Six => 6,
            DataBits::Seven => 7,
            DataBits::Eight => 8,
        };
        let stop_bits = match cfg.stop_bits {
            StopBits::One => 1,
            StopBits::OnePointFive => 3,
            StopBits::Two => 2,
        };
        let parity = match cfg.parity {
            Parity::None => 0,
            Parity::Odd => 1,
            Parity::Even => 2,
            Parity::Mark => 3,
            Parity::Space => 4,
        };

        if self.baud_rate == baud as i32
            && self.data_bits == data_bits
            && self.stop_bits == stop_bits
            && self.parity == parity
        {
            return Ok(());
        }

        let line = self.line_request_data(&cfg, baud)?;
        self.ctrl_out(SET_LINE_REQUEST, 0, 0, line)?;
        self.reset_device()?;

        self.baud_rate = baud as i32;
        self.data_bits = data_bits;
        self.stop_bits = stop_bits;
        self.parity = parity;
        Ok(())
    }

    fn set_flow_control(&mut self, flow: FlowControl) -> Result<()> {
        match flow {
            FlowControl::None => {
                if self.device_type == DeviceType::TypeHxn {
                    self.vendor_out(0x0a, 0xff, vec![])?;
                } else {
                    self.vendor_out(0, 0, vec![])?;
                }
            }
            FlowControl::RtsCts => {
                if self.device_type == DeviceType::TypeHxn {
                    self.vendor_out(0x0a, 0xfa, vec![])?;
                } else {
                    self.vendor_out(0, 0x61, vec![])?;
                }
            }
            FlowControl::XonXoffInline => {
                if self.device_type == DeviceType::TypeHxn {
                    self.vendor_out(0x0a, 0xee, vec![])?;
                } else {
                    self.vendor_out(0, 0xc1, vec![])?;
                }
            }
            _ => return Err(UsbSerialError::Unsupported("flow control".into())),
        }
        self.flow = flow;
        Ok(())
    }

    fn set_dtr(&mut self, value: bool) -> Result<()> {
        let new_lines = if value {
            self.control_lines | CONTROL_DTR
        } else {
            self.control_lines & !CONTROL_DTR
        };
        self.set_control_lines(new_lines)
    }

    fn set_rts(&mut self, value: bool) -> Result<()> {
        let new_lines = if value {
            self.control_lines | CONTROL_RTS
        } else {
            self.control_lines & !CONTROL_RTS
        };
        self.set_control_lines(new_lines)
    }

    fn set_break(&mut self, enabled: bool) -> Result<()> {
        let value = if enabled { 0xffffu16 } else { 0 };
        self.ctrl_out(SEND_BREAK_REQUEST, value, 0, vec![])
    }

    fn purge(&mut self, kind: PurgeKind) -> Result<()> {
        match kind {
            PurgeKind::Rx => self.purge_hw_buffers(true, false),
            PurgeKind::Tx => self.purge_hw_buffers(false, true),
            PurgeKind::Both => self.purge_hw_buffers(true, true),
        }
    }

    fn modem_status(&mut self) -> Result<ModemStatus> {
        let status = self.get_status()?;
        Ok(status_flag_to_modem(status))
    }

    fn bulk_in_mps(&self) -> u16 {
        self.endpoints.as_ref().map(|e| e.data.mps).unwrap_or(64)
    }

    fn take_bulk_in(&mut self) -> Option<Box<dyn BulkIn>> {
        let transport = self.transport.as_ref()?;
        self.endpoints.as_mut()?.data.take_in(transport)
    }

    fn rx_filters(&self) -> Vec<Box<dyn crate::rx_filter::RxFilter>> {
        if self.flow == FlowControl::XonXoffInline {
            vec![Box::new(XonXoffRxFilter::new(true))]
        } else {
            vec![]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_standard_baud() {
        let mut d = ProlificDriver::new(0);
        d.device_type = DeviceType::TypeHx;
        assert_eq!(d.filter_baud_rate(115200).unwrap(), 115200);
    }

    #[test]
    fn filter_custom_baud_hx() {
        let mut d = ProlificDriver::new(0);
        d.device_type = DeviceType::TypeHx;
        let encoded = d.filter_baud_rate(500_000).unwrap();
        assert_ne!(encoded, 500_000);
        assert!(encoded & (1 << 31) != 0);
    }
}
