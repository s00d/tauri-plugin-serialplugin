//! Devices behind a real file descriptor: a TCP serial bridge and (unix) a PTY pair.

use crate::{Modem, WAIT};
use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long a device-side read waits before it looks at its deadline again.
const POLL: Duration = Duration::from_millis(20);

/// The device side of a port: a PTY master or the TCP peer.
pub trait Io: Read + Write + Send {}
impl<X: Read + Write + Send> Io for X {}

/// A TCP serial bridge: the listener and the `tcp://` path to open.
pub fn tcp_listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    (listener, path)
}

/// The connection the host opened to `listener`.
pub fn accept(listener: &TcpListener) -> TcpStream {
    listener.set_nonblocking(true).unwrap();
    let deadline = Instant::now() + WAIT;
    let stream = loop {
        match listener.accept() {
            Ok((stream, _)) => break stream,
            Err(e) if e.kind() == ErrorKind::WouldBlock && Instant::now() < deadline => {
                std::thread::yield_now();
            }
            Err(e) => panic!("the host did not connect: {e}"),
        }
    };
    stream.set_nonblocking(false).unwrap();
    stream.set_read_timeout(Some(POLL)).unwrap();
    stream
}

/// Read from the device side until `done` holds for everything read.
pub fn recv_until(io: &mut dyn Io, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while !done(&got) {
        assert!(
            Instant::now() < deadline,
            "the host never sent it, got {:?}",
            String::from_utf8_lossy(&got)
        );
        read_some(io, &mut got);
    }
    got
}

/// One read; `false` when the host closed the connection.
fn read_some(io: &mut dyn Io, got: &mut Vec<u8>) -> bool {
    let mut buf = [0; 256];
    match io.read(&mut buf) {
        Ok(0) => return false,
        Ok(n) => got.extend(&buf[..n]),
        Err(e)
            if matches!(
                e.kind(),
                ErrorKind::TimedOut | ErrorKind::WouldBlock | ErrorKind::Interrupted
            ) => {}
        // Linux: EIO on a PTY master while no slave is open.
        Err(_) => std::thread::yield_now(),
    }
    true
}

/// Run `modem` on the device side until the host closes the port or `stop` is dropped.
pub fn serve(mut io: Box<dyn Io>, mut modem: Modem) -> Server {
    let (stop, stopped) = std::sync::mpsc::channel::<()>();
    let thread = std::thread::spawn(move || loop {
        if stopped.try_recv() == Err(std::sync::mpsc::TryRecvError::Disconnected) {
            return;
        }
        let mut got = Vec::new();
        if !read_some(io.as_mut(), &mut got) {
            return;
        }
        let reply = modem.feed(&got);
        if io.write_all(&reply).is_err() {
            return;
        }
    });
    Server {
        stop: Some(stop),
        thread: Some(thread),
    }
}

/// A running device thread; dropping it ends the thread.
pub struct Server {
    stop: Option<std::sync::mpsc::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.take();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A PTY pair: the master is the device, the slave is the port, and the slave's path.
#[cfg(unix)]
pub fn pty() -> (serialport::TTYPort, serialport::TTYPort, String) {
    use serialport::SerialPort;
    // `ptsname` is not thread-safe on macOS.
    static PAIR: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let guard = PAIR.lock().unwrap_or_else(|e| e.into_inner());
    let (mut master, slave) = serialport::TTYPort::pair().expect("pty pair");
    drop(guard);
    let path = slave.name().expect("slave name");
    master.set_timeout(POLL).unwrap();
    (master, slave, path)
}
