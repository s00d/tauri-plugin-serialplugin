//! The 4.2 core API from outside the crate, through this file's own fake port
//! (`SerialManager::insert_port`): error kinds, split read/write handles, disconnect
//! handling, event delivery and the per-manager watch registry.

use serial_codec::framing::FramerConfig;
use serialplugin_core::error::{CoreError, Error, ErrorKind};
use serialplugin_core::events::{
    CoreEvent, CoreSink, EventSink, ExchangeOptions, PortStatus, RxPrepareMode, SerialEvent,
    WatchOptions,
};
use serialplugin_core::state::ClearBuffer;
use serialplugin_core::SerialManager;
use serialport::SerialPort;
use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- fake port

type Responder = Box<dyn FnMut(&[u8]) -> Vec<u8> + Send>;

#[derive(Default)]
struct Shared {
    rx: Mutex<VecDeque<u8>>,
    cv: Condvar,
    tx: Mutex<Vec<u8>>,
    read_error: Mutex<Option<io::ErrorKind>>,
    write_error: Mutex<Option<io::ErrorKind>>,
    stall_writes: AtomicBool,
    /// `bytes_to_read` reports this when non-zero (bytes "in the driver").
    driver_pending: AtomicU32,
    live: AtomicUsize,
    cleared_input: AtomicUsize,
    responder: Mutex<Option<Responder>>,
}

/// Handle to a fake device; [`Fake::port`] makes the `SerialPort` for the manager.
#[derive(Clone, Default)]
struct Fake(Arc<Shared>);

struct FakePort {
    s: Arc<Shared>,
    timeout: Duration,
    clonable: bool,
}

impl Fake {
    fn new() -> Self {
        Self::default()
    }
    fn port(&self, clonable: bool) -> Box<dyn SerialPort> {
        self.0.live.fetch_add(1, Ordering::SeqCst);
        Box::new(FakePort {
            s: self.0.clone(),
            timeout: Duration::from_millis(1000),
            clonable,
        })
    }
    fn push(&self, bytes: &[u8]) {
        self.0.rx.lock().unwrap().extend(bytes);
        self.0.cv.notify_all();
    }
    fn tx(&self) -> Vec<u8> {
        self.0.tx.lock().unwrap().clone()
    }
    fn live(&self) -> usize {
        self.0.live.load(Ordering::SeqCst)
    }
    fn fail_reads(&self, kind: io::ErrorKind) {
        *self.0.read_error.lock().unwrap() = Some(kind);
        self.0.cv.notify_all();
    }
    fn fail_writes(&self, kind: io::ErrorKind) {
        *self.0.write_error.lock().unwrap() = Some(kind);
    }
    fn respond(&self, f: impl FnMut(&[u8]) -> Vec<u8> + Send + 'static) {
        *self.0.responder.lock().unwrap() = Some(Box::new(f));
    }
    /// A tiny AT modem: `AT+CSQ` → `+CSQ` (after `urc`, if any), `AT` → `OK`.
    fn modem(&self, urc: &'static str) {
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

impl Read for FakePort {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let deadline = Instant::now() + self.timeout;
        let mut rx = self.s.rx.lock().unwrap();
        loop {
            if let Some(kind) = *self.s.read_error.lock().unwrap() {
                return Err(io::Error::new(kind, "fake read error"));
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
        if let Some(kind) = *self.s.write_error.lock().unwrap() {
            return Err(io::Error::new(kind, "fake write error"));
        }
        if self.s.stall_writes.load(Ordering::SeqCst) {
            thread::sleep(self.timeout);
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
    fn set_data_bits(&mut self, _: serialport::DataBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_flow_control(&mut self, _: serialport::FlowControl) -> serialport::Result<()> {
        Ok(())
    }
    fn set_parity(&mut self, _: serialport::Parity) -> serialport::Result<()> {
        Ok(())
    }
    fn set_stop_bits(&mut self, _: serialport::StopBits) -> serialport::Result<()> {
        Ok(())
    }
    fn set_timeout(&mut self, timeout: Duration) -> serialport::Result<()> {
        self.timeout = timeout;
        Ok(())
    }
    fn write_request_to_send(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
    fn write_data_terminal_ready(&mut self, _: bool) -> serialport::Result<()> {
        Ok(())
    }
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
        Ok(false)
    }
    fn bytes_to_read(&self) -> serialport::Result<u32> {
        match self.s.driver_pending.load(Ordering::SeqCst) {
            0 => Ok(self.s.rx.lock().unwrap().len() as u32),
            n => Ok(n),
        }
    }
    fn bytes_to_write(&self) -> serialport::Result<u32> {
        Ok(0)
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
        Ok(())
    }
    fn clear_break(&self) -> serialport::Result<()> {
        Ok(())
    }
}

// ---------------------------------------------------------------- helpers

fn old_sink() -> (EventSink, Receiver<SerialEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Arc::new(move |e| tx.lock().unwrap().send(e).is_ok()), rx)
}

fn core_sink() -> (CoreSink, Receiver<CoreEvent>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Arc::new(move |e| tx.lock().unwrap().send(e).is_ok()), rx)
}

fn watch_opts(flush_ms: u64, route_urc: bool) -> WatchOptions {
    WatchOptions {
        serial_data_flush_interval_ms: Some(flush_ms),
        route_urc,
        ..Default::default()
    }
}

fn readline() -> Option<FramerConfig> {
    Some(serde_json::from_value(serde_json::json!({"type": "readline"})).unwrap())
}

/// Data bytes of old-API events received within `wait`, until `done` says stop.
fn collect_data(
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

fn core_data(ev: &CoreEvent) -> Option<&[u8]> {
    match ev {
        CoreEvent::Event(SerialEvent::Data { data, .. }) => Some(data),
        _ => None,
    }
}

fn next_frame(rx: &Receiver<CoreEvent>) -> Vec<u8> {
    loop {
        let ev = rx.recv_timeout(Duration::from_secs(3)).expect("core frame");
        if let Some(d) = core_data(&ev) {
            return d.to_vec();
        }
    }
}

fn wait_until(what: &str, wait: Duration, f: impl Fn() -> bool) {
    let deadline = Instant::now() + wait;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn manager_with(path: &str, port: Box<dyn SerialPort>) -> SerialManager {
    let m = SerialManager::new();
    m.insert_port(path.into(), port).expect("insert_port");
    m
}

// ---------------------------------------------------------------- 1. error kinds

#[test]
fn core_error_kinds_and_old_texts() {
    let m = SerialManager::new();

    // NotFound, with the 4.x text on conversion.
    let e = m.write_core("nope".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    let old = m.write("nope".into(), "x".into()).unwrap_err();
    assert_eq!(Error::from(e).to_string(), old.to_string());

    // open of a missing device: same text as `open`, kind NotFound.
    let missing = "/dev/serialplugin-does-not-exist".to_string();
    let e = m
        .open_core(
            missing.clone(),
            9600,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound, "{e}");
    let old = m
        .open(missing, 9600, None, None, None, None, None)
        .unwrap_err();
    assert_eq!(e.to_string(), old.to_string());
    assert!(matches!(Error::from(e), Error::String(_)));

    // Disconnected / PermissionDenied keep the io::Error as source.
    let fake = Fake::new();
    let m = manager_with("kinds", fake.port(true));
    for (io_kind, kind) in [
        (io::ErrorKind::BrokenPipe, ErrorKind::Disconnected),
        (io::ErrorKind::PermissionDenied, ErrorKind::PermissionDenied),
    ] {
        fake.fail_writes(io_kind);
        let e = m.write_core("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.kind(), kind, "{e}");
        let src = std::error::Error::source(&e)
            .and_then(|s| s.downcast_ref::<io::Error>())
            .expect("io source");
        assert_eq!(src.kind(), io_kind);
        let old = m.write_binary("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.to_string(), old.to_string());
    }
    *fake.0.write_error.lock().unwrap() = None;

    // Timeout.
    let opts = ExchangeOptions {
        timeout_ms: Some(100),
        rx_prepare: Some(RxPrepareMode::None),
        ..Default::default()
    };
    let e = m
        .exchange_core("kinds".into(), b"AT\r".to_vec(), opts)
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Timeout, "{e}");
    assert_eq!(e.to_string(), "exchange timed out after 100 ms");

    // Cancelled.
    let bg = m.clone();
    let t = thread::spawn(move || {
        bg.exchange_core(
            "kinds".into(),
            b"AT\r".to_vec(),
            ExchangeOptions {
                timeout_ms: Some(5000),
                rx_prepare: Some(RxPrepareMode::None),
                ..Default::default()
            },
        )
    });
    thread::sleep(Duration::from_millis(100));
    m.cancel_exchange("kinds".into()).unwrap();
    let e = t.join().unwrap().unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Cancelled, "{e}");

    // Busy / Other from old errors.
    assert_eq!(
        CoreError::from(Error::new("read already in progress")).kind(),
        ErrorKind::Busy
    );
    assert_eq!(
        CoreError::from(Error::new("weird")).kind(),
        ErrorKind::Other
    );
    m.close_core("kinds".into()).unwrap();
}

// ---------------------------------------------------------------- 2. split handles

#[test]
fn set_timeout_takes_effect_and_hub_does_not_overwrite_it() {
    let fake = Fake::new();
    let m = manager_with("timeouts", fake.port(true));
    fake.0.stall_writes.store(true, Ordering::SeqCst);
    m.set_timeout("timeouts".into(), Duration::from_millis(80))
        .unwrap();
    let t = Instant::now();
    assert!(m.write("timeouts".into(), "x".into()).is_err());
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );

    let (sink, _rx) = old_sink();
    m.watch("timeouts".into(), watch_opts(10, false), sink)
        .unwrap();
    thread::sleep(Duration::from_millis(150)); // the hub polls meanwhile
    let t = Instant::now();
    assert!(m.write("timeouts".into(), "x".into()).is_err());
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    m.close("timeouts".into()).unwrap();
}

#[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
#[test]
fn write_p99_under_3ms_with_reading_active() {
    let (_guard, mut master, slave) = serialplugin_core::sync_util::pty_pair_locked();
    let m = manager_with("pty-fast-write", Box::new(slave));
    let (sink, _rx) = old_sink();
    m.watch("pty-fast-write".into(), watch_opts(10, false), sink)
        .unwrap();
    master.set_timeout(Duration::from_millis(20)).unwrap();
    let stop = Arc::new(AtomicBool::new(false));
    let stop_bg = stop.clone();
    let drain = thread::spawn(move || {
        let mut buf = [0u8; 1024];
        while !stop_bg.load(Ordering::SeqCst) {
            let _ = master.read(&mut buf);
        }
    });
    let mut times = Vec::new();
    for _ in 0..300 {
        let t = Instant::now();
        m.write_binary("pty-fast-write".into(), b"ping\n".to_vec())
            .unwrap();
        times.push(t.elapsed());
    }
    stop.store(true, Ordering::SeqCst);
    drain.join().unwrap();
    times.sort();
    let p99 = times[times.len() * 99 / 100];
    assert!(p99 < Duration::from_millis(3), "p99 write {p99:?}");
    m.close("pty-fast-write".into()).unwrap();
}

// ---------------------------------------------------------------- 3. disconnect

#[test]
fn disconnect_releases_both_handles_once_and_close_succeeds() {
    let fake = Fake::new();
    let m = manager_with("unplug", fake.port(true));
    assert_eq!(fake.live(), 2, "reader + cloned writer");
    let (sub, states) = core_sink();
    let sub = m.subscribe_core(sub);
    let (sink, rx) = old_sink();
    m.watch("unplug".into(), watch_opts(10, false), sink)
        .unwrap();

    fake.fail_reads(io::ErrorKind::BrokenPipe);
    let ev = rx.recv_timeout(Duration::from_secs(3)).expect("disconnect");
    assert!(matches!(ev, SerialEvent::Disconnect { .. }), "{ev:?}");
    wait_until("both handles closed", Duration::from_secs(3), || {
        fake.live() == 0
    });
    // 4.x: the path stays managed (and `watch` gets `Disconnect`) until `close`.
    assert!(m.managed_ports().unwrap().contains(&"unplug".to_string()));
    thread::sleep(Duration::from_millis(200));
    assert!(rx.try_recv().is_err(), "Disconnect exactly once");
    let disconnected: Vec<_> = states
        .try_iter()
        .filter(|e| {
            matches!(e, CoreEvent::PortState { state: PortStatus::Disconnected { kind, .. }, .. }
                if *kind == ErrorKind::Disconnected)
        })
        .collect();
    assert_eq!(disconnected.len(), 1);

    m.close("unplug".into()).expect("close after disconnect");
    assert!(!m.managed_ports().unwrap().contains(&"unplug".to_string()));
    assert_eq!(
        m.close("unplug".into()).unwrap_err().to_string(),
        "Serial port unplug is not open!"
    );
    m.unsubscribe_core(sub).unwrap();
}

#[test]
fn intentional_close_delivers_tail_in_order_without_disconnect() {
    let fake = Fake::new();
    let m = manager_with("bye", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    let (sink, rx) = old_sink();
    m.watch("bye".into(), watch_opts(5_000, false), sink)
        .unwrap();
    let (csink, crx) = core_sink();
    m.watch_core("bye".into(), watch_opts(5_000, false), readline(), csink)
        .unwrap();
    fake.push(b"head\ntail-data");
    thread::sleep(Duration::from_millis(150)); // in the batch / the parser
    let t = Instant::now();
    m.close("bye".into()).unwrap();
    assert!(t.elapsed() < Duration::from_secs(1));

    let data = collect_data(&rx, Duration::from_secs(2), |d| d.len() >= 14);
    assert_eq!(data, b"head\ntail-data");
    thread::sleep(Duration::from_millis(100));
    assert!(rx
        .try_iter()
        .all(|e| !matches!(e, SerialEvent::Disconnect { .. })));
    assert_eq!(next_frame(&crx), b"head");
    assert_eq!(next_frame(&crx), b"tail-data");
    let states: Vec<_> = states.try_iter().collect();
    assert!(states.iter().any(|e| matches!(
        e,
        CoreEvent::PortState {
            state: PortStatus::Closed,
            ..
        }
    )));
    assert!(!states.iter().any(|e| matches!(
        e,
        CoreEvent::PortState {
            state: PortStatus::Disconnected { .. },
            ..
        }
    )));
}

#[test]
fn subscribe_core_reports_opened_and_closed_without_a_watch() {
    let (sub, states) = core_sink();
    let m = SerialManager::new();
    let id = m.subscribe_core(sub);
    let fake = Fake::new();
    m.insert_port("states".into(), fake.port(true)).unwrap();
    m.close("states".into()).unwrap();
    let got: Vec<_> = (0..2)
        .map(|_| states.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect();
    assert!(
        matches!(&got[0], CoreEvent::PortState { path, state: PortStatus::Opened } if path == "states")
    );
    assert!(
        matches!(&got[1], CoreEvent::PortState { path, state: PortStatus::Closed } if path == "states")
    );
    m.unsubscribe_core(id).unwrap();
    assert_eq!(
        m.unsubscribe_core(id).unwrap_err().kind(),
        ErrorKind::NotFound
    );
}

// ---------------------------------------------------------------- 6. delivery

#[test]
fn slow_core_sink_never_stalls_hub_at_or_close() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("slow", fake.port(true));
    let first = AtomicBool::new(true);
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let sink: CoreSink = Arc::new(move |e| {
        if core_data(&e).is_some() && first.swap(false, Ordering::SeqCst) {
            thread::sleep(Duration::from_secs(10));
        }
        if let CoreEvent::Event(e) = e {
            return tx.lock().unwrap().send(e).is_ok();
        }
        true
    });
    m.watch_core("slow".into(), watch_opts(5, false), None, sink)
        .unwrap();
    fake.push(b"one ");
    thread::sleep(Duration::from_millis(100));
    fake.push(b"two ");
    thread::sleep(Duration::from_millis(100));

    let t = Instant::now();
    let reply = m.at("slow".into(), "AT+CSQ".into(), None).unwrap();
    assert!(t.elapsed() < Duration::from_secs(1), "at {:?}", t.elapsed());
    assert!(reply.lines.iter().any(|l| l == "+CSQ: 20,99"), "{reply:?}");

    fake.push(b"three");
    thread::sleep(Duration::from_millis(100));
    let t = Instant::now();
    m.close("slow".into()).unwrap();
    assert!(
        t.elapsed() < Duration::from_secs(1),
        "close {:?}",
        t.elapsed()
    );

    let data = collect_data(&rx, Duration::from_secs(15), |d| d.len() >= 13);
    assert_eq!(data, b"one two three");
}

#[test]
fn rust_receiver_overflow_unsubscribes_and_port_keeps_working() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("flood", fake.port(true));
    m.set_core_queue_capacity(4096);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let gate = Mutex::new(Some(gate_rx));
    let (tx, slow_rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let slow: CoreSink = Arc::new(move |e| {
        if let Some(g) = gate.lock().unwrap().take() {
            let _ = g.recv();
        }
        tx.lock().unwrap().send(e).is_ok()
    });
    let opts = WatchOptions {
        serial_data_flush_interval_ms: Some(1),
        size: Some(256),
        ..Default::default()
    };
    m.watch_core("flood".into(), opts.clone(), None, slow)
        .unwrap();
    let (fast, fast_rx) = core_sink();
    m.watch_core("flood".into(), opts, None, fast).unwrap();

    for _ in 0..64 {
        fake.push(&[b'z'; 256]);
        thread::sleep(Duration::from_millis(2));
    }
    let mut fast_bytes = 0;
    while fast_bytes < 64 * 256 {
        let ev = fast_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("fast sink");
        fast_bytes += core_data(&ev).map_or(0, <[u8]>::len);
    }
    assert!(m.managed_ports().unwrap().contains(&"flood".to_string()));
    m.at("flood".into(), "AT".into(), None)
        .expect("at after overflow");

    gate_tx.send(()).unwrap();
    let mut got = 0u64;
    let dropped_from = loop {
        match slow_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("overflow")
        {
            CoreEvent::Overflow { dropped_from } => break dropped_from,
            ev => got += core_data(&ev).map_or(0, |d| d.len() as u64),
        }
    };
    assert_eq!(dropped_from, got);
    assert!(dropped_from < 64 * 256);
    fake.push(b"after");
    thread::sleep(Duration::from_millis(200));
    assert!(slow_rx.try_recv().is_err(), "unsubscribed after Overflow");
    m.close("flood".into()).unwrap();
}

#[test]
fn core_sinks_may_call_at_on_the_same_port() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("reentrant", fake.port(true));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mgr = m.clone();
    let core: CoreSink = Arc::new(move |e| {
        if core_data(&e).is_some() {
            let r = mgr.at("reentrant".into(), "AT".into(), None);
            let _ = tx.lock().unwrap().send(r.is_ok());
        }
        true
    });
    m.watch_core("reentrant".into(), watch_opts(5, false), None, core)
        .unwrap();
    fake.push(b"ping");
    assert!(rx
        .recv_timeout(Duration::from_secs(5))
        .expect("sink at() hung"));
    m.close("reentrant".into()).unwrap();
}

/// 4.x timing of the old `watch`: the sink runs on the RX side (the hub thread, or the
/// caller of `unwatch` / `close` for the tail), never on a queue behind it, so nothing
/// reaches it once `close()` has returned.
#[test]
fn old_watch_sink_runs_on_the_rx_side_as_in_4x() {
    let fake = Fake::new();
    let m = manager_with("timing", fake.port(true));
    let closed = Arc::new(AtomicBool::new(false));
    let late = Arc::new(AtomicUsize::new(0));
    let queued = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink: EventSink = {
        let (closed, late, queued, got) =
            (closed.clone(), late.clone(), queued.clone(), got.clone());
        Arc::new(move |e| {
            if closed.load(Ordering::SeqCst) {
                late.fetch_add(1, Ordering::SeqCst);
            }
            if thread::current().name() == Some("serial-watch-delivery") {
                queued.fetch_add(1, Ordering::SeqCst);
            }
            if let SerialEvent::Data { data, .. } = e {
                got.lock().unwrap().extend(data);
            }
            thread::sleep(Duration::from_millis(30));
            true
        })
    };
    // A 5 s batch: "head" is still in the hub at `close()`, which delivers it.
    m.watch("timing".into(), watch_opts(5_000, false), sink)
        .unwrap();
    fake.push(b"head");
    thread::sleep(Duration::from_millis(150));
    m.close("timing".into()).unwrap();
    closed.store(true, Ordering::SeqCst);
    thread::sleep(Duration::from_millis(200));
    assert_eq!(late.load(Ordering::SeqCst), 0, "sink called after close()");
    assert_eq!(
        queued.load(Ordering::SeqCst),
        0,
        "sink behind a delivery queue"
    );
    assert_eq!(got.lock().unwrap().as_slice(), b"head");
}

// ---------------------------------------------------------------- 7. registry

#[test]
fn registry_is_per_manager_and_one_old_watch_per_port() {
    let (f1, f2) = (Fake::new(), Fake::new());
    let m1 = manager_with("same-path", f1.port(true));
    let m2 = manager_with("same-path", f2.port(true));
    m1.insert_port("other".into(), Fake::new().port(true))
        .unwrap();
    let (s, _r1) = old_sink();
    let id = m1
        .watch("same-path".into(), WatchOptions::default(), s)
        .unwrap();
    let (s, _r2) = old_sink();
    m2.watch("same-path".into(), WatchOptions::default(), s)
        .expect("other manager, same path");
    let (s, _r3) = old_sink();
    let err = m1
        .watch("same-path".into(), WatchOptions::default(), s)
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "A watch is already active for port same-path"
    );
    let (s, _r4) = old_sink();
    let err = m1
        .watch_with_id(id, "other".into(), WatchOptions::default(), s)
        .unwrap_err();
    assert_eq!(err.to_string(), format!("watch id {id} is already in use"));
    for _ in 0..3 {
        let (s, _r) = core_sink();
        m1.watch_core("same-path".into(), WatchOptions::default(), None, s)
            .expect("watch_core is unlimited");
    }
    m1.close_all().unwrap();
    m2.close_all().unwrap();
}

#[test]
fn watch_core_subscribers_have_their_own_parser_and_copy() {
    let fake = Fake::new();
    let m = manager_with("fanout", fake.port(true));
    let (old, old_rx) = old_sink();
    m.watch_framed("fanout".into(), WatchOptions::default(), readline(), old)
        .unwrap();
    let (a, a_rx) = core_sink();
    let a_id = m
        .watch_core("fanout".into(), WatchOptions::default(), readline(), a)
        .unwrap();
    let (b, b_rx) = core_sink();
    m.watch_core("fanout".into(), watch_opts(5, false), None, b)
        .unwrap();

    fake.push(b"x1\npar");
    assert_eq!(next_frame(&a_rx), b"x1");
    m.unwatch_core(a_id).unwrap();
    assert_eq!(next_frame(&a_rx), b"par", "remainder on unwatch_core");
    fake.push(b"tial\n");
    let lines = collect_data(&old_rx, Duration::from_secs(3), |d| d.len() >= 9);
    assert_eq!(lines, b"x1partial");
    let mut raw = Vec::new();
    while raw.len() < 11 {
        raw.extend(next_frame(&b_rx));
    }
    assert_eq!(raw, b"x1\npartial\n");
    thread::sleep(Duration::from_millis(100));
    assert!(a_rx.try_iter().all(|e| core_data(&e).is_none()));
    assert_eq!(
        m.unwatch_core(a_id).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    m.close("fanout".into()).unwrap();
}

#[test]
fn urcs_during_at_go_to_every_route_urc_subscriber() {
    let fake = Fake::new();
    fake.modem("\r\n+CREG: 1,1\r\n");
    let m = manager_with("urc", fake.port(true));
    let (u1, u1_rx) = core_sink();
    let (u2, u2_rx) = core_sink();
    let (raw, raw_rx) = core_sink();
    m.watch_core("urc".into(), watch_opts(5, true), None, u1)
        .unwrap();
    m.watch_core("urc".into(), watch_opts(5, true), None, u2)
        .unwrap();
    m.watch_core("urc".into(), watch_opts(5, false), None, raw)
        .unwrap();
    let reply = m.at("urc".into(), "AT+CSQ".into(), None).unwrap();
    assert!(reply.lines.iter().any(|l| l == "+CSQ: 20,99"), "{reply:?}");
    for rx in [&u1_rx, &u2_rx] {
        let ev = rx.recv_timeout(Duration::from_secs(2)).expect("urc copy");
        assert!(
            matches!(&ev, CoreEvent::Event(SerialEvent::Urc { line, .. }) if line == "+CREG: 1,1"),
            "{ev:?}"
        );
    }
    thread::sleep(Duration::from_millis(100));
    assert!(raw_rx
        .try_iter()
        .all(|e| !matches!(e, CoreEvent::Event(SerialEvent::Urc { .. }))));
    m.close("urc".into()).unwrap();
}

#[test]
fn core_sink_returning_false_is_removed() {
    let fake = Fake::new();
    let m = manager_with("gone", fake.port(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let sink: CoreSink = Arc::new(move |_| {
        c.fetch_add(1, Ordering::SeqCst);
        false
    });
    let id = m
        .watch_core("gone".into(), watch_opts(5, false), None, sink)
        .unwrap();
    for chunk in [b"a", b"b", b"c"] {
        fake.push(chunk);
        thread::sleep(Duration::from_millis(60));
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The stream is gone, so its id is too.
    assert_eq!(m.unwatch_core(id).unwrap_err().kind(), ErrorKind::NotFound);
    m.close("gone".into()).unwrap();
}

// ---------------------------------------------------------------- 10. bugs

#[test]
fn exchange_waiter_first_finish_wins() {
    use serialplugin_core::hub::ExchangeWaiter;
    let options = ExchangeOptions {
        command: Some("AT".into()),
        ..Default::default()
    }
    .resolve();
    let waiter = ExchangeWaiter::new(options, Arc::new(AtomicBool::new(false)));
    waiter.push_bytes(b"AT\r\r\nOK\r\n");
    waiter.fail_with_reason("late failure".into());
    assert!(waiter.wait(100).is_ok(), "the completed reply must win");
}

#[test]
fn clear_input_also_clears_the_hub_buffer() {
    let fake = Fake::new();
    let m = manager_with("stale", fake.port(true));
    fake.push(b"stale");
    wait_until("hub buffered the bytes", Duration::from_secs(2), || {
        m.bytes_to_read("stale".into()).unwrap() == 5
    });
    m.clear_buffer("stale".into(), ClearBuffer::Input).unwrap();
    assert_eq!(m.bytes_to_read("stale".into()).unwrap(), 0);
    assert!(fake.0.cleared_input.load(Ordering::SeqCst) >= 1);
    assert!(m.read("stale".into(), Some(50), None).is_err());
    m.close("stale".into()).unwrap();
}

#[test]
fn bytes_to_read_is_honest_while_the_hub_reads() {
    for clonable in [true, false] {
        let fake = Fake::new();
        fake.0.driver_pending.store(7, Ordering::SeqCst);
        let m = manager_with("honest", fake.port(clonable));
        let (s, _rx) = old_sink();
        m.watch("honest".into(), WatchOptions::default(), s)
            .unwrap();
        for _ in 0..30 {
            assert_eq!(
                m.bytes_to_read("honest".into()).unwrap(),
                7,
                "clonable={clonable}"
            );
        }
        m.close("honest".into()).unwrap();
    }
}

// ---------------------------------------------------------------- 11. CMUX write

#[test]
fn write_on_cmux_virtual_path_goes_out_as_uih() {
    use serialplugin_core::cmux::frame::{encode_frame, PF, SABM, UA, UIH};
    use serialplugin_core::cmux::Deframer;
    let fake = Fake::new();
    type Frames = Arc<Mutex<Vec<(u8, Vec<u8>)>>>;
    let uih: Frames = Arc::default();
    let seen = uih.clone();
    let mut mux = false;
    let mut deframer = Deframer::default();
    fake.respond(move |bytes| {
        if !mux {
            if bytes.starts_with(b"AT+CMUX") {
                mux = true;
                return b"\r\nOK\r\n".to_vec();
            }
            return Vec::new();
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
    let m = manager_with("muxed", fake.port(true));
    m.enable_mux("muxed".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let vpath = m.open_mux_channel("muxed".into(), 2).unwrap();
    assert_eq!(m.write(vpath.clone(), "hello".into()).unwrap(), 5);
    assert_eq!(m.write_binary(vpath, vec![1, 2, 3]).unwrap(), 3);
    wait_until("UIH frames", Duration::from_secs(2), || {
        uih.lock().unwrap().len() >= 2
    });
    let got = uih.lock().unwrap().clone();
    assert_eq!(got, vec![(2, b"hello".to_vec()), (2, vec![1, 2, 3])]);
    m.close("muxed".into()).unwrap();
    let _ = fake.tx();
}

/// A CMUX modem with the two Quectel M26 quirks seen on hardware (`420-cmuxhw`):
/// a frame within `deaf_ms` of its `AT+CMUX` OK is lost and the modem then ignores
/// every frame for 2 s; a CLD that arrives while a DISC is still unanswered (UA goes
/// out 5 ms after the DISC) is answered only after 500 ms, longer than T2.
fn m26_like_modem(fake: &Fake, deaf_ms: u64) {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF, SABM, UA, UIH};
    use serialplugin_core::cmux::Deframer;
    let push_later = {
        let fake = fake.clone();
        move |ms: u64, bytes: Vec<u8>| {
            let fake = fake.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(ms));
                fake.push(&bytes);
            });
        }
    };
    let fake_rx = fake.clone();
    let disc_pending = Arc::new(AtomicBool::new(false));
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
                SABM => out.extend(encode_frame(f.dlci, true, UA | PF, &[])),
                DISC => {
                    disc_pending.store(true, Ordering::SeqCst);
                    let pending = disc_pending.clone();
                    let ua = encode_frame(f.dlci, true, UA | PF, &[]);
                    let fake = fake_rx.clone();
                    thread::spawn(move || {
                        thread::sleep(Duration::from_millis(5));
                        pending.store(false, Ordering::SeqCst);
                        fake.push(&ua);
                    });
                }
                UIH if f.dlci == 0 && f.payload.first() == Some(&0xC3) => {
                    let rsp = encode_frame(0, true, UIH, &[0xC1, 0x01]);
                    let delay = if disc_pending.load(Ordering::SeqCst) {
                        500
                    } else {
                        1
                    };
                    push_later(delay, rsp);
                }
                _ => {}
            }
        }
        out
    });
}

/// Root cause 1: SABM went out right after the `AT+CMUX` OK, inside the window in
/// which the modem is still switching, and every retry hit the modem's deaf period.
#[test]
fn enable_mux_right_after_open_waits_for_the_modem_to_switch() {
    let fake = Fake::new();
    m26_like_modem(&fake, 30);
    let m = manager_with("m26-open", fake.port(true));
    m.enable_mux(
        "m26-open".into(),
        "AT+CMUX=0,0,5,127,10,3,30,10,2".into(),
        1000,
    )
    .expect("enable_mux with no traffic before it");
    m.disable_mux("m26-open".into()).unwrap();
    m.close("m26-open".into()).unwrap();
}

/// Root cause 2: closing a channel sent DISC and CLD back to back; the modem holds its
/// CLD response while the DISC is in progress, so `disable_mux` waited out all of T2.
#[test]
fn disable_mux_with_a_channel_gets_the_cld_response_well_before_t2() {
    let fake = Fake::new();
    m26_like_modem(&fake, 0);
    let m = manager_with("m26-cld", fake.port(true));
    m.enable_mux(
        "m26-cld".into(),
        "AT+CMUX=0,0,5,127,10,3,30,10,2".into(),
        1000,
    )
    .unwrap();
    m.open_mux_channel("m26-cld".into(), 1).unwrap();
    let t = Instant::now();
    m.disable_mux("m26-cld".into()).unwrap();
    let took = t.elapsed();
    assert!(
        took < Duration::from_millis(150),
        "disable_mux took {took:?}"
    );
    m.close("m26-cld".into()).unwrap();
}

// ---------------------------------------------------------------- 4.2.0 audit fixes

/// 4.1.0 (`audit420/base.log`): a port that dies before any watch keeps its path in
/// `managed_ports`, a late `watch` gets the stored `Disconnect`, calls fail with the
/// disconnect reason, and `close` removes it and succeeds once.
#[test]
fn a_port_that_died_before_its_watch_behaves_as_in_4x() {
    let fake = Fake::new();
    let m = manager_with("dis", fake.port(true));
    fake.fail_reads(io::ErrorKind::BrokenPipe);
    wait_until("handles released", Duration::from_secs(3), || {
        fake.live() == 0
    });
    let reason = "Serial port disconnected: fake read error";
    assert_eq!(m.managed_ports().unwrap(), vec!["dis".to_string()]);

    let (s, rx) = old_sink();
    m.watch("dis".into(), WatchOptions::default(), s)
        .expect("late watch");
    match rx.recv_timeout(Duration::from_secs(2)).expect("Disconnect") {
        SerialEvent::Disconnect { path, reason: r } => {
            assert_eq!((path.as_str(), r.as_str()), ("dis", reason));
        }
        other => panic!("{other:?}"),
    }
    let (s, _r) = old_sink();
    assert_eq!(
        m.watch("dis".into(), WatchOptions::default(), s)
            .unwrap_err()
            .to_string(),
        "A watch is already active for port dis"
    );
    let e = m.write_core("dis".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Disconnected);
    assert_eq!(e.to_string(), reason);
    assert_eq!(
        m.write("dis".into(), "x".into()).unwrap_err().to_string(),
        reason
    );
    assert_eq!(
        m.set_baud_rate("dis".into(), 9600).unwrap_err().to_string(),
        reason
    );
    let (cs, crx) = core_sink();
    m.watch_core("dis".into(), WatchOptions::default(), None, cs)
        .expect("late watch_core");
    assert!(matches!(
        crx.recv_timeout(Duration::from_secs(2)).unwrap(),
        CoreEvent::Event(SerialEvent::Disconnect { .. })
    ));

    m.close("dis".into()).expect("close after disconnect");
    assert!(m.managed_ports().unwrap().is_empty());
    assert_eq!(
        m.close("dis".into()).unwrap_err().to_string(),
        "Serial port dis is not open!"
    );
    let (s, _r) = old_sink();
    assert_eq!(
        m.watch("dis".into(), WatchOptions::default(), s)
            .unwrap_err()
            .to_string(),
        "Port 'dis' not found"
    );
}

/// Windows reports an unplugged adapter as PermissionDenied: still `Disconnected`.
#[test]
fn port_state_kind_of_an_unplug_is_disconnected() {
    let fake = Fake::new();
    let m = manager_with("win-unplug", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    fake.fail_reads(io::ErrorKind::PermissionDenied);
    let kind = loop {
        match states.recv_timeout(Duration::from_secs(5)).expect("state") {
            CoreEvent::PortState {
                state: PortStatus::Disconnected { kind, .. },
                ..
            } => break kind,
            _ => continue,
        }
    };
    assert_eq!(kind, ErrorKind::Disconnected);
    m.close("win-unplug".into()).unwrap();
}

/// "Opened elsewhere": flock and Windows ERROR_ACCESS_DENIED come as `NoDevice`.
#[test]
fn port_in_use_errors_are_busy() {
    for text in [
        "Unable to acquire exclusive lock on serial port",
        "Access is denied.",
        "Device or resource busy",
    ] {
        let e = serialport::Error::new(serialport::ErrorKind::NoDevice, text);
        assert_eq!(ErrorKind::from_serialport(&e), ErrorKind::Busy, "{text}");
    }
    let gone = serialport::Error::new(serialport::ErrorKind::NoDevice, "No such file");
    assert_eq!(ErrorKind::from_serialport(&gone), ErrorKind::NotFound);
}

/// The 4.x global registry must not lose a manager's watch to another manager
/// registering the same id.
#[test]
fn a_watch_id_taken_by_another_manager_is_refused() {
    use serialplugin_core::port::watch_registry as wr;
    let a = manager_with("id-a", Fake::new().port(true));
    let b = manager_with("id-b", Fake::new().port(true));
    let id = 0xABCD_0101;
    let (s, _ra) = old_sink();
    a.watch_with_id(id, "id-a".into(), WatchOptions::default(), s)
        .unwrap();
    let (s, _rb) = old_sink();
    assert_eq!(
        b.watch_with_id(id, "id-b".into(), WatchOptions::default(), s)
            .unwrap_err()
            .to_string(),
        format!("watch id {id} is already in use")
    );
    assert_eq!(wr::paths_for_port("id-a"), vec![id]);
    a.unwatch(id).unwrap();
    assert!(!wr::contains(id));
    a.close("id-a".into()).unwrap();
    b.close("id-b".into()).unwrap();
}

/// A device that fails at once after `insert_port`: `Opened`, then `Disconnected`.
#[test]
fn insert_port_of_a_dead_device_reports_opened_then_disconnected() {
    for round in 0..5 {
        let fake = Fake::new();
        fake.fail_reads(io::ErrorKind::BrokenPipe);
        let m = SerialManager::new();
        let (sub, states) = core_sink();
        m.subscribe_core(sub);
        m.insert_port("dead".into(), fake.port(true)).unwrap();
        let first = states.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(
                &first,
                CoreEvent::PortState {
                    state: PortStatus::Opened,
                    ..
                }
            ),
            "round {round}: {first:?}"
        );
        let second = states
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| panic!("round {round}: no Disconnected"));
        assert!(
            matches!(
                &second,
                CoreEvent::PortState {
                    state: PortStatus::Disconnected { .. },
                    ..
                }
            ),
            "round {round}: {second:?}"
        );
        assert_eq!(m.managed_ports().unwrap(), vec!["dead".to_string()]);
        m.close("dead".into()).unwrap();
    }
}

/// A big split write gets at least twice its wire time (4.x), however short the
/// port timeout; the port timeout is back for the next write.
#[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
#[test]
fn split_write_waits_at_least_twice_the_wire_time() {
    let fake = Fake::new();
    let m = manager_with("wire", fake.port(true));
    m.set_timeout("wire".into(), Duration::from_millis(80))
        .unwrap();
    fake.0.stall_writes.store(true, Ordering::SeqCst);
    // 6000 bytes at 115200 baud: ~573 ms on the wire.
    let t = Instant::now();
    assert!(m.write_binary("wire".into(), vec![0; 6000]).is_err());
    assert!(
        t.elapsed() >= Duration::from_millis(1000),
        "{:?}",
        t.elapsed()
    );
    let t = Instant::now();
    assert!(m.write("wire".into(), "x".into()).is_err());
    assert!(
        t.elapsed() < Duration::from_millis(500),
        "{:?}",
        t.elapsed()
    );
    m.close("wire".into()).unwrap();
}

// ---------------------------------------------------------------- CMUX on the manager

type MuxFrames = Arc<Mutex<Vec<(u8, Vec<u8>)>>>;

/// A fake modem: `AT+CMUX…` → OK (also again later), UA to every SABM, UIH data
/// frames recorded; CLD is never answered.
fn mux_fake() -> (Fake, MuxFrames) {
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

/// Waiting for the CLD response (T2) must not block calls on other ports.
#[test]
fn closing_a_muxed_port_does_not_hold_the_manager_lock() {
    let (fake, _uih) = mux_fake();
    let m = manager_with("slow-cld", fake.port(true));
    m.insert_port("other".into(), Fake::new().port(true))
        .unwrap();
    // T2 = 100 (1 s); the fake never answers CLD.
    m.enable_mux("slow-cld".into(), "AT+CMUX=0,0,5,31,10,3,100".into(), 1000)
        .unwrap();
    let bg = m.clone();
    let closing = thread::spawn(move || bg.close("slow-cld".into()));
    thread::sleep(Duration::from_millis(150));
    let t = Instant::now();
    m.write("other".into(), "x".into()).unwrap();
    m.managed_ports().unwrap();
    assert!(
        t.elapsed() < Duration::from_millis(300),
        "blocked behind the CLD wait: {:?}",
        t.elapsed()
    );
    closing.join().unwrap().unwrap();
    m.close("other".into()).unwrap();
}

/// The modem closes DLCI 2: `Disconnect` on the channel watch, `PortState` for the
/// virtual path; `openMuxChannel` again gives a clean channel a new watch can use.
#[test]
fn channel_closed_by_the_modem_reports_state_and_reopens_cleanly() {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF, UIH};
    let (fake, _uih) = mux_fake();
    let m = manager_with("dlc", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    m.enable_mux("dlc".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("dlc".into(), 2).unwrap();
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s).unwrap();

    fake.push(&encode_frame(2, true, DISC | PF, &[]));
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).expect("Disconnect") {
            SerialEvent::Disconnect { path, .. } => break assert_eq!(path, v),
            _ => continue,
        }
    }
    loop {
        match states.recv_timeout(Duration::from_secs(2)).expect("state") {
            CoreEvent::PortState {
                path,
                state: PortStatus::Disconnected { kind, .. },
            } if path == v => break assert_eq!(kind, ErrorKind::Disconnected),
            _ => continue,
        }
    }
    assert!(m.write(v.clone(), "x".into()).is_err());

    assert_eq!(m.open_mux_channel("dlc".into(), 2).unwrap(), v);
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s)
        .expect("watch on the re-opened channel");
    fake.push(&encode_frame(2, true, UIH, b"hello"));
    assert_eq!(
        collect_data(&rx, Duration::from_secs(2), |d| d.len() >= 5),
        b"hello"
    );
    m.close("dlc".into()).unwrap();
}

/// DISC on DLCI 0: the modem left multiplexer mode. Channel watches get
/// `Disconnect`, channel writes fail and the session is detached, so CMUX can be
/// started again.
#[test]
fn modem_leaving_mux_tears_the_session_down() {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF};
    let (fake, _uih) = mux_fake();
    let m = manager_with("left", fake.port(true));
    m.enable_mux("left".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("left".into(), 1).unwrap();
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s).unwrap();

    fake.push(&encode_frame(0, true, DISC | PF, &[]));
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).expect("Disconnect") {
            SerialEvent::Disconnect { path, .. } => break assert_eq!(path, v),
            _ => continue,
        }
    }
    wait_until("channel write fails", Duration::from_secs(2), || {
        m.write(v.clone(), "x".into()).is_err()
    });
    wait_until("CMUX can start again", Duration::from_secs(3), || {
        m.enable_mux("left".into(), "AT+CMUX=0".into(), 1000)
            .is_ok()
    });
    m.close("left".into()).unwrap();
}
