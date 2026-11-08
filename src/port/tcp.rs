//! Serial-over-TCP transport: `tcp://host:port`.
//!
//! Talks raw bytes to a TCP serial bridge (ser2net in raw mode, ESP-Link, Moxa/USR
//! device servers, `socat TCP-LISTEN:… /dev/ttyUSB0`). Works on every platform,
//! including iOS, which has no local serial API. Line settings (baud, parity, …) and
//! modem lines are stored but not sent: a raw bridge has its own port settings.
// ponytail: raw TCP only; add RFC 2217 (telnet COM-PORT-OPTION) if remote line
// control is needed.
#[cfg(target_os = "ios")]
use crate::serialport;

use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{self, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

pub const TCP_SCHEME: &str = "tcp://";

/// `tcp://host:port` → `host:port`, or `None` for an ordinary device path.
pub fn tcp_address(path: &str) -> Option<&str> {
    path.strip_prefix(TCP_SCHEME)
}

pub struct TcpSerialPort {
    stream: TcpStream,
    path: String,
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
    timeout: Duration,
}

/// Socket timeouts must be non-zero.
fn socket_timeout(t: Duration) -> Option<Duration> {
    Some(t.max(Duration::from_millis(1)))
}

impl TcpSerialPort {
    pub fn connect(
        path: &str,
        baud_rate: u32,
        data_bits: DataBits,
        flow_control: FlowControl,
        parity: Parity,
        stop_bits: StopBits,
        timeout: Duration,
    ) -> io::Result<Self> {
        let addr = tcp_address(path)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a tcp:// path"))?;
        let mut last_err = io::Error::new(io::ErrorKind::NotFound, format!("{addr}: no address"));
        // Connecting is not bounded by the (short) read timeout: allow a few seconds.
        let connect_timeout = timeout.max(Duration::from_secs(3));
        for sock in addr.to_socket_addrs()? {
            match TcpStream::connect_timeout(&sock, connect_timeout) {
                Ok(stream) => {
                    stream.set_nodelay(true)?;
                    stream.set_read_timeout(socket_timeout(timeout))?;
                    stream.set_write_timeout(socket_timeout(timeout))?;
                    return Ok(Self {
                        stream,
                        path: path.to_string(),
                        baud_rate,
                        data_bits,
                        flow_control,
                        parity,
                        stop_bits,
                        timeout,
                    });
                }
                Err(e) => last_err = e,
            }
        }
        Err(last_err)
    }
}

impl Read for TcpSerialPort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.stream.read(buf) {
            // A closed socket is an unplugged device, not an empty read.
            Ok(0) if !buf.is_empty() => Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "TCP serial bridge closed the connection",
            )),
            // Unix reports a read timeout as WouldBlock; serial callers expect TimedOut.
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                Err(io::Error::new(io::ErrorKind::TimedOut, e))
            }
            other => other,
        }
    }
}

impl Write for TcpSerialPort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl SerialPort for TcpSerialPort {
    fn name(&self) -> Option<String> {
        Some(self.path.clone())
    }

    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.baud_rate)
    }

    fn data_bits(&self) -> serialport::Result<DataBits> {
        Ok(self.data_bits)
    }

    fn flow_control(&self) -> serialport::Result<FlowControl> {
        Ok(self.flow_control)
    }

    fn parity(&self) -> serialport::Result<Parity> {
        Ok(self.parity)
    }

    fn stop_bits(&self) -> serialport::Result<StopBits> {
        Ok(self.stop_bits)
    }

    fn timeout(&self) -> Duration {
        self.timeout
    }

    fn set_baud_rate(&mut self, baud_rate: u32) -> serialport::Result<()> {
        self.baud_rate = baud_rate;
        Ok(())
    }

    fn set_data_bits(&mut self, data_bits: DataBits) -> serialport::Result<()> {
        self.data_bits = data_bits;
        Ok(())
    }

    fn set_flow_control(&mut self, flow_control: FlowControl) -> serialport::Result<()> {
        self.flow_control = flow_control;
        Ok(())
    }

    fn set_parity(&mut self, parity: Parity) -> serialport::Result<()> {
        self.parity = parity;
        Ok(())
    }

    fn set_stop_bits(&mut self, stop_bits: StopBits) -> serialport::Result<()> {
        self.stop_bits = stop_bits;
        Ok(())
    }

    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.stream.set_read_timeout(socket_timeout(timeout))?;
        self.stream.set_write_timeout(socket_timeout(timeout))?;
        self.timeout = timeout;
        Ok(())
    }

    fn write_request_to_send(&mut self, _level: bool) -> serialport::Result<()> {
        Ok(())
    }

    fn write_data_terminal_ready(&mut self, _level: bool) -> serialport::Result<()> {
        Ok(())
    }

    // A connected raw bridge behaves like a cable with the lines asserted.
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }

    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }

    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(false)
    }

    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(true)
    }

    fn bytes_to_read(&self) -> serialport::Result<u32> {
        self.stream.set_nonblocking(true)?;
        let mut buf = [0u8; 4096];
        let pending = match self.stream.peek(&mut buf) {
            Ok(n) => n as u32,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => 0,
            Err(e) => {
                let _ = self.stream.set_nonblocking(false);
                return Err(e.into());
            }
        };
        self.stream.set_nonblocking(false)?;
        Ok(pending)
    }

    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
    }

    fn clear(&self, buffer_to_clear: ClearBuffer) -> serialport::Result<()> {
        if matches!(buffer_to_clear, ClearBuffer::Output) {
            return Ok(());
        }
        // Drop whatever already arrived, without waiting for more.
        self.stream.set_nonblocking(true)?;
        let mut buf = [0u8; 4096];
        let result = loop {
            match (&self.stream).read(&mut buf) {
                Ok(0) => break Ok(()),
                Ok(_) => continue,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break Ok(()),
                Err(e) => break Err(e.into()),
            }
        };
        self.stream.set_nonblocking(false)?;
        result
    }

    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        Ok(Box::new(Self {
            stream: self.stream.try_clone()?,
            path: self.path.clone(),
            baud_rate: self.baud_rate,
            data_bits: self.data_bits,
            flow_control: self.flow_control,
            parity: self.parity,
            stop_bits: self.stop_bits,
            timeout: self.timeout,
        }))
    }

    fn set_break(&self) -> serialport::Result<()> {
        Ok(())
    }

    fn clear_break(&self) -> serialport::Result<()> {
        Ok(())
    }
}
