//! Shared fixtures of the integration tests: a fake serial device (`Fake`) handed to
//! `SerialManager::insert_port`, event sinks into channels, and wait helpers.
#![allow(dead_code, unused_imports)]

pub(crate) use serial_codec::framing::FramerConfig;
pub(crate) use serialplugin_core::error::{CoreError, Error, ErrorKind};
pub(crate) use serialplugin_core::events::{
    CoreEvent, CoreSink, EventSink, ExchangeOptions, PortStatus, RxPrepareMode, SerialEvent,
    WatchOptions,
};
pub(crate) use serialplugin_core::state::ClearBuffer;
pub(crate) use serialplugin_core::SerialManager;
pub(crate) use serialport::SerialPort;
pub(crate) use std::collections::VecDeque;
pub(crate) use std::io::{self, Read, Write};
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
pub(crate) use std::sync::mpsc::{self, Receiver};
pub(crate) use std::sync::{Arc, Condvar, Mutex};
pub(crate) use std::thread;
pub(crate) use std::time::{Duration, Instant};

// ---------------------------------------------------------------- fake port

pub(crate) type Responder = Box<dyn FnMut(&[u8]) -> Vec<u8> + Send>;

#[derive(Default)]
pub(crate) struct Shared {
    pub(crate) rx: Mutex<VecDeque<u8>>,
    pub(crate) cv: Condvar,
    pub(crate) tx: Mutex<Vec<u8>>,
    pub(crate) read_error: Mutex<Option<io::ErrorKind>>,
    pub(crate) write_error: Mutex<Option<io::ErrorKind>>,
    /// Writes fail as timed out (after recording the port timeout they saw).
    pub(crate) stall_writes: AtomicBool,
    /// Writes block in the device until [`Fake::release_writes`]; `held` counts them.
    pub(crate) hold_writes: Mutex<bool>,
    pub(crate) held: AtomicUsize,
    /// Reads block in the device until [`Fake::release_reads`]; `held` counts them.
    pub(crate) hold_reads: Mutex<bool>,
    pub(crate) hold_cv: Condvar,
    /// `bytes_to_read` reports this when non-zero (bytes "in the driver").
    pub(crate) driver_pending: AtomicU32,
    pub(crate) live: AtomicUsize,
    pub(crate) cleared_input: AtomicUsize,
    pub(crate) responder: Mutex<Option<Responder>>,
    /// Every read returns `Ok(0)` at once (a tty after hang-up).
    pub(crate) eof: AtomicBool,
    /// The next this many reads return `Ok(0)` at once.
    pub(crate) eof_left: AtomicU32,
    /// When each `Ok(0)` read happened.
    pub(crate) eof_reads: Mutex<Vec<Instant>>,
    /// CTS, DSR, RI, CD when set (else true, true, false, false).
    pub(crate) lines: Mutex<Option<[bool; 4]>>,
    /// `bytes_to_write` reports this.
    pub(crate) out_pending: AtomicU32,
    /// The port timeout at each write.
    pub(crate) write_timeouts: Mutex<Vec<Duration>>,
    /// Setting and modem-line calls, as text.
    pub(crate) calls: Mutex<Vec<String>>,
}

/// Handle to a fake device; [`Fake::port`] makes the `SerialPort` for the manager.
#[derive(Clone, Default)]
pub(crate) struct Fake(pub(crate) Arc<Shared>);

pub(crate) struct FakePort {
    pub(crate) s: Arc<Shared>,
    pub(crate) timeout: Duration,
    pub(crate) clonable: bool,
}

impl Fake {
    pub(crate) fn new() -> Self {
        Self::default()
    }
    pub(crate) fn port(&self, clonable: bool) -> Box<dyn SerialPort> {
        self.0.live.fetch_add(1, Ordering::SeqCst);
        Box::new(FakePort {
            s: self.0.clone(),
            timeout: Duration::from_millis(1000),
            clonable,
        })
    }
    pub(crate) fn push(&self, bytes: &[u8]) {
        self.0.rx.lock().unwrap().extend(bytes);
        self.0.cv.notify_all();
    }
    pub(crate) fn tx(&self) -> Vec<u8> {
        self.0.tx.lock().unwrap().clone()
    }
    /// Bytes pushed that no read took yet.
    pub(crate) fn rx_len(&self) -> usize {
        self.0.rx.lock().unwrap().len()
    }
    pub(crate) fn live(&self) -> usize {
        self.0.live.load(Ordering::SeqCst)
    }
    pub(crate) fn fail_reads(&self, kind: io::ErrorKind) {
        *self.0.read_error.lock().unwrap() = Some(kind);
        self.0.cv.notify_all();
    }
    pub(crate) fn eof(&self) {
        self.0.eof.store(true, Ordering::SeqCst);
        self.0.cv.notify_all();
    }
    pub(crate) fn eof_reads(&self) -> Vec<Instant> {
        self.0.eof_reads.lock().unwrap().clone()
    }
    pub(crate) fn fail_writes(&self, kind: io::ErrorKind) {
        *self.0.write_error.lock().unwrap() = Some(kind);
    }
    /// From now on a write (or read) blocks in the device until released.
    pub(crate) fn hold_writes(&self) {
        *self.0.hold_writes.lock().unwrap() = true;
    }
    pub(crate) fn release_writes(&self) {
        *self.0.hold_writes.lock().unwrap() = false;
        self.0.hold_cv.notify_all();
    }
    pub(crate) fn hold_reads(&self) {
        *self.0.hold_reads.lock().unwrap() = true;
    }
    pub(crate) fn release_reads(&self) {
        *self.0.hold_reads.lock().unwrap() = false;
        self.0.hold_cv.notify_all();
    }
    /// Calls blocked in the device right now.
    pub(crate) fn held(&self) -> usize {
        self.0.held.load(Ordering::SeqCst)
    }
    pub(crate) fn respond(&self, f: impl FnMut(&[u8]) -> Vec<u8> + Send + 'static) {
        *self.0.responder.lock().unwrap() = Some(Box::new(f));
    }
    /// A tiny AT modem: `AT+CSQ` → `+CSQ` (after `urc`, if any), `AT` → `OK`.
    pub(crate) fn modem(&self, urc: &'static str) {
        let mut line = Vec::new();
        self.respond(move |bytes| {
            let mut out = Vec::new();
            for &b in bytes {
                if b != b'\r' {
                    line.push(b);
                    continue;
                }
                let cmd = String::from_utf8_lossy(&line).to_string();
                line.clear();
                let reply = match cmd.as_str() {
                    "AT+CSQ" => format!("{urc}\r\n+CSQ: 20,99\r\n\r\nOK\r\n"),
                    "AT" => "\r\nOK\r\n".into(),
                    _ => "\r\nERROR\r\n".into(),
                };
                out.extend_from_slice(reply.as_bytes());
            }
            out
        });
    }
}

impl Drop for FakePort {
    fn drop(&mut self) {
        self.s.live.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Shared {
    /// Block while `gate` is set (counted in `held`).
    fn wait_gate(&self, gate: &Mutex<bool>) {
        let mut on = gate.lock().unwrap();
        if !*on {
            return;
        }
        self.held.fetch_add(1, Ordering::SeqCst);
        while *on {
            on = self.hold_cv.wait(on).unwrap();
        }
        self.held.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.s.wait_gate(&self.s.hold_reads);
        let deadline = Instant::now() + self.timeout;
        let mut rx = self.s.rx.lock().unwrap();
        loop {
            if let Some(kind) = *self.s.read_error.lock().unwrap() {
                return Err(io::Error::new(kind, "fake read error"));
            }
            let left = self.s.eof_left.load(Ordering::SeqCst);
            if self.s.eof.load(Ordering::SeqCst) || left > 0 {
                if left > 0 {
                    self.s.eof_left.store(left - 1, Ordering::SeqCst);
                }
                self.s.eof_reads.lock().unwrap().push(Instant::now());
                return Ok(0);
            }
            if !rx.is_empty() {
                let n = buf.len().min(rx.len());
                for (slot, b) in buf.iter_mut().zip(rx.drain(..n)) {
                    *slot = b;
                }
                return Ok(n);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(io::Error::new(io::ErrorKind::TimedOut, "fake read timeout"));
            }
            rx = self.s.cv.wait_timeout(rx, left).unwrap().0;
        }
    }
}

impl Write for FakePort {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.s.wait_gate(&self.s.hold_writes);
        if let Some(kind) = *self.s.write_error.lock().unwrap() {
            return Err(io::Error::new(kind, "fake write error"));
        }
        self.s.write_timeouts.lock().unwrap().push(self.timeout);
        if self.s.stall_writes.load(Ordering::SeqCst) {
            // A device that never takes the bytes: the write ends at the port timeout,
            // which is recorded above (no need to wait it out here).
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "fake write stalled",
            ));
        }
        self.s.tx.lock().unwrap().extend_from_slice(buf);
        let reply = self.s.responder.lock().unwrap().as_mut().map(|f| f(buf));
        if let Some(reply) = reply.filter(|r| !r.is_empty()) {
            self.s.rx.lock().unwrap().extend(reply);
            self.s.cv.notify_all();
        }
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
        Ok(115_200)
    }
    fn data_bits(&self) -> serialport::Result<serialport::DataBits> {
        Ok(serialport::DataBits::Eight)
    }
    fn flow_control(&self) -> serialport::Result<serialport::FlowControl> {
        Ok(serialport::FlowControl::None)
    }
    fn parity(&self) -> serialport::Result<serialport::Parity> {
        Ok(serialport::Parity::None)
    }
    fn stop_bits(&self) -> serialport::Result<serialport::StopBits> {
        Ok(serialport::StopBits::One)
    }
    fn timeout(&self) -> Duration {
        self.timeout
    }
    fn set_baud_rate(&mut self, _: u32) -> serialport::Result<()> {
        Ok(())
    }
    fn set_data_bits(&mut self, v: serialport::DataBits) -> serialport::Result<()> {
        self.s.call(format!("data_bits {v:?}"));
        Ok(())
    }
    fn set_flow_control(&mut self, v: serialport::FlowControl) -> serialport::Result<()> {
        self.s.call(format!("flow {v:?}"));
        Ok(())
    }
    fn set_parity(&mut self, v: serialport::Parity) -> serialport::Result<()> {
        self.s.call(format!("parity {v:?}"));
        Ok(())
    }
    fn set_stop_bits(&mut self, _: serialport::StopBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.timeout = timeout;
        Ok(())
    }
    fn write_request_to_send(&mut self, v: bool) -> serialport::Result<()> {
        self.s.call(format!("rts {v}"));
        Ok(())
    }
    fn write_data_terminal_ready(&mut self, v: bool) -> serialport::Result<()> {
        self.s.call(format!("dtr {v}"));
        Ok(())
    }
    fn read_clear_to_send(&mut self) -> serialport::Result<bool> {
        Ok(self.s.line(0, true))
    }
    fn read_data_set_ready(&mut self) -> serialport::Result<bool> {
        Ok(self.s.line(1, true))
    }
    fn read_ring_indicator(&mut self) -> serialport::Result<bool> {
        Ok(self.s.line(2, false))
    }
    fn read_carrier_detect(&mut self) -> serialport::Result<bool> {
        Ok(self.s.line(3, false))
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        match self.s.driver_pending.load(Ordering::SeqCst) {
            0 => Ok(self.s.rx.lock().unwrap().len() as u32),
            n => Ok(n),
        }
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(self.s.out_pending.load(Ordering::SeqCst))
    }
    fn clear(&self, which: serialport::ClearBuffer) -> serialport::Result<()> {
        if !matches!(which, serialport::ClearBuffer::Output) {
            self.s.rx.lock().unwrap().clear();
            self.s.cleared_input.fetch_add(1, Ordering::SeqCst);
        }
        Ok(())
    }
    fn try_clone(&self) -> serialport::Result<Box<dyn SerialPort>> {
        if !self.clonable {
            return Err(serialport::Error::new(
                serialport::ErrorKind::Unknown,
                "no clone",
            ));
        }
        self.s.live.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(FakePort {
            s: self.s.clone(),
            timeout: self.timeout,
            clonable: true,
        }))
    }
    fn set_break(&self) -> serialport::Result<()> {
        self.s.call("break on".into());
        Ok(())
    }
    fn clear_break(&self) -> serialport::Result<()> {
        self.s.call("break off".into());
        Ok(())
    }
}

impl Shared {
    pub(crate) fn call(&self, what: String) {
        self.calls.lock().unwrap().push(what);
    }
    pub(crate) fn line(&self, i: usize, default: bool) -> bool {
        self.lines.lock().unwrap().map_or(default, |l| l[i])
    }
}

// ---------------------------------------------------------------- helpers

pub(crate) fn old_sink() -> (EventSink, Receiver<SerialEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Arc::new(move |e| tx.lock().unwrap().send(e).is_ok()), rx)
}

pub(crate) fn core_sink() -> (CoreSink, Receiver<CoreEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Arc::new(move |e| tx.lock().unwrap().send(e).is_ok()), rx)
}

pub(crate) fn watch_opts(flush_ms: u64, route_urc: bool) -> WatchOptions {
    WatchOptions {
        serial_data_flush_interval_ms: Some(flush_ms),
        route_urc,
        ..Default::default()
    }
}

pub(crate) fn readline() -> Option<FramerConfig> {
    Some(serde_json::from_value(serde_json::json!({"type": "readline"})).unwrap())
}

/// Data bytes of old-API events received within `wait`, until `done` says stop.
pub(crate) fn collect_data(
    rx: &Receiver<SerialEvent>,
    wait: Duration,
    done: impl Fn(&[u8]) -> bool,
) -> Vec<u8> {
    let deadline = Instant::now() + wait;
    let mut out = Vec::new();
    while !done(&out) {
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(SerialEvent::Data { data, .. }) => out.extend(data),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    out
}

pub(crate) fn core_data(ev: &CoreEvent) -> Option<&[u8]> {
    match ev {
        CoreEvent::Event(SerialEvent::Data { data, .. }) => Some(data),
        _ => None,
    }
}

pub(crate) fn next_frame(rx: &Receiver<CoreEvent>) -> Vec<u8> {
    loop {
        let ev = rx.recv_timeout(Duration::from_secs(3)).expect("core frame");
        if let Some(d) = core_data(&ev) {
            return d.to_vec();
        }
    }
}

/// Poll `f` until it holds; the deadline only bounds how long a failure takes.
pub(crate) fn wait_until(what: &str, wait: Duration, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + wait;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

pub(crate) fn manager_with(path: &str, port: Box<dyn SerialPort>) -> SerialManager {
    let m = SerialManager::new();
    m.insert_port(path.into(), port).expect("insert_port");
    m
}

/// A CMUX modem with the two Quectel M26 quirks seen on hardware (`420-cmuxhw`):
/// a frame within `deaf_ms` of its `AT+CMUX` OK is lost and the modem then ignores
/// every frame for 2 s; a CLD that arrives while a DISC is still unanswered is answered
/// only after ~500 ms on hardware, longer than T2. Here the UA to a DISC goes out with
/// the DISC write; the returned flag is set when a CLD arrives while that UA was not
/// read yet (the quirk's trigger). The CLD is answered at once either way.
pub(crate) fn m26_like_modem(fake: &Fake, deaf_ms: u64) -> Arc<AtomicBool> {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF, SABM, UA, UIH};
    use serialplugin_core::cmux::Deframer;
    let unread = fake.clone();
    let cld_before_ua = Arc::new(AtomicBool::new(false));
    let early = cld_before_ua.clone();
    let mut ok_at: Option<Instant> = None;
    let mut deaf_until = Instant::now();
    let mut deframer = Deframer::default();
    fake.respond(move |bytes| {
        let Some(ok) = ok_at else {
            if bytes.starts_with(b"AT+CMUX") {
                ok_at = Some(Instant::now());
                return b"\r\nOK\r\n".to_vec();
            }
            return Vec::new();
        };
        // The deaf window is the modem's own timing; a slow host only sends later.
        let now = Instant::now();
        if now < ok + Duration::from_millis(deaf_ms) {
            deaf_until = now + Duration::from_secs(2);
        }
        if now < deaf_until {
            return Vec::new();
        }
        let mut out = Vec::new();
        for f in deframer.feed(bytes) {
            match f.kind {
                SABM | DISC => out.extend(encode_frame(f.dlci, true, UA | PF, &[])),
                UIH if f.dlci == 0 && f.payload.first() == Some(&0xC3) => {
                    if unread.rx_len() > 0 {
                        early.store(true, Ordering::SeqCst);
                    }
                    out.extend(encode_frame(0, true, UIH, &[0xC1, 0x01]));
                }
                _ => {}
            }
        }
        out
    });
    cld_before_ua
}

// ---------------------------------------------------------------- CMUX on the manager

pub(crate) type MuxFrames = Arc<Mutex<Vec<(u8, Vec<u8>)>>>;

/// A fake modem: `AT+CMUX…` → OK (also again later), UA to every SABM, UIH data
/// frames recorded; CLD is never answered.
pub(crate) fn mux_fake() -> (Fake, MuxFrames) {
    use serialplugin_core::cmux::frame::{encode_frame, PF, SABM, UA, UIH};
    use serialplugin_core::cmux::Deframer;
    let fake = Fake::new();
    let uih: MuxFrames = Arc::default();
    let seen = uih.clone();
    let mut deframer = Deframer::default();
    fake.respond(move |bytes| {
        if bytes.starts_with(b"AT+CMUX") {
            deframer = Deframer::default();
            return b"\r\nOK\r\n".to_vec();
        }
        let mut out = Vec::new();
        for f in deframer.feed(bytes) {
            match f.kind {
                SABM => out.extend(encode_frame(f.dlci, true, UA | PF, &[])),
                UIH if f.dlci != 0 => seen.lock().unwrap().push((f.dlci, f.payload)),
                _ => {}
            }
        }
        out
    });
    (fake, uih)
}

pub(crate) fn open_watch(
    m: &SerialManager,
    path: &str,
    sink: CoreSink,
) -> Result<(String, serialplugin_core::events::SubscriptionId), CoreError> {
    m.open_watch_core(
        path.into(),
        115_200,
        None,
        None,
        None,
        None,
        Some(200),
        Default::default(),
        watch_opts(5, false),
        readline(),
        sink,
    )
}

// ---------------------------------------------------------------- manager API details

pub(crate) fn wait_event(rx: &Receiver<CoreEvent>, what: &str, f: impl Fn(&CoreEvent) -> bool) {
    // One deadline for the whole wait: other events must not extend it.
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let ev = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|_| panic!("no {what}"));
        if f(&ev) {
            return;
        }
    }
}

/// Wait until the hub has read `n` bytes off the fake into its idle buffer.
pub(crate) fn wait_until_idle(m: &SerialManager, fake: &Fake, path: &str, n: u32) {
    wait_until("hub buffered the bytes", Duration::from_secs(2), || {
        fake.0.rx.lock().unwrap().is_empty() && m.bytes_to_read(path.into()).unwrap() == n
    });
}

/// Wait until the hub has read every byte pushed to `fake`.
pub(crate) fn wait_until_taken(fake: &Fake) {
    wait_until("hub read the bytes", Duration::from_secs(2), || {
        fake.0.rx.lock().unwrap().is_empty()
    });
}

/// `true` when no event matching `bad` arrives within a short window (other events
/// are skipped).
pub(crate) fn quiet_for<E>(rx: &Receiver<E>, bad: impl Fn(&E) -> bool) -> bool {
    let deadline = Instant::now() + Duration::from_millis(150);
    loop {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(e) if bad(&e) => return false,
            Ok(_) => continue,
            Err(_) => return true,
        }
    }
}
