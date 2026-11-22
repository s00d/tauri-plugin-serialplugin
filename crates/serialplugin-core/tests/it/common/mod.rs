//! Helpers on the core's types; the devices come from `serialplugin-testing`. Each module
//! uses some of them.
#![allow(dead_code)]

pub use serialplugin_testing::{
    eventually, mux, peer, Call, FakeDevice, Lines, Modem, ModemLog, MuxOptions, WAIT,
};

use serial_codec::framing::FramerConfig;
use serialplugin_core::error::{CoreError, Error, ErrorKind};
use serialplugin_core::events::{
    CoreEvent, ExchangeOptions, PortStatus, RxPrepareMode, SerialEvent, WatchOptions,
};
use serialplugin_core::SerialManager;
use serialport::SerialPort;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

/// A manager with `port` open as `path`.
pub fn manager_with(path: &str, port: impl SerialPort + 'static) -> SerialManager {
    let m = SerialManager::new();
    m.insert_port(path.into(), Box::new(port))
        .expect("insert_port");
    m
}

/// An event of a watch or of a `watch_core` stream.
pub trait Event {
    fn serial(&self) -> Option<&SerialEvent>;
}

impl Event for SerialEvent {
    fn serial(&self) -> Option<&SerialEvent> {
        Some(self)
    }
}

impl Event for CoreEvent {
    fn serial(&self) -> Option<&SerialEvent> {
        match self {
            CoreEvent::Event(e) => Some(e),
            _ => None,
        }
    }
}

pub type Sink<E> = Arc<dyn Fn(E) -> bool + Send + Sync>;

/// A sink into a channel.
pub fn sink<E: Send + 'static>() -> (Sink<E>, Receiver<E>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (Arc::new(move |e| tx.lock().unwrap().send(e).is_ok()), rx)
}

/// The next event; panics when none comes.
pub fn next<E>(rx: &Receiver<E>, what: &str) -> E {
    rx.recv_timeout(WAIT)
        .unwrap_or_else(|e| panic!("no {what}: {e}"))
}

/// The first event `pick` accepts (others are skipped).
pub fn pick<E, T>(rx: &Receiver<E>, what: &str, mut pick: impl FnMut(&E) -> Option<T>) -> T {
    // One deadline for the whole wait: other events must not extend it.
    let deadline = Instant::now() + WAIT;
    loop {
        let event = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|e| panic!("no {what}: {e}"));
        if let Some(found) = pick(&event) {
            return found;
        }
    }
}

pub fn data<E: Event>(event: &E) -> Option<Vec<u8>> {
    match event.serial()? {
        SerialEvent::Data { data, size, .. } => {
            assert_eq!(*size, data.len());
            Some(data.clone())
        }
        _ => None,
    }
}

/// The bytes of the next `Data`.
pub fn next_data<E: Event>(rx: &Receiver<E>) -> Vec<u8> {
    pick(rx, "data", data)
}

/// `Data` bytes until at least `n` arrived.
pub fn collect<E: Event>(rx: &Receiver<E>, n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    while out.len() < n {
        out.extend(next_data(rx));
    }
    out
}

/// The next URC line.
pub fn next_urc<E: Event>(rx: &Receiver<E>) -> String {
    pick(rx, "URC", |e| match e.serial() {
        Some(SerialEvent::Urc { line, .. }) => Some(line.clone()),
        _ => None,
    })
}

/// Path and reason of the next `Disconnect`.
pub fn next_disconnect<E: Event>(rx: &Receiver<E>) -> (String, String) {
    pick(rx, "Disconnect", |e| match e.serial() {
        Some(SerialEvent::Disconnect { path, reason }) => Some((path.clone(), reason.clone())),
        _ => None,
    })
}

/// The next state of `path`.
pub fn next_state(rx: &Receiver<CoreEvent>, path: &str) -> PortStatus {
    pick(rx, "port state", |e| match e {
        CoreEvent::PortState { path: p, state } if p == path => Some(state.clone()),
        _ => None,
    })
}

/// Wait until `done` holds; a model's checks then show what is missing.
pub fn settle(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !done() && Instant::now() < deadline {
        std::thread::yield_now();
    }
}

/// What is left of a stream whose sender is gone.
pub fn rest<E>(rx: Receiver<E>) -> Vec<E> {
    let mut out = Vec::new();
    loop {
        match rx.recv_timeout(WAIT) {
            Ok(e) => out.push(e),
            Err(mpsc::RecvTimeoutError::Disconnected) => return out,
            Err(e) => panic!("the sink is still held: {e}"),
        }
    }
}

pub fn kind(e: Error) -> ErrorKind {
    CoreError::from(e).kind()
}

/// A call's result as `Ok` or the kind of its error.
pub fn to_kind<T>(r: Result<T, Error>) -> Result<(), ErrorKind> {
    r.map(|_| ()).map_err(kind)
}

pub fn core_kind<T>(r: Result<T, CoreError>) -> Result<(), ErrorKind> {
    r.map(|_| ()).map_err(|e| e.kind())
}

/// Watch options: batch window and URC split.
pub fn watch_opts(flush_ms: u64, route_urc: bool) -> WatchOptions {
    WatchOptions {
        serial_data_flush_interval_ms: Some(flush_ms),
        route_urc,
        ..Default::default()
    }
}

/// A batch window no test waits out.
pub const NEVER: u64 = 3_600_000;

/// The `readline` parser (LF).
pub fn readline() -> Option<FramerConfig> {
    parser(serde_json::json!({"type": "readline"}))
}

pub fn parser(config: serde_json::Value) -> Option<FramerConfig> {
    Some(serde_json::from_value(config).expect("parser config"))
}

/// Exchange options without RX preparation.
pub fn no_prepare(timeout_ms: u64) -> ExchangeOptions {
    ExchangeOptions {
        timeout_ms: Some(timeout_ms),
        rx_prepare: Some(RxPrepareMode::None),
        ..Default::default()
    }
}

/// Longer than any test.
pub const LONG_MS: u64 = 60_000;

/// Run `call` on a clone of `m` in the background.
pub fn background<T: Send + 'static>(
    m: &SerialManager,
    call: impl FnOnce(SerialManager) -> T + Send + 'static,
) -> JoinHandle<T> {
    let m = m.clone();
    std::thread::spawn(move || call(m))
}

/// Wait until the hub holds `n` bytes no one took.
pub fn wait_idle(m: &SerialManager, device: &FakeDevice, path: &str, n: u32) {
    eventually("the hub buffered the bytes", || {
        device.unread() == 0 && m.bytes_to_read(path.into()).unwrap() == n
    });
}

/// Open `path` with a `readline` `watch_core` on it.
pub fn open_watch(
    m: &SerialManager,
    path: &str,
    sink: Sink<CoreEvent>,
) -> Result<(String, serialplugin_core::SubscriptionId), CoreError> {
    m.open_watch_core(
        path.into(),
        115_200,
        None,
        None,
        None,
        None,
        Some(200),
        Default::default(),
        watch_opts(0, false),
        readline(),
        sink,
    )
}

/// `open` with the defaults.
pub fn open(m: &SerialManager, path: &str) -> Result<String, Error> {
    m.open(path.into(), 115_200, None, None, None, None, Some(100))
}
