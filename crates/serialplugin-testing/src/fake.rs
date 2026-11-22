//! An in-memory serial device.

use crate::{eventually, Modem};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

type Responder = Box<dyn FnMut(&[u8], usize) -> Vec<u8> + Send>;

/// Modem status lines a [`FakeDevice`] reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lines {
    /// Clear to send.
    pub cts: bool,
    /// Data set ready.
    pub dsr: bool,
    /// Ring indicator.
    pub ring: bool,
    /// Carrier detect.
    pub carrier: bool,
}

impl Default for Lines {
    /// A ready device: CTS and DSR on.
    fn default() -> Self {
        Self {
            cts: true,
            dsr: true,
            ring: false,
            carrier: false,
        }
    }
}

/// A setting or control call the host made on the port, in order ([`FakeDevice::calls`]).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Call {
    /// `set_baud_rate`
    BaudRate(u32),
    /// `set_data_bits`
    DataBits(DataBits),
    /// `set_flow_control`
    FlowControl(FlowControl),
    /// `set_parity`
    Parity(Parity),
    /// `set_stop_bits`
    StopBits(StopBits),
    /// `write_request_to_send`
    Rts(bool),
    /// `write_data_terminal_ready`
    Dtr(bool),
    /// `set_break` / `clear_break`
    Break(bool),
    /// `clear` of the input (also as part of `All`)
    ClearInput,
    /// `clear` of the output (also as part of `All`)
    ClearOutput,
}

#[derive(Clone)]
struct Settings {
    baud_rate: u32,
    data_bits: DataBits,
    flow_control: FlowControl,
    parity: Parity,
    stop_bits: StopBits,
}

struct State {
    /// Bytes the host has not read yet.
    rx: VecDeque<u8>,
    /// Everything the host wrote.
    written: Vec<u8>,
    write_timeouts: Vec<Duration>,
    calls: Vec<Call>,
    settings: Settings,
    lines: Lines,
    read_error: Option<io::ErrorKind>,
    /// Reads left that fail with the kind.
    read_errors_left: (usize, io::ErrorKind),
    write_error: Option<io::ErrorKind>,
    /// Reads left that return `Ok(0)`.
    eof_left: usize,
    eof_reads: usize,
    stall_writes: bool,
    hold_writes: bool,
    hold_reads: bool,
    held: usize,
    driver_pending: Option<u32>,
    out_pending: u32,
    ports: usize,
    responder: Option<Responder>,
}

#[derive(Clone)]
struct Shared(Arc<(Mutex<State>, Condvar)>);

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.0 .0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn change<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        let out = f(&mut self.lock());
        self.0 .1.notify_all();
        out
    }

    /// Block while `gate` holds, counted as a held call.
    fn pass(&self, gate: impl Fn(&State) -> bool) -> MutexGuard<'_, State> {
        let mut state = self.lock();
        if gate(&state) {
            state.held += 1;
            while gate(&state) {
                state = self.0 .1.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            state.held -= 1;
        }
        state
    }
}

/// The device side of an in-memory serial port; clones are handles to the same device.
///
/// [`port`](Self::port) gives the host its side, a [`serialport::SerialPort`]. The device
/// records what the host does and decides what it sees: bytes, errors, held calls.
///
/// ```
/// use serialplugin_testing::FakeDevice;
/// use std::io::{Read, Write};
///
/// let device = FakeDevice::new();
/// let mut port = device.port();
/// port.write_all(b"ping").unwrap();
/// assert_eq!(device.written(), b"ping");
///
/// device.send(b"pong");
/// let mut buf = [0; 4];
/// port.read_exact(&mut buf).unwrap();
/// assert_eq!(&buf, b"pong");
/// ```
#[derive(Clone)]
pub struct FakeDevice(Shared);

impl Default for FakeDevice {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FakeDevice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakeDevice").finish_non_exhaustive()
    }
}

impl FakeDevice {
    /// A silent device at 115200 8N1.
    pub fn new() -> Self {
        let state = State {
            rx: VecDeque::new(),
            written: Vec::new(),
            write_timeouts: Vec::new(),
            calls: Vec::new(),
            settings: Settings {
                baud_rate: 115_200,
                data_bits: DataBits::Eight,
                flow_control: FlowControl::None,
                parity: Parity::None,
                stop_bits: StopBits::One,
            },
            lines: Lines::default(),
            read_error: None,
            read_errors_left: (0, io::ErrorKind::Other),
            write_error: None,
            eof_left: 0,
            eof_reads: 0,
            stall_writes: false,
            hold_writes: false,
            hold_reads: false,
            held: 0,
            driver_pending: None,
            out_pending: 0,
            ports: 0,
            responder: None,
        };
        Self(Shared(Arc::new((Mutex::new(state), Condvar::new()))))
    }

    /// A device where `modem` answers what the host writes.
    pub fn with_modem(modem: Modem) -> Self {
        let device = Self::new();
        device.modem(modem);
        device
    }

    /// The host's port (timeout 1 s). `try_clone` works, as on a real desktop port.
    pub fn port(&self) -> FakePort {
        self.open(true)
    }

    /// Like [`port`](Self::port), but `try_clone` fails: the host has one handle for reads
    /// and writes.
    pub fn unclonable_port(&self) -> FakePort {
        self.open(false)
    }

    fn open(&self, clonable: bool) -> FakePort {
        self.0.change(|s| s.ports += 1);
        FakePort {
            shared: self.0.clone(),
            timeout: Duration::from_secs(1),
            clonable,
        }
    }

    /// Send bytes to the host.
    pub fn send(&self, bytes: &[u8]) {
        self.0.change(|s| s.rx.extend(bytes));
    }

    /// Bytes sent that the host has not read yet.
    pub fn unread(&self) -> usize {
        self.0.lock().rx.len()
    }

    /// Wait until the host read everything sent.
    pub fn wait_read(&self) {
        eventually("the host read every byte", || self.unread() == 0);
    }

    /// Everything the host wrote so far.
    pub fn written(&self) -> Vec<u8> {
        self.0.lock().written.clone()
    }

    /// Wait until what the host wrote satisfies `done`; returns it.
    pub fn wait_written(&self, what: &str, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
        eventually(what, || done(&self.0.lock().written));
        self.written()
    }

    /// The port timeout at each write.
    pub fn write_timeouts(&self) -> Vec<Duration> {
        self.0.lock().write_timeouts.clone()
    }

    /// Setting and control calls so far.
    pub fn calls(&self) -> Vec<Call> {
        self.0.lock().calls.clone()
    }

    /// Answer every write: `reply` gets the written bytes and returns what the device sends.
    pub fn respond(&self, mut reply: impl FnMut(&[u8]) -> Vec<u8> + Send + 'static) {
        self.0
            .change(|s| s.responder = Some(Box::new(move |bytes, _| reply(bytes))));
    }

    /// Let `modem` answer what the host writes.
    pub fn modem(&self, mut modem: Modem) {
        self.0.change(|s| {
            s.responder = Some(Box::new(move |bytes, unread| {
                modem.feed_with_unread(bytes, unread)
            }));
        });
    }

    /// Every read fails with `kind` from now on (an unplugged device).
    pub fn fail_reads(&self, kind: io::ErrorKind) {
        self.0.change(|s| s.read_error = Some(kind));
    }

    /// The next `n` reads fail with `kind`, then the device works again.
    pub fn fail_reads_times(&self, n: usize, kind: io::ErrorKind) {
        self.0.change(|s| s.read_errors_left = (n, kind));
    }

    /// Every write fails with `kind` from now on.
    pub fn fail_writes(&self, kind: io::ErrorKind) {
        self.0.change(|s| s.write_error = Some(kind));
    }

    /// Writes time out: the device takes no bytes.
    pub fn stall_writes(&self, on: bool) {
        self.0.change(|s| s.stall_writes = on);
    }

    /// Every read returns `Ok(0)` from now on (a tty after hang-up).
    pub fn eof(&self) {
        self.0.change(|s| s.eof_left = usize::MAX);
    }

    /// The next `n` reads return `Ok(0)`.
    pub fn eof_times(&self, n: usize) {
        self.0.change(|s| s.eof_left = n);
    }

    /// Reads that returned `Ok(0)` so far.
    pub fn eof_reads(&self) -> usize {
        self.0.lock().eof_reads
    }

    /// While on, writes block in the device.
    pub fn hold_writes(&self, on: bool) {
        self.0.change(|s| s.hold_writes = on);
    }

    /// While on, reads block in the device.
    pub fn hold_reads(&self, on: bool) {
        self.0.change(|s| s.hold_reads = on);
    }

    /// Calls blocked in the device right now.
    pub fn held(&self) -> usize {
        self.0.lock().held
    }

    /// Wait until `n` calls are blocked in the device.
    pub fn wait_held(&self, n: usize) {
        eventually("the host is blocked in the device", || self.held() == n);
    }

    /// Set the status lines.
    pub fn set_lines(&self, lines: Lines) {
        self.0.change(|s| s.lines = lines);
    }

    /// What `bytes_to_read` reports; `None` is the number of unread bytes.
    pub fn set_driver_pending(&self, n: Option<u32>) {
        self.0.change(|s| s.driver_pending = n);
    }

    /// What `bytes_to_write` reports.
    pub fn set_out_pending(&self, n: u32) {
        self.0.change(|s| s.out_pending = n);
    }

    /// Open handles of the host, clones included; 0 once it closed the port.
    pub fn open_ports(&self) -> usize {
        self.0.lock().ports
    }

    /// Wait until the host dropped every handle.
    pub fn wait_closed(&self) {
        eventually("the host closed the port", || self.open_ports() == 0);
    }
}

/// The host side of a [`FakeDevice`].
pub struct FakePort {
    shared: Shared,
    timeout: Duration,
    clonable: bool,
}

impl std::fmt::Debug for FakePort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FakePort")
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl FakePort {
    fn set(&self, call: Call, f: impl FnOnce(&mut Settings)) -> serialport::Result<()> {
        self.shared.change(|s| {
            f(&mut s.settings);
            s.calls.push(call);
        });
        Ok(())
    }

    fn settings(&self) -> Settings {
        self.shared.lock().settings.clone()
    }
}

impl Drop for FakePort {
    fn drop(&mut self) {
        self.shared.change(|s| s.ports -= 1);
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let mut state = self.shared.pass(|s| s.hold_reads);
        let deadline = Instant::now() + self.timeout;
        loop {
            if let Some(kind) = state.read_error {
                return Err(io::Error::new(kind, "fake device: read error"));
            }
            if let (left @ 1.., kind) = state.read_errors_left {
                state.read_errors_left.0 = left - 1;
                return Err(io::Error::new(kind, "fake device: read error"));
            }
            if state.eof_left > 0 {
                if state.eof_left != usize::MAX {
                    state.eof_left -= 1;
                }
                state.eof_reads += 1;
                return Ok(0);
            }
            if !state.rx.is_empty() {
                let n = buf.len().min(state.rx.len());
                for (slot, byte) in buf.iter_mut().zip(state.rx.drain(..n)) {
                    *slot = byte;
                }
                return Ok(n);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "fake device: no data",
                ));
            }
            state = self
                .shared
                .0
                 .1
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.shared.pass(|s| s.hold_writes);
        if let Some(kind) = state.write_error {
            return Err(io::Error::new(kind, "fake device: write error"));
        }
        state.write_timeouts.push(self.timeout);
        if state.stall_writes {
            // The write would end at the port timeout, which is recorded: no need to wait.
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fake device: write stalled",
            ));
        }
        state.written.extend(buf);
        // Without the lock: the responder may use its device.
        if let Some(mut responder) = state.responder.take() {
            let unread = state.rx.len();
            drop(state);
            let reply = responder(buf, unread);
            state = self.shared.lock();
            if state.responder.is_none() {
                state.responder = Some(responder);
            }
            state.rx.extend(reply);
        }
        drop(state);
        self.shared.0 .1.notify_all();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl SerialPort for FakePort {
    fn name(&self) -> Option<String> {
        Some("fake".into())
    }
    fn baud_rate(&self) -> serialport::Result<u32> {
        Ok(self.settings().baud_rate)
    }
    fn data_bits(&self) -> serialport::Result<DataBits> {
        Ok(self.settings().data_bits)
    }
    fn flow_control(&self) -> serialport::Result<FlowControl> {
        Ok(self.settings().flow_control)
    }
    fn parity(&self) -> serialport::Result<Parity> {
        Ok(self.settings().parity)
    }
    fn stop_bits(&self) -> serialport::Result<StopBits> {
        Ok(self.settings().stop_bits)
    }
    fn timeout(&self) -> Duration {
        self.timeout
    }
    fn set_baud_rate(&mut self, v: u32) -> serialport::Result<()> {
        self.set(Call::BaudRate(v), |s| s.baud_rate = v)
    }
    fn set_data_bits(&mut self, v: DataBits) -> serialport::Result<()> {
        self.set(Call::DataBits(v), |s| s.data_bits = v)
    }
    fn set_flow_control(&mut self, v: FlowControl) -> serialport::Result<()> {
        self.set(Call::FlowControl(v), |s| s.flow_control = v)
    }
    fn set_parity(&mut self, v: Parity) -> serialport::Result<()> {
        self.set(Call::Parity(v), |s| s.parity = v)
    }
    fn set_stop_bits(&mut self, v: StopBits) -> serialport::Result<()> {
        self.set(Call::StopBits(v), |s| s.stop_bits = v)
    }
    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.timeout = timeout;
        Ok(())
    }
    fn write_request_to_send(&mut self, v: bool) -> serialport::Result<()> {
        self.set(Call::Rts(v), |_| {})
    }
    fn write_data_terminal_ready(&mut self, v: bool) -> serialport::Result<()> {
        self.set(Call::Dtr(v), |_| {})
    }
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(self.shared.lock().lines.cts)
    }
    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(self.shared.lock().lines.dsr)
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(self.shared.lock().lines.ring)
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(self.shared.lock().lines.carrier)
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        let state = self.shared.lock();
        Ok(state.driver_pending.unwrap_or(state.rx.len() as u32))
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(self.shared.lock().out_pending)
    }
    fn clear(&self, which: ClearBuffer) -> serialport::Result<()> {
        self.shared.change(|s| {
            if matches!(which, ClearBuffer::Input | ClearBuffer::All) {
                s.rx.clear();
                s.calls.push(Call::ClearInput);
            }
            if matches!(which, ClearBuffer::Output | ClearBuffer::All) {
                s.calls.push(Call::ClearOutput);
            }
        });
        Ok(())
    }
    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        if !self.clonable {
            return Err(serialport::Error::new(
                serialport::ErrorKind::Unknown,
                "fake device: this port has no clones",
            ));
        }
        self.shared.change(|s| s.ports += 1);
        Ok(Box::new(FakePort {
            shared: self.shared.clone(),
            timeout: self.timeout,
            clonable: true,
        }))
    }
    fn set_break(&self) -> serialport::Result<()> {
        self.set(Call::Break(true), |_| {})
    }
    fn clear_break(&self) -> serialport::Result<()> {
        self.set(Call::Break(false), |_| {})
    }
}
