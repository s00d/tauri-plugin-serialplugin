//! `serialport` crate compatibility layer — full [`serialport::SerialPort`] on Android USB.

use crate::config::{DataBits, FlowControl, LineConfig, Parity, PurgeKind, StopBits};
use crate::error::UsbSerialError;
use crate::port::SerialPortHandle;
use serialport::{
    ClearBuffer, DataBits as SpDataBits, FlowControl as SpFlowControl, Parity as SpParity,
    SerialPort, StopBits as SpStopBits,
};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const DEFAULT_WRITE_CHUNK: usize = 512;
const READ_POLL_MS: u64 = 1;

/// USB bulk-OUT timeout per chunk: `max(clamp(port_timeout_ms, 1, 600_000), 2000)`.
pub fn chunk_write_timeout_ms(port_timeout: Duration) -> u32 {
    let ms = port_timeout.as_millis() as u64;
    ms.clamp(1, 600_000).max(2000) as u32
}

fn usb_err(e: UsbSerialError) -> io::Error {
    io::Error::other(e.to_string())
}

fn sp_err(e: UsbSerialError) -> serialport::Error {
    serialport::Error::new(serialport::ErrorKind::Unknown, e.to_string())
}

struct SerialPortInner {
    handle: Mutex<SerialPortHandle>,
    name: String,
    timeout: Mutex<Duration>,
    line_config: Mutex<LineConfig>,
    flow_control: Mutex<FlowControl>,
    rx_ring: Mutex<VecDeque<u8>>,
    write_chunk: usize,
    reader_started: Mutex<bool>,
}

impl SerialPortInner {
    fn new(
        handle: SerialPortHandle,
        name: String,
        line_config: LineConfig,
        flow_control: FlowControl,
        write_chunk: usize,
    ) -> Self {
        Self {
            handle: Mutex::new(handle),
            name,
            timeout: Mutex::new(Duration::from_millis(1000)),
            line_config: Mutex::new(line_config),
            flow_control: Mutex::new(flow_control),
            rx_ring: Mutex::new(VecDeque::new()),
            write_chunk: write_chunk.max(64),
            reader_started: Mutex::new(false),
        }
    }

    fn start_reader(&self) -> io::Result<()> {
        let mut started = self.reader_started.lock().unwrap();
        if *started {
            return Ok(());
        }
        self.handle
            .lock()
            .unwrap()
            .start_reader()
            .map_err(usb_err)?;
        *started = true;
        Ok(())
    }

    fn ensure_reader_started(&self) -> io::Result<()> {
        if *self.reader_started.lock().unwrap() {
            return Ok(());
        }
        self.start_reader()
    }

    fn timeout(&self) -> Duration {
        *self.timeout.lock().unwrap()
    }

    fn drain_ring(&self, buf: &mut [u8]) -> usize {
        let mut ring = self.rx_ring.lock().unwrap();
        let n = ring.len().min(buf.len());
        for (dst, src) in buf[..n].iter_mut().zip(ring.drain(..n)) {
            *dst = src;
        }
        n
    }

    fn push_ring(&self, data: &[u8]) {
        if data.is_empty() {
            return;
        }
        self.rx_ring.lock().unwrap().extend(data);
    }

    fn ring_len(&self) -> usize {
        self.rx_ring.lock().unwrap().len()
    }

    fn clear_ring(&self) {
        self.rx_ring.lock().unwrap().clear();
    }

    fn refill_from_reader(&self, scratch: &mut [u8]) -> io::Result<usize> {
        let mut handle = self.handle.lock().unwrap();
        handle.try_read(scratch).map_err(usb_err)
    }

    fn map_purge(kind: ClearBuffer) -> PurgeKind {
        match kind {
            ClearBuffer::Input => PurgeKind::Rx,
            ClearBuffer::Output => PurgeKind::Tx,
            ClearBuffer::All => PurgeKind::Both,
        }
    }
}

/// Shared [`serialport::SerialPort`] adapter over [`SerialPortHandle`] + background reader.
#[derive(Clone)]
pub struct SerialPortAdapter {
    inner: Arc<SerialPortInner>,
}

impl SerialPortAdapter {
    pub fn new(
        handle: SerialPortHandle,
        name: impl Into<String>,
        line_config: LineConfig,
        flow_control: FlowControl,
    ) -> io::Result<Self> {
        Ok(Self {
            inner: Arc::new(SerialPortInner::new(
                handle,
                name.into(),
                line_config,
                flow_control,
                DEFAULT_WRITE_CHUNK,
            )),
        })
    }

    /// Start background bulk-IN reader after line/DTR setup (avoids racing driver init on OTG).
    pub fn start_reader(&self) -> io::Result<()> {
        self.inner.start_reader()
    }

    /// Stop background reader and close the USB driver (idempotent).
    pub fn shutdown(&self) {
        let mut handle = self.inner.handle.lock().unwrap();
        handle.stop_reader();
        handle.close();
        self.inner.clear_ring();
    }

    pub fn with_write_chunk(mut self, chunk: usize) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.write_chunk = chunk.max(64);
        }
        self
    }
}

impl Read for SerialPortAdapter {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        self.inner.ensure_reader_started()?;
        let deadline = Instant::now() + self.inner.timeout();
        let mut scratch = [0u8; 4096];
        loop {
            let n = self.inner.drain_ring(buf);
            if n > 0 {
                return Ok(n);
            }
            match self.inner.refill_from_reader(&mut scratch) {
                Ok(0) => {
                    if Instant::now() >= deadline {
                        return Ok(0);
                    }
                    thread::sleep(Duration::from_millis(READ_POLL_MS));
                }
                Ok(n) => self.inner.push_ring(&scratch[..n]),
                Err(e) => {
                    let kind = e.kind();
                    if kind == io::ErrorKind::TimedOut || kind == io::ErrorKind::WouldBlock {
                        if Instant::now() >= deadline {
                            return Ok(0);
                        }
                        thread::sleep(Duration::from_millis(READ_POLL_MS));
                    } else {
                        return Err(e);
                    }
                }
            }
        }
    }
}

impl Write for SerialPortAdapter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let chunk = self.inner.write_chunk;
        let mut handle = self.inner.handle.lock().unwrap();
        let mut offset = 0usize;
        while offset < buf.len() {
            let end = (offset + chunk).min(buf.len());
            let n = handle.write(&buf[offset..end]).map_err(usb_err)?;
            if n == 0 {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "USB write returned 0",
                ));
            }
            offset += n;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SerialPort for SerialPortAdapter {
    fn name(&self) -> Option<String> {
        Some(self.inner.name.clone())
    }

    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.inner.line_config.lock().unwrap().baud_rate)
    }

    fn data_bits(&self) -> serialport::Result<SpDataBits> {
        Ok(match self.inner.line_config.lock().unwrap().data_bits {
            DataBits::Five => SpDataBits::Five,
            DataBits::Six => SpDataBits::Six,
            DataBits::Seven => SpDataBits::Seven,
            DataBits::Eight => SpDataBits::Eight,
        })
    }

    fn flow_control(&self) -> serialport::Result<SpFlowControl> {
        Ok(match *self.inner.flow_control.lock().unwrap() {
            FlowControl::None => SpFlowControl::None,
            FlowControl::RtsCts | FlowControl::DtrDsr => SpFlowControl::Hardware,
            FlowControl::XonXoff | FlowControl::XonXoffInline => SpFlowControl::Software,
        })
    }

    fn parity(&self) -> serialport::Result<SpParity> {
        Ok(match self.inner.line_config.lock().unwrap().parity {
            Parity::None => SpParity::None,
            Parity::Odd => SpParity::Odd,
            Parity::Even => SpParity::Even,
            Parity::Mark | Parity::Space => SpParity::None,
        })
    }

    fn stop_bits(&self) -> serialport::Result<SpStopBits> {
        Ok(match self.inner.line_config.lock().unwrap().stop_bits {
            StopBits::One | StopBits::OnePointFive => SpStopBits::One,
            StopBits::Two => SpStopBits::Two,
        })
    }

    fn timeout(&self) -> Duration {
        self.inner.timeout()
    }

    fn set_baud_rate(&mut self, baud_rate: u32) -> serialport::Result<()> {
        let mut cfg = self.inner.line_config.lock().unwrap();
        cfg.baud_rate = baud_rate;
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_line_config(*cfg)
            .map_err(sp_err)
    }

    fn set_data_bits(&mut self, data_bits: SpDataBits) -> serialport::Result<()> {
        let mut cfg = self.inner.line_config.lock().unwrap();
        cfg.data_bits = match data_bits {
            SpDataBits::Five => DataBits::Five,
            SpDataBits::Six => DataBits::Six,
            SpDataBits::Seven => DataBits::Seven,
            SpDataBits::Eight => DataBits::Eight,
        };
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_line_config(*cfg)
            .map_err(sp_err)
    }

    fn set_flow_control(&mut self, flow_control: SpFlowControl) -> serialport::Result<()> {
        let flow = match flow_control {
            SpFlowControl::None => FlowControl::None,
            SpFlowControl::Hardware => FlowControl::RtsCts,
            SpFlowControl::Software => FlowControl::XonXoff,
        };
        *self.inner.flow_control.lock().unwrap() = flow;
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_flow_control(flow)
            .map_err(sp_err)
    }

    fn set_parity(&mut self, parity: SpParity) -> serialport::Result<()> {
        let mut cfg = self.inner.line_config.lock().unwrap();
        cfg.parity = match parity {
            SpParity::None => Parity::None,
            SpParity::Odd => Parity::Odd,
            SpParity::Even => Parity::Even,
        };
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_line_config(*cfg)
            .map_err(sp_err)
    }

    fn set_stop_bits(&mut self, stop_bits: SpStopBits) -> serialport::Result<()> {
        let mut cfg = self.inner.line_config.lock().unwrap();
        cfg.stop_bits = match stop_bits {
            SpStopBits::One => StopBits::One,
            SpStopBits::Two => StopBits::Two,
        };
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_line_config(*cfg)
            .map_err(sp_err)
    }

    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        *self.inner.timeout.lock().unwrap() = timeout;
        Ok(())
    }

    fn write_request_to_send(&mut self, level: bool) -> serialport::Result<()> {
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_rts(level)
            .map_err(sp_err)
    }

    fn write_data_terminal_ready(&mut self, level: bool) -> serialport::Result<()> {
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_dtr(level)
            .map_err(sp_err)
    }

    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(self
            .inner
            .handle
            .lock()
            .unwrap()
            .modem_status()
            .map_err(sp_err)?
            .cts)
    }

    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(self
            .inner
            .handle
            .lock()
            .unwrap()
            .modem_status()
            .map_err(sp_err)?
            .dsr)
    }

    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(self
            .inner
            .handle
            .lock()
            .unwrap()
            .modem_status()
            .map_err(sp_err)?
            .ri)
    }

    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(self
            .inner
            .handle
            .lock()
            .unwrap()
            .modem_status()
            .map_err(sp_err)?
            .cd)
    }

    fn bytes_to_read(&self) -> serialport::Result<u32> {
        Ok(self.inner.ring_len() as u32)
    }

    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
    }

    fn clear(&self, buffer_to_clear: ClearBuffer) -> serialport::Result<()> {
        if matches!(buffer_to_clear, ClearBuffer::Input | ClearBuffer::All) {
            self.inner.clear_ring();
        }
        self.inner
            .handle
            .lock()
            .unwrap()
            .clear(SerialPortInner::map_purge(buffer_to_clear))
            .map_err(sp_err)
    }

    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        Ok(Box::new(SerialPortAdapter {
            inner: self.inner.clone(),
        }))
    }

    fn set_break(&self) -> serialport::Result<()> {
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_break(true)
            .map_err(sp_err)
    }

    fn clear_break(&self) -> serialport::Result<()> {
        self.inner
            .handle
            .lock()
            .unwrap()
            .set_break(false)
            .map_err(sp_err)
    }
}

pub fn line_config_from_serialport(
    baud_rate: u32,
    data_bits: SpDataBits,
    parity: SpParity,
    stop_bits: SpStopBits,
) -> LineConfig {
    LineConfig {
        baud_rate,
        data_bits: match data_bits {
            SpDataBits::Five => DataBits::Five,
            SpDataBits::Six => DataBits::Six,
            SpDataBits::Seven => DataBits::Seven,
            SpDataBits::Eight => DataBits::Eight,
        },
        parity: match parity {
            SpParity::None => Parity::None,
            SpParity::Odd => Parity::Odd,
            SpParity::Even => Parity::Even,
        },
        stop_bits: match stop_bits {
            SpStopBits::One => StopBits::One,
            SpStopBits::Two => StopBits::Two,
        },
    }
}

pub fn flow_from_serialport(flow: SpFlowControl) -> FlowControl {
    match flow {
        SpFlowControl::None => FlowControl::None,
        SpFlowControl::Hardware => FlowControl::RtsCts,
        SpFlowControl::Software => FlowControl::XonXoff,
    }
}
