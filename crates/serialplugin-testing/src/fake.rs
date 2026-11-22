//! An in-memory serial device.

use crate::{Modem, WAIT};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

type Responder = Box<dyn FnMut(&FakeDevice, &[u8], usize) -> Vec<u8> + Send>;

/// Modem status lines a [`FakeDevice`] reports.
///
/// ```
/// use serialplugin_testing::Lines;
///
/// let lines = Lines::default().cts(false).ring(true);
/// assert!(!lines.cts && lines.dsr && lines.ring && !lines.carrier);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
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

impl Lines {
    /// Set clear to send.
    pub fn cts(mut self, on: bool) -> Self {
        self.cts = on;
        self
    }

    /// Set data set ready.
    pub fn dsr(mut self, on: bool) -> Self {
        self.dsr = on;
        self
    }

    /// Set the ring indicator.
    pub fn ring(mut self, on: bool) -> Self {
        self.ring = on;
        self
    }

    /// Set carrier detect.
    pub fn carrier(mut self, on: bool) -> Self {
        self.carrier = on;
        self
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
    /// Read calls in progress.
    reading: usize,
    driver_pending: Option<u32>,
    out_pending: u32,
    /// Handles of the host.
    ports: usize,
    /// Handles of the test.
    handles: usize,
    responder: Option<Responder>,
    /// Writes the responder has not answered yet.
    unanswered: VecDeque<Vec<u8>>,
    /// A writer runs the responder (without the lock): it answers the writes of others too.
    answering: bool,
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

    fn notify(&self) {
        self.0 .1.notify_all();
    }

    /// Block while `gate` holds, counted as a held call.
    fn pass<'a>(
        &'a self,
        mut state: MutexGuard<'a, State>,
        gate: impl Fn(&State) -> bool,
    ) -> MutexGuard<'a, State> {
        if gate(&state) {
            state.held += 1;
            self.notify();
            while gate(&state) {
                state = self.0 .1.wait(state).unwrap_or_else(|e| e.into_inner());
            }
            state.held -= 1;
            self.notify();
        }
        state
    }

    /// Wait until `done` holds; panics with `what` after [`WAIT`].
    fn wait(&self, what: &str, done: impl Fn(&State) -> bool) {
        let deadline = Instant::now() + WAIT;
        let mut state = self.lock();
        while !done(&state) {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                drop(state);
                panic!("never: {what}");
            }
            let woken = self.0 .1.wait_timeout(state, left);
            state = woken.unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Answer the writes not answered yet, in order.
    fn answer(&self) {
        /// The responder while it is out; a panic in it puts it back.
        struct Out<'a> {
            shared: &'a Shared,
            responder: Option<Responder>,
            done: bool,
        }
        impl Drop for Out<'_> {
            fn drop(&mut self) {
                if self.done {
                    return;
                }
                self.shared.change(|s| {
                    if s.responder.is_none() {
                        s.responder = self.responder.take();
                    }
                    s.answering = false;
                });
            }
        }
        {
            let mut state = self.lock();
            if state.answering || state.responder.is_none() {
                return;
            }
            state.answering = true;
        }
        let device = FakeDevice::handle(self.clone());
        let mut out = Out {
            shared: self,
            responder: None,
            done: false,
        };
        while !out.done {
            let mut state = self.lock();
            // The first one, or one an answer has set meanwhile.
            let mut replaced = None;
            if let Some(new) = state.responder.take() {
                replaced = out.responder.replace(new);
            }
            let next = state.unanswered.pop_front();
            let unread = state.rx.len();
            if next.is_none() {
                state.responder = out.responder.take();
                state.answering = false;
                out.done = true;
            }
            // The responder may use its device, also when it is dropped.
            drop(state);
            drop(replaced);
            if let (Some(bytes), Some(responder)) = (next, &mut out.responder) {
                let reply = responder(&device, &bytes, unread);
                self.change(|s| s.rx.extend(reply));
            }
        }
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
///
/// Dropping the last `FakeDevice` ends [`hold_reads`](Self::hold_reads) and
/// [`hold_writes`](Self::hold_writes), so a test that fails while it holds the host does not
/// hang on the way out.
pub struct FakeDevice(Shared);

impl Clone for FakeDevice {
    fn clone(&self) -> Self {
        Self::handle(self.0.clone())
    }
}

impl Drop for FakeDevice {
    fn drop(&mut self) {
        self.0.change(|s| {
            s.handles -= 1;
            if s.handles == 0 {
                s.hold_reads = false;
                s.hold_writes = false;
            }
        });
    }
}

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
            reading: 0,
            driver_pending: None,
            out_pending: 0,
            ports: 0,
            handles: 0,
            responder: None,
            unanswered: VecDeque::new(),
            answering: false,
        };
        Self::handle(Shared(Arc::new((Mutex::new(state), Condvar::new()))))
    }

    fn handle(shared: Shared) -> Self {
        shared.change(|s| s.handles += 1);
        Self(shared)
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

    /// Wait until the host read everything sent. It may still be handling it: see
    /// [`wait_handled`](Self::wait_handled).
    pub fn wait_read(&self) {
        self.0.wait("the host read every byte", |s| s.rx.is_empty());
    }

    /// Wait until the host read everything sent and asks for more: a host that reads and
    /// handles the bytes on one thread has handled them by then.
    pub fn wait_handled(&self) {
        self.0.wait("the host handled every byte", |s| {
            s.rx.is_empty() && s.reading > 0
        });
    }

    /// Everything the host wrote so far.
    pub fn written(&self) -> Vec<u8> {
        self.0.lock().written.clone()
    }

    /// Wait until what the host wrote satisfies `done`; returns it.
    pub fn wait_written(&self, what: &str, done: impl Fn(&[u8]) -> bool) -> Vec<u8> {
        self.0.wait(what, |s| done(&s.written));
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

    /// Answer every write: `reply` gets the device and the written bytes and returns what
    /// the device sends. Writes are answered one at a time, in order; one made while `reply`
    /// runs (by another handle, or by `reply` itself) is answered after it.
    ///
    /// Use the device it is given: a `FakeDevice` captured by `reply` is never dropped.
    pub fn respond(&self, mut reply: impl FnMut(&FakeDevice, &[u8]) -> Vec<u8> + Send + 'static) {
        self.set_responder(Box::new(move |device, bytes, _| reply(device, bytes)));
    }

    /// Let `modem` answer what the host writes.
    pub fn modem(&self, mut modem: Modem) {
        self.set_responder(Box::new(move |_, bytes, unread| {
            modem.feed_with_unread(bytes, unread)
        }));
    }

    fn set_responder(&self, responder: Responder) {
        // The old one is dropped without the lock.
        let _old = self.0.change(|s| s.responder.replace(responder));
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

    /// The device works again: undoes [`fail_reads`](Self::fail_reads),
    /// [`fail_reads_times`](Self::fail_reads_times), [`fail_writes`](Self::fail_writes),
    /// [`stall_writes`](Self::stall_writes), [`eof`](Self::eof) and
    /// [`eof_times`](Self::eof_times).
    pub fn recover(&self) {
        self.0.change(|s| {
            s.read_error = None;
            s.read_errors_left.0 = 0;
            s.write_error = None;
            s.stall_writes = false;
            s.eof_left = 0;
        });
    }

    /// The next `n` reads return `Ok(0)`.
    pub fn eof_times(&self, n: usize) {
        self.0.change(|s| s.eof_left = n);
    }

    /// Reads that returned `Ok(0)` so far.
    pub fn eof_reads(&self) -> usize {
        self.0.lock().eof_reads
    }

    /// While on, writes block in the device. Ends when the last `FakeDevice` is dropped.
    pub fn hold_writes(&self, on: bool) {
        self.0.change(|s| s.hold_writes = on);
    }

    /// While on, reads block in the device, also one that waits for data already (its
    /// timeout starts again when it is let go). Ends when the last `FakeDevice` is dropped.
    pub fn hold_reads(&self, on: bool) {
        self.0.change(|s| s.hold_reads = on);
    }

    /// Calls blocked in the device right now.
    pub fn held(&self) -> usize {
        self.0.lock().held
    }

    /// Wait until `n` calls are blocked in the device.
    pub fn wait_held(&self, n: usize) {
        self.0
            .wait("the host is blocked in the device", |s| s.held == n);
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
        self.0.wait("the host closed the port", |s| s.ports == 0);
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

    fn read_locked<'a>(
        &'a self,
        mut state: MutexGuard<'a, State>,
        buf: &mut [u8],
    ) -> (MutexGuard<'a, State>, io::Result<usize>) {
        let mut deadline = Instant::now() + self.timeout;
        let fails = |kind| Err(io::Error::new(kind, "fake device: read error"));
        loop {
            if state.hold_reads {
                state = self.shared.pass(state, |s| s.hold_reads);
                deadline = Instant::now() + self.timeout;
            }
            if let Some(kind) = state.read_error {
                return (state, fails(kind));
            }
            if let (left @ 1.., kind) = state.read_errors_left {
                state.read_errors_left.0 = left - 1;
                return (state, fails(kind));
            }
            if state.eof_left > 0 {
                if state.eof_left != usize::MAX {
                    state.eof_left -= 1;
                }
                state.eof_reads += 1;
                return (state, Ok(0));
            }
            if !state.rx.is_empty() {
                let n = buf.len().min(state.rx.len());
                for (slot, byte) in buf.iter_mut().zip(state.rx.drain(..n)) {
                    *slot = byte;
                }
                return (state, Ok(n));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                let e = io::Error::new(io::ErrorKind::TimedOut, "fake device: no data");
                return (state, Err(e));
            }
            let woken = self.shared.0 .1.wait_timeout(state, left);
            state = woken.unwrap_or_else(|e| e.into_inner()).0;
        }
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
        let mut state = self.shared.lock();
        state.reading += 1;
        self.shared.notify();
        let (mut state, result) = self.read_locked(state, buf);
        state.reading -= 1;
        drop(state);
        self.shared.notify();
        result
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut state = self.shared.pass(self.shared.lock(), |s| s.hold_writes);
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
        if state.responder.is_some() || state.answering {
            state.unanswered.push_back(buf.to_vec());
        }
        drop(state);
        self.shared.notify();
        self.shared.answer();
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
