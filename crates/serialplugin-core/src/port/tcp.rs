//! Serial-over-TCP transport: `tcp://host:port`.
//!
//! Talks raw bytes to a TCP serial bridge (ser2net in raw mode, ESP-Link, Moxa/USR
//! device servers, `socat TCP-LISTEN:… /dev/ttyUSB0`). Works on every platform,
//! including iOS, which has no local serial API. Line settings (baud, parity, …) and
//! modem lines are stored but not sent: a raw bridge has its own port settings.
//!
//! TCP keepalive is on with a short idle time, so a bridge that vanished without a FIN
//! (power cut, Wi-Fi drop) fails the next read within about
//! `KEEPALIVE_IDLE + KEEPALIVE_RETRIES * KEEPALIVE_INTERVAL` and the hub reports
//! `Disconnect`, instead of the port timing out quietly forever.
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

/// Idle time before the first keepalive probe.
const KEEPALIVE_IDLE: Duration = Duration::from_secs(10);
/// Time between unanswered probes (whole seconds: most stacks round down).
#[allow(dead_code)] // not settable on every stack
const KEEPALIVE_INTERVAL: Duration = Duration::from_secs(5);
/// Unanswered probes before the stack drops the connection (not set on Windows,
/// where it stays at the system's 10).
#[allow(dead_code)] // not settable on every stack
const KEEPALIVE_RETRIES: u32 = 3;

// ponytail: keepalive only covers an idle link; a half-open link with unsent data
// waits for the TCP retransmit timeout (TCP_USER_TIMEOUT on Linux) if that matters.
fn enable_keepalive(stream: &TcpStream) -> io::Result<()> {
    let ka = socket2::TcpKeepalive::new().with_time(KEEPALIVE_IDLE);
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios",
        target_os = "windows"
    ))]
    let ka = ka.with_interval(KEEPALIVE_INTERVAL);
    #[cfg(any(
        target_os = "android",
        target_os = "linux",
        target_os = "macos",
        target_os = "ios"
    ))]
    let ka = ka.with_retries(KEEPALIVE_RETRIES);
    socket2::SockRef::from(stream).set_tcp_keepalive(&ka)
}

/// Read errors as the hub expects them: a read timeout is `TimedOut` (benign), a
/// dead connection is a disconnect kind.
fn map_read_error(e: io::Error) -> io::Error {
    match e.kind() {
        // Unix reports a read timeout as WouldBlock; serial callers expect TimedOut.
        io::ErrorKind::WouldBlock => io::Error::new(io::ErrorKind::TimedOut, e),
        // Unix: SO_RCVTIMEO gives EAGAIN, so ETIMEDOUT here is failed keepalive probes.
        // (Windows uses WSAETIMEDOUT for both; it reports a keepalive drop as
        // WSAENETRESET below.)
        #[cfg(unix)]
        io::ErrorKind::TimedOut => io::Error::new(io::ErrorKind::ConnectionAborted, e),
        #[cfg(windows)]
        _ if e.raw_os_error() == Some(10052) => io::Error::new(io::ErrorKind::ConnectionReset, e),
        _ => e,
    }
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
                    // Best effort: a stack that rejects an option still connects.
                    if let Err(e) = enable_keepalive(&stream) {
                        crate::log_warn!("{path}: TCP keepalive not set: {e}");
                    }
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
            Err(e) => Err(map_read_error(e)),
            ok => ok,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    fn connect(listener: &TcpListener) -> TcpSerialPort {
        let path = format!("tcp://{}", listener.local_addr().unwrap());
        TcpSerialPort::connect(
            &path,
            115_200,
            DataBits::Eight,
            FlowControl::None,
            Parity::None,
            StopBits::One,
            Duration::from_millis(50),
        )
        .unwrap()
    }

    #[test]
    fn keepalive_is_on_with_a_short_idle() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = connect(&listener);
        let sock = socket2::SockRef::from(&port.stream);
        assert!(sock.keepalive().unwrap());
        #[cfg(unix)]
        {
            assert_eq!(sock.tcp_keepalive_time().unwrap(), KEEPALIVE_IDLE);
            #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
            assert_eq!(sock.tcp_keepalive_interval().unwrap(), KEEPALIVE_INTERVAL);
        }
        // A clone shares the socket, so it keeps the options.
        let clone = port.try_clone().unwrap();
        drop(clone);
    }

    /// Unix reports a read timeout as WouldBlock; a dead keepalive is ETIMEDOUT.
    #[test]
    fn keepalive_timeout_reads_as_a_disconnect() {
        let timeout = map_read_error(io::Error::from(io::ErrorKind::WouldBlock));
        assert_eq!(timeout.kind(), io::ErrorKind::TimedOut);
        #[cfg(unix)]
        {
            let dead = map_read_error(io::Error::from(io::ErrorKind::TimedOut));
            assert_eq!(dead.kind(), io::ErrorKind::ConnectionAborted);
        }
        let reset = map_read_error(io::Error::from(io::ErrorKind::ConnectionReset));
        assert_eq!(reset.kind(), io::ErrorKind::ConnectionReset);
    }

    /// The bridge going away ends the hub with `Disconnect`, not endless timeouts.
    #[cfg(not(target_os = "android"))]
    #[test]
    fn bridge_gone_ends_in_disconnect_event() {
        use crate::events::{EventSink, SerialEvent};
        use std::sync::{mpsc, Arc, Mutex};

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = connect(&listener);
        let (peer, _) = listener.accept().unwrap();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let sink: EventSink = Arc::new(move |event: SerialEvent| {
            if matches!(event, SerialEvent::Disconnect { .. }) {
                let _ = tx.lock().unwrap().send(());
            }
            true
        });
        let port: Arc<Mutex<Box<dyn SerialPort>>> = Arc::new(Mutex::new(Box::new(port)));
        let hub = crate::hub::PortRxHub::start(port, "tcp://bridge".into());
        hub.attach_watch_with(sink, 10, 64, false, None);
        drop(peer);
        rx.recv_timeout(Duration::from_secs(3))
            .expect("no Disconnect after the bridge closed");
        hub.stop();
    }
}
