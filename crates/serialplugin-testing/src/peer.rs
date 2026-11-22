//! Devices behind a real file descriptor: a TCP serial bridge and (unix) a PTY pair.

use crate::{Modem, WAIT};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a device-side call waits before it looks at its deadline (or stop) again.
const POLL: Duration = Duration::from_millis(20);

/// The device side of a port: a PTY master or the TCP peer.
pub trait Io: Read + Write + Send {
    /// Bound every read by `timeout`: it ends with `TimedOut` or `WouldBlock`.
    fn bound_reads(&mut self, timeout: Duration) -> io::Result<()>;

    /// The same for writes.
    fn bound_writes(&mut self, timeout: Duration) -> io::Result<()>;

    /// The host may open the port again after it closed it. Otherwise its end is final.
    fn reopens(&self) -> bool {
        false
    }
}

impl Io for TcpStream {
    fn bound_reads(&mut self, timeout: Duration) -> io::Result<()> {
        self.set_read_timeout(Some(timeout))
    }

    fn bound_writes(&mut self, timeout: Duration) -> io::Result<()> {
        self.set_write_timeout(Some(timeout))
    }
}

#[cfg(unix)]
impl Io for serialport::TTYPort {
    fn bound_reads(&mut self, timeout: Duration) -> io::Result<()> {
        Ok(serialport::SerialPort::set_timeout(self, timeout)?)
    }

    fn bound_writes(&mut self, timeout: Duration) -> io::Result<()> {
        self.bound_reads(timeout)
    }

    fn reopens(&self) -> bool {
        true
    }
}

/// A TCP serial bridge: the listener and the `tcp://` path to open.
pub fn tcp_listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    (listener, path)
}

/// The connection the host opened to `listener` (a blocking one, as
/// [`tcp_listener`] gives; it is blocking again afterwards).
pub fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + WAIT;
    let accepted = loop {
        match listener.accept() {
            Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            other => break other,
        }
    };
    listener.set_nonblocking(false).unwrap();
    let (mut stream, _) = accepted.unwrap_or_else(|e| panic!("the host did not connect: {e}"));
    stream.set_nonblocking(false).unwrap();
    stream.bound_reads(POLL).unwrap();
    stream
}

/// Read from the device side until `done` holds for everything read; panics after
/// [`WAIT`], and at once when the host closed the connection for good.
pub fn recv_until(io: &mut dyn Io, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    io.bound_reads(POLL).expect("the device side is open");
    while !done(&got) {
        let closed = match read_some(io) {
            Got::Data(bytes) => {
                got.extend(bytes);
                false
            }
            Got::Nothing => false,
            Got::Closed => true,
        };
        let text = || String::from_utf8_lossy(&got).into_owned();
        assert!(
            !closed || io.reopens(),
            "the host closed the connection, got {:?}",
            text()
        );
        assert!(
            Instant::now() < deadline,
            "the host never sent it{}, got {:?}",
            if closed { " (the port is closed)" } else { "" },
            text()
        );
        if closed {
            std::thread::park_timeout(POLL);
        }
    }
    got
}

enum Got {
    Data(Vec<u8>),
    /// Nothing yet.
    Nothing,
    /// No host on the other side.
    Closed,
}

fn read_some(io: &mut dyn Io) -> Got {
    let mut buf = [0; 256];
    match io.read(&mut buf) {
        Ok(0) => Got::Closed,
        Ok(n) => Got::Data(buf[..n].to_vec()),
        Err(e) if is_transient(&e) => Got::Nothing,
        // Linux: EIO on a PTY master while no slave is open. TCP: the connection is gone.
        Err(_) => Got::Closed,
    }
}

fn is_transient(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
    )
}

/// Run `modem` on the device side until the [`Server`] is dropped, or the host closed a
/// connection that does not come back (a TCP bridge; a PTY is served again when it is
/// reopened). It sets the timeouts of `io`.
pub fn serve(mut io: Box<dyn Io>, mut modem: Modem) -> Server {
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let thread = std::thread::spawn(move || {
        if io.bound_reads(POLL).and(io.bound_writes(POLL)).is_err() {
            return;
        }
        let mut reply = Vec::new();
        while !stopped.load(Ordering::SeqCst) {
            let closed = if reply.is_empty() {
                match read_some(io.as_mut()) {
                    Got::Data(bytes) => {
                        reply = modem.feed(&bytes);
                        false
                    }
                    Got::Nothing => false,
                    Got::Closed => true,
                }
            } else {
                // A host that does not read holds the modem up, it does not end it.
                match io.write(&reply) {
                    Ok(0) => true,
                    Ok(n) => {
                        reply.drain(..n);
                        false
                    }
                    Err(e) => !is_transient(&e),
                }
            };
            if closed && !io.reopens() {
                return;
            }
            if closed {
                // What a closed port was sent is lost.
                reply.clear();
                std::thread::park_timeout(POLL);
            }
        }
    });
    Server {
        stop,
        thread: Some(thread),
    }
}

/// A running device thread; dropping it ends the thread.
#[must_use = "the device is served until the Server is dropped"]
#[derive(Debug)]
pub struct Server {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            thread.thread().unpark();
            let _ = thread.join();
        }
    }
}

/// Both ends of a pseudo terminal.
#[cfg(unix)]
#[derive(Debug)]
#[non_exhaustive]
pub struct Pty {
    /// The device side (the master).
    pub device: serialport::TTYPort,
    /// The host side (the slave), open; drop it when the code under test opens `path` itself.
    pub host: serialport::TTYPort,
    /// The path of the host side.
    pub path: String,
}

/// A new PTY pair.
#[cfg(unix)]
pub fn pty() -> Pty {
    use serialport::SerialPort;
    // `ptsname` is not thread-safe on macOS.
    static PAIR: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = PAIR.lock().unwrap_or_else(|e| e.into_inner());
    let (mut device, host) = serialport::TTYPort::pair().expect("pty pair");
    drop(guard);
    let path = host.name().expect("slave name");
    device.set_timeout(POLL).unwrap();
    Pty { device, host, path }
}
