//! Model test of `SerialManager` through its public API, over the fake device of
//! `common`: random action sequences (open, close, watches, RX, reads, writes,
//! exchanges, disconnects, ...) on a few paths, each checked against a reference
//! model after every step.
//!
//! The model decides what each step should return (the `ErrorKind` of an error, never
//! its text), who receives each RX byte (watches > idle buffer; a read takes what fits
//! and leaves the rest), which port-state events go out, and which paths are managed.
//! A step the model calls unobservable in the current state returns `None` and is
//! skipped, so a shrunk sequence stays valid.

use crate::common::*;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{
    CoreEvent, ExchangeCompletionMode, ExchangeOptions, PortStatus, RxPrepareMode, SerialEvent,
    SubscriptionId, WatchOptions,
};
use serialplugin_core::state::ClearBuffer;
use serialplugin_core::{CoreError, Error, SerialManager};
use std::collections::BTreeSet;
use std::io;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Paths per case (calls on a path not open are calls on an unknown or dead path).
const PATHS: usize = 2;
/// Timeout of a `Read` step that may wait (nothing pending): short, it is waited out.
const READ_MS: u64 = 20;

#[derive(Clone, Debug)]
enum Action {
    Open {
        p: usize,
        clonable: bool,
    },
    Close(usize),
    ForceClose(usize),
    CloseAll,
    /// The device fails and `close` follows at once: `close` must succeed exactly once.
    CloseRacingDisconnect(usize),
    Watch(usize),
    Unwatch(usize),
    WatchCore(usize),
    UnwatchCore(usize),
    /// A port-state subscriber (`subscribe_core`).
    Subscribe,
    Unsubscribe(usize),
    /// The device sends bytes.
    Push(usize, Vec<u8>),
    /// A read with a short timeout (`fill`: `read_binary`, else `read`).
    Read {
        p: usize,
        max: usize,
        fill: bool,
    },
    /// A read that the bytes the device sends next complete.
    ReadThenPush {
        p: usize,
        max: usize,
        fill: bool,
        bytes: Vec<u8>,
    },
    CancelRead(usize),
    Write(usize, Vec<u8>),
    /// An exchange the device answers with `body` + "\n".
    Exchange(usize, Vec<u8>),
    CancelExchange(usize),
    Clear(usize, ClearBuffer),
    SetBaud(usize, u32),
    Disconnect {
        p: usize,
        eof: bool,
    },
}

/// What a call returns: `Ok` or the kind of its error.
type Kind = Result<(), ErrorKind>;

#[derive(Clone, Debug, PartialEq)]
enum State {
    Opened,
    Closed,
    Disconnected(String),
    /// `close` raced a disconnect: either may have won.
    ClosedOrDisconnected(String),
}

#[derive(Clone, Debug)]
enum Port {
    Absent,
    Open {
        dev: usize,
        clonable: bool,
        idle: Vec<u8>,
    },
    /// Released after a disconnect (the 4.x tombstone), with its reason.
    Gone,
}

/// One watch (old or core): registered in the manager, attached to a live hub, and
/// what it should have received.
#[derive(Clone, Debug, Default)]
struct Rec {
    path: usize,
    registered: bool,
    attached: bool,
    data: Vec<u8>,
    disconnects: usize,
}

#[derive(Clone, Debug)]
struct Model {
    ports: Vec<Port>,
    /// Bytes each device should have been sent.
    devices: Vec<Vec<u8>>,
    watches: Vec<Rec>,
    cores: Vec<Rec>,
    /// Port states each subscriber should have got (`None`: unsubscribed).
    subscribers: Vec<Option<Vec<(usize, State)>>>,
    reads: Vec<Result<Vec<u8>, ErrorKind>>,
    exchanges: Vec<Result<Vec<u8>, ErrorKind>>,
}

const BROKEN_PIPE: &str = "Serial port disconnected: fake read error";
const EOF: &str = "Serial port disconnected: EOF";

impl Model {
    fn new() -> Self {
        Self {
            ports: vec![Port::Absent; PATHS],
            devices: Vec::new(),
            watches: Vec::new(),
            cores: Vec::new(),
            subscribers: vec![Some(Vec::new())],
            reads: Vec::new(),
            exchanges: Vec::new(),
        }
    }

    fn state(&mut self, p: usize, s: State) {
        for sub in self.subscribers.iter_mut().flatten() {
            sub.push((p, s.clone()));
        }
    }

    fn recs(&mut self) -> impl Iterator<Item = &mut Rec> {
        self.watches.iter_mut().chain(self.cores.iter_mut())
    }

    fn watched(&self, p: usize) -> bool {
        self.watches
            .iter()
            .chain(&self.cores)
            .any(|r| r.path == p && r.attached)
    }

    /// The error of a call on a path that is not open.
    fn not_open(&self, p: usize) -> Option<ErrorKind> {
        match &self.ports[p] {
            Port::Open { .. } => None,
            Port::Gone => Some(ErrorKind::Disconnected),
            Port::Absent => Some(ErrorKind::NotFound),
        }
    }

    fn kind_on_open(&self, p: usize) -> Kind {
        self.not_open(p).map_or(Ok(()), Err)
    }

    fn idle(&mut self, p: usize) -> &mut Vec<u8> {
        match &mut self.ports[p] {
            Port::Open { idle, .. } => idle,
            _ => unreachable!("not open"),
        }
    }

    /// The watches stop: the registry forgets them, the hub is gone.
    fn forget(&mut self, p: usize) {
        for r in self.recs().filter(|r| r.path == p) {
            r.registered = false;
            r.attached = false;
        }
    }

    fn close(&mut self, p: usize) -> Kind {
        let was = std::mem::replace(&mut self.ports[p], Port::Absent);
        self.forget(p);
        match was {
            Port::Open { .. } => {
                self.state(p, State::Closed);
                Ok(())
            }
            Port::Gone => Ok(()),
            Port::Absent => Err(ErrorKind::NotFound),
        }
    }

    /// Bytes from the device: the watches of the port get a copy each, else idle.
    fn rx(&mut self, p: usize, bytes: &[u8]) {
        if self.watched(p) {
            for r in self.recs().filter(|r| r.path == p && r.attached) {
                r.data.extend_from_slice(bytes);
            }
        } else {
            self.idle(p).extend_from_slice(bytes);
        }
    }

    /// Run `a`: what the call returns, or `None` when the step is not observable now.
    fn step(&mut self, a: &Action) -> Option<Kind> {
        use Action::*;
        Some(match a {
            Open { p, clonable } => {
                if !matches!(self.ports[*p], Port::Absent) {
                    let _ = self.close(*p);
                }
                self.devices.push(Vec::new());
                self.ports[*p] = Port::Open {
                    dev: self.devices.len() - 1,
                    clonable: *clonable,
                    idle: Vec::new(),
                };
                self.state(*p, State::Opened);
                Ok(())
            }
            Close(p) => self.close(*p),
            ForceClose(p) => {
                let _ = self.close(*p);
                Ok(())
            }
            CloseAll => {
                for p in 0..PATHS {
                    if !matches!(self.ports[p], Port::Absent) {
                        let _ = self.close(p);
                    }
                }
                Ok(())
            }
            CloseRacingDisconnect(p) => {
                // Watches would see either outcome: only the close / release race here.
                let busy = self.recs().any(|r| r.path == *p && r.registered);
                if !matches!(self.ports[*p], Port::Open { .. }) || busy {
                    return None;
                }
                self.ports[*p] = Port::Absent;
                self.state(*p, State::ClosedOrDisconnected(BROKEN_PIPE.into()));
                Ok(())
            }
            Watch(p) => {
                if self.watches.iter().any(|r| r.path == *p && r.registered) {
                    return Some(Err(ErrorKind::Busy));
                }
                match &self.ports[*p] {
                    Port::Absent => return Some(Err(ErrorKind::NotFound)),
                    // A late watch on a released port gets its `Disconnect`.
                    Port::Gone => self.watches.push(Rec {
                        path: *p,
                        registered: true,
                        disconnects: 1,
                        ..Rec::default()
                    }),
                    Port::Open { .. } => {
                        self.idle(*p).clear();
                        self.watches.push(Rec {
                            path: *p,
                            registered: true,
                            attached: true,
                            ..Rec::default()
                        });
                    }
                }
                Ok(())
            }
            Unwatch(i) => {
                if self.watches.is_empty() {
                    return None;
                }
                let n = self.watches.len();
                let r = &mut self.watches[i % n];
                let was = r.registered;
                r.registered = false;
                r.attached = false;
                if was {
                    Ok(())
                } else {
                    Err(ErrorKind::NotFound)
                }
            }
            WatchCore(p) => {
                match &self.ports[*p] {
                    Port::Absent => return Some(Err(ErrorKind::NotFound)),
                    Port::Gone => self.cores.push(Rec {
                        path: *p,
                        disconnects: 1,
                        ..Rec::default()
                    }),
                    Port::Open { .. } => {
                        self.idle(*p).clear();
                        self.cores.push(Rec {
                            path: *p,
                            registered: true,
                            attached: true,
                            ..Rec::default()
                        });
                    }
                }
                Ok(())
            }
            UnwatchCore(i) => {
                if self.cores.is_empty() {
                    return None;
                }
                let n = self.cores.len();
                let r = &mut self.cores[i % n];
                let ok = r.registered && r.attached;
                r.registered = false;
                r.attached = false;
                if ok {
                    Ok(())
                } else {
                    Err(ErrorKind::NotFound)
                }
            }
            Subscribe => {
                self.subscribers.push(Some(Vec::new()));
                Ok(())
            }
            Unsubscribe(i) => {
                let n = self.subscribers.len();
                let sub = &mut self.subscribers[i % n];
                if sub.take().is_some() {
                    Ok(())
                } else {
                    Err(ErrorKind::NotFound)
                }
            }
            Push(p, bytes) => {
                if !matches!(self.ports[*p], Port::Open { .. }) {
                    return None; // no device on the path
                }
                self.rx(*p, bytes);
                Ok(())
            }
            Read { p, max, fill } => {
                let result = match self.not_open(*p) {
                    Some(kind) => Err(kind),
                    None if self.watched(*p) => Err(ErrorKind::Other),
                    None => {
                        let idle = self.idle(*p);
                        let n = (*max).min(idle.len());
                        let got: Vec<u8> = idle.drain(..n).collect();
                        // A fill read waits for the rest and times out with what it has.
                        if got.is_empty() {
                            Err(ErrorKind::Other)
                        } else {
                            Ok(got)
                        }
                    }
                };
                let kind = result.as_ref().map(|_| ()).map_err(|k| *k);
                let _ = fill;
                self.reads.push(result);
                kind
            }
            ReadThenPush {
                p,
                max,
                fill,
                bytes,
            } => {
                // Deterministic only when the read waits (nothing idle) and the bytes
                // complete it, whether its slot is in before the bytes arrive or not.
                let open = matches!(&self.ports[*p], Port::Open { idle, .. } if idle.is_empty());
                if !open || self.watched(*p) || (*fill && bytes.len() < *max) {
                    return None;
                }
                let n = (*max).min(bytes.len());
                self.reads.push(Ok(bytes[..n].to_vec()));
                self.idle(*p).extend_from_slice(&bytes[n..]);
                Ok(())
            }
            CancelRead(p) => self.kind_on_open(*p),
            Write(p, bytes) => {
                let kind = self.kind_on_open(*p);
                if let Port::Open { dev, .. } = self.ports[*p] {
                    self.devices[dev].extend_from_slice(bytes);
                }
                kind
            }
            Exchange(p, body) => {
                let result = match self.ports[*p].clone() {
                    Port::Open { dev, .. } => {
                        self.devices[dev].push(b'?');
                        self.devices[dev].extend_from_slice(body);
                        // Bytes nobody took yet come first, then the reply.
                        let mut reply = std::mem::take(self.idle(*p));
                        reply.extend_from_slice(body);
                        reply.push(b'\n');
                        Ok(reply)
                    }
                    _ => Err(self.not_open(*p).unwrap()),
                };
                let kind = result.as_ref().map(|_| ()).map_err(|k| *k);
                self.exchanges.push(result);
                kind
            }
            CancelExchange(p) => self.kind_on_open(*p),
            Clear(p, which) => {
                let kind = self.kind_on_open(*p);
                if kind.is_ok() && !matches!(which, ClearBuffer::Output) {
                    self.idle(*p).clear();
                }
                kind
            }
            SetBaud(p, _) => self.kind_on_open(*p),
            Disconnect { p, eof } => {
                if !matches!(self.ports[*p], Port::Open { .. }) {
                    return None;
                }
                let reason = if *eof { EOF } else { BROKEN_PIPE };
                for r in self.recs().filter(|r| r.path == *p && r.attached) {
                    r.disconnects += 1;
                    r.attached = false;
                }
                self.ports[*p] = Port::Gone;
                self.state(*p, State::Disconnected(reason.into()));
                Ok(())
            }
        })
    }

    /// Device events need a device: they go to the `p`-th open path (by index among
    /// the open ones), `None` when no path is open.
    fn resolve(&self, a: &Action) -> Option<Action> {
        let open: Vec<usize> = (0..PATHS)
            .filter(|p| matches!(self.ports[*p], Port::Open { .. }))
            .collect();
        let pick = |p: usize| (!open.is_empty()).then(|| open[p % open.len()]);
        Some(match a.clone() {
            Action::Push(p, b) => Action::Push(pick(p)?, b),
            Action::Disconnect { p, eof } => Action::Disconnect { p: pick(p)?, eof },
            Action::CloseRacingDisconnect(p) => Action::CloseRacingDisconnect(pick(p)?),
            Action::ReadThenPush {
                p,
                max,
                fill,
                bytes,
            } => Action::ReadThenPush {
                p: pick(p)?,
                max,
                fill,
                bytes,
            },
            other => other,
        })
    }

    /// Paths `managed_ports` lists.
    fn managed(&self) -> BTreeSet<usize> {
        (0..PATHS)
            .filter(|p| !matches!(self.ports[*p], Port::Absent))
            .collect()
    }
}

// ---------------------------------------------------------------- the system

#[derive(Default)]
struct Got {
    data: Vec<u8>,
    disconnects: usize,
}

type Shared = Arc<Mutex<Got>>;

fn watch_recorder() -> (serialplugin_core::EventSink, Shared) {
    let got: Shared = Arc::default();
    let g = got.clone();
    let sink: serialplugin_core::EventSink = Arc::new(move |ev| {
        let mut g = g.lock().unwrap();
        match ev {
            SerialEvent::Data { data, .. } => g.data.extend(data),
            SerialEvent::Disconnect { .. } => g.disconnects += 1,
            // URC copies of an exchange reply and read-error reports carry no data.
            SerialEvent::Urc { .. } | SerialEvent::Error { .. } => {}
            #[allow(unreachable_patterns)]
            other => panic!("unexpected watch event {other:?}"),
        }
        true
    });
    (sink, got)
}

fn core_recorder() -> (serialplugin_core::CoreSink, Shared) {
    let (sink, got) = watch_recorder();
    let core: serialplugin_core::CoreSink = Arc::new(move |ev| match ev {
        CoreEvent::Event(e) => sink(e),
        other => panic!("unexpected core event {other:?}"),
    });
    (core, got)
}

#[allow(clippy::type_complexity)]
struct Sut {
    m: SerialManager,
    names: Vec<String>,
    devices: Vec<Fake>,
    /// Device index of the port last opened on each path.
    current: Vec<usize>,
    watch_ids: Vec<u32>,
    watches: Vec<Shared>,
    core_ids: Vec<SubscriptionId>,
    cores: Vec<Shared>,
    /// Each port-state subscriber: id, its events, the states seen so far.
    subscribers: Vec<(SubscriptionId, Receiver<CoreEvent>, Vec<(usize, State)>)>,
    reads: Vec<Result<Vec<u8>, ErrorKind>>,
    exchanges: Vec<Result<Vec<u8>, ErrorKind>>,
}

fn kind(e: Error) -> ErrorKind {
    CoreError::from(e).kind()
}

fn to_kind<T>(r: Result<T, Error>) -> Kind {
    r.map(|_| ()).map_err(kind)
}

fn core_kind<T>(r: Result<T, CoreError>) -> Kind {
    r.map(|_| ()).map_err(|e| e.kind())
}

/// The fake answers a write that starts with '?' with the rest of it and "\n".
fn answering_device() -> Fake {
    let fake = Fake::new();
    fake.respond(|bytes| match bytes.split_first() {
        Some((b'?', body)) => [body, b"\n"].concat(),
        _ => Vec::new(),
    });
    fake
}

fn watch_options() -> WatchOptions {
    WatchOptions {
        serial_data_flush_interval_ms: Some(0),
        ..Default::default()
    }
}

impl Sut {
    fn new(case: usize) -> Self {
        let m = SerialManager::new();
        let (sink, rx) = core_sink();
        let id = m.subscribe_core(sink);
        Self {
            m,
            names: (0..PATHS).map(|p| format!("case{case}_p{p}")).collect(),
            devices: Vec::new(),
            current: vec![0; PATHS],
            watch_ids: Vec::new(),
            watches: Vec::new(),
            core_ids: Vec::new(),
            cores: Vec::new(),
            subscribers: vec![(id, rx, Vec::new())],
            reads: Vec::new(),
            exchanges: Vec::new(),
        }
    }

    /// The device of the port last opened on `p`.
    fn device(&self, p: usize) -> &Fake {
        &self.devices[self.current[p]]
    }

    fn name(&self, p: usize) -> String {
        self.names[p].clone()
    }

    fn step(&mut self, a: &Action) -> Kind {
        use Action::*;
        let m = self.m.clone();
        match a {
            Open { p, clonable } => {
                let fake = answering_device();
                let r = m.insert_port(self.name(*p), fake.port(*clonable));
                self.devices.push(fake);
                self.current[*p] = self.devices.len() - 1;
                core_kind(r)
            }
            Close(p) => core_kind(m.close_core(self.name(*p))),
            ForceClose(p) => to_kind(m.force_close(self.name(*p))),
            CloseAll => to_kind(m.close_all()),
            CloseRacingDisconnect(p) => {
                self.device(*p).fail_reads(io::ErrorKind::BrokenPipe);
                let first = core_kind(m.close_core(self.name(*p)));
                let again = core_kind(m.close_core(self.name(*p)));
                assert_eq!(again, Err(ErrorKind::NotFound), "close succeeded twice");
                first
            }
            Watch(p) => {
                let (sink, got) = watch_recorder();
                let r = m.watch(self.name(*p), watch_options(), sink);
                if let Ok(id) = &r {
                    self.watch_ids.push(*id);
                    self.watches.push(got);
                }
                to_kind(r)
            }
            Unwatch(i) => to_kind(m.unwatch(self.watch_ids[i % self.watch_ids.len()])),
            WatchCore(p) => {
                let (sink, got) = core_recorder();
                let r = m.watch_core(self.name(*p), watch_options(), None, sink);
                if let Ok(id) = &r {
                    assert!(!self.core_ids.contains(id), "watch_core id {id:?} reused");
                    self.core_ids.push(*id);
                    self.cores.push(got);
                }
                core_kind(r)
            }
            UnwatchCore(i) => core_kind(m.unwatch_core(self.core_ids[i % self.core_ids.len()])),
            Subscribe => {
                let (sink, rx) = core_sink();
                let id = m.subscribe_core(sink);
                assert!(
                    self.subscribers.iter().all(|s| s.0 != id),
                    "subscription id reused"
                );
                self.subscribers.push((id, rx, Vec::new()));
                Ok(())
            }
            Unsubscribe(i) => {
                let n = self.subscribers.len();
                core_kind(m.unsubscribe_core(self.subscribers[i % n].0))
            }
            Push(p, bytes) => {
                self.device(*p).push(bytes);
                Ok(())
            }
            Read { p, max, fill } => {
                let r = if *fill {
                    m.read_binary(self.name(*p), Some(READ_MS), Some(*max))
                } else {
                    m.read(self.name(*p), Some(READ_MS), Some(*max))
                        .map(String::into_bytes)
                };
                let r = r.map_err(kind);
                let k = r.as_ref().map(|_| ()).map_err(|k| *k);
                self.reads.push(r);
                k
            }
            ReadThenPush {
                p,
                max,
                fill,
                bytes,
            } => {
                let (bg, name, max, fill) = (m.clone(), self.name(*p), *max, *fill);
                let reader = std::thread::spawn(move || {
                    if fill {
                        bg.read_binary(name, Some(3_000), Some(max))
                    } else {
                        bg.read(name, Some(3_000), Some(max))
                            .map(String::into_bytes)
                    }
                });
                self.device(*p).push(bytes);
                let r = reader.join().unwrap().map_err(kind);
                let k = r.as_ref().map(|_| ()).map_err(|k| *k);
                self.reads.push(r);
                k
            }
            CancelRead(p) => to_kind(m.cancel_read(self.name(*p))),
            Write(p, bytes) => to_kind(m.write_binary(self.name(*p), bytes.clone())),
            Exchange(p, body) => {
                let options = ExchangeOptions {
                    timeout_ms: Some(3_000),
                    rx_prepare: Some(RxPrepareMode::None),
                    completion_mode: Some(ExchangeCompletionMode::Substring),
                    terminators: Some(vec!["\n".into()]),
                    ..Default::default()
                };
                let payload = [b"?", body.as_slice()].concat();
                let r = m
                    .exchange_binary(self.name(*p), payload, options)
                    .map(|reply| reply.raw)
                    .map_err(kind);
                let k = r.as_ref().map(|_| ()).map_err(|k| *k);
                self.exchanges.push(r);
                k
            }
            CancelExchange(p) => to_kind(m.cancel_exchange(self.name(*p))),
            Clear(p, which) => to_kind(m.clear_buffer(self.name(*p), *which)),
            SetBaud(p, baud) => to_kind(m.set_baud_rate(self.name(*p), *baud)),
            Disconnect { p, eof } => {
                let dev = self.device(*p);
                if *eof {
                    dev.eof();
                } else {
                    dev.fail_reads(io::ErrorKind::BrokenPipe);
                }
                Ok(())
            }
        }
    }

    /// Wait until what is asynchronous (hub threads, delivery queues, handles) matches
    /// the model, then compare everything.
    fn check(&mut self, model: &Model) {
        // Only bounds how long a failing case takes; a loaded host may need seconds.
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            for s in &mut self.subscribers {
                while let Ok(ev) = s.1.try_recv() {
                    let CoreEvent::PortState { path, state } = ev else {
                        panic!("unexpected subscriber event {ev:?}");
                    };
                    let p = self
                        .names
                        .iter()
                        .position(|n| *n == path)
                        .expect("own path");
                    s.2.push((p, seen(state)));
                }
            }
            if self.settled(model) {
                break;
            }
            assert!(
                Instant::now() < deadline,
                "never settled: {}",
                self.diff(model)
            );
            std::thread::yield_now();
        }
        assert_eq!(self.diff(model), "");
    }

    fn settled(&self, model: &Model) -> bool {
        let recs = |got: &[Shared], want: &[Rec]| {
            got.len() == want.len()
                && got.iter().zip(want).all(|(g, w)| {
                    let g = g.lock().unwrap();
                    g.data.len() >= w.data.len() && g.disconnects >= w.disconnects
                })
        };
        let states = self
            .subscribers
            .iter()
            .zip(&model.subscribers)
            .all(|(s, w)| s.2.len() >= w.as_ref().map_or(s.2.len(), Vec::len));
        let ports = (0..PATHS).all(|p| match &model.ports[p] {
            Port::Open { dev, idle, .. } => {
                self.devices[*dev].rx_len() == 0
                    && self.m.bytes_to_read(self.name(p)).ok() == Some(idle.len() as u32)
            }
            _ => true,
        });
        let live = self
            .devices
            .iter()
            .enumerate()
            .all(|(d, f)| f.live() == expected_handles(model, d));
        recs(&self.watches, &model.watches)
            && recs(&self.cores, &model.cores)
            && states
            && ports
            && live
    }

    /// Differences from the model, empty when there are none.
    fn diff(&self, model: &Model) -> String {
        let mut out = String::new();
        let mut cmp = |what: String, ok: bool| {
            if !ok {
                out.push_str(&what);
                out.push('\n');
            }
        };
        for (i, (g, w)) in self.watches.iter().zip(&model.watches).enumerate() {
            let g = g.lock().unwrap();
            cmp(
                format!(
                    "watch #{i}: data {:?} / {:?}, disconnects {} / {}",
                    g.data, w.data, g.disconnects, w.disconnects
                ),
                g.data == w.data && g.disconnects == w.disconnects,
            );
        }
        for (i, (g, w)) in self.cores.iter().zip(&model.cores).enumerate() {
            let g = g.lock().unwrap();
            cmp(
                format!(
                    "watch_core #{i}: data {:?} / {:?}, disconnects {} / {}",
                    g.data, w.data, g.disconnects, w.disconnects
                ),
                g.data == w.data && g.disconnects == w.disconnects,
            );
        }
        cmp(
            format!(
                "watch count {} / {}",
                self.watches.len(),
                model.watches.len()
            ),
            self.watches.len() == model.watches.len(),
        );
        cmp(
            format!(
                "watch_core count {} / {}",
                self.cores.len(),
                model.cores.len()
            ),
            self.cores.len() == model.cores.len(),
        );
        for (i, (s, w)) in self.subscribers.iter().zip(&model.subscribers).enumerate() {
            if let Some(w) = w {
                let ok = s.2.len() == w.len()
                    && s.2.iter().zip(w).all(|((p, got), (q, want))| {
                        p == q
                            && match want {
                                State::ClosedOrDisconnected(r) => {
                                    *got == State::Closed || *got == State::Disconnected(r.clone())
                                }
                                want => got == want,
                            }
                    });
                cmp(format!("subscriber #{i}: {:?} / {:?}", s.2, w), ok);
            }
        }
        cmp(
            format!("reads {:?} / {:?}", self.reads, model.reads),
            self.reads == model.reads,
        );
        cmp(
            format!("exchanges {:?} / {:?}", self.exchanges, model.exchanges),
            self.exchanges == model.exchanges,
        );
        for (d, f) in self.devices.iter().enumerate() {
            cmp(
                format!("device {d} tx {:?} / {:?}", f.tx(), model.devices[d]),
                f.tx() == model.devices[d],
            );
            cmp(
                format!(
                    "device {d} handles {} / {}",
                    f.live(),
                    expected_handles(model, d)
                ),
                f.live() == expected_handles(model, d),
            );
        }
        let mut managed: Vec<String> = model.managed().iter().map(|p| self.name(*p)).collect();
        managed.sort();
        let got = self.m.managed_ports().unwrap();
        cmp(
            format!("managed_ports {got:?} / {managed:?}"),
            got == managed,
        );
        for (p, port) in model.ports.iter().enumerate().take(PATHS) {
            if let Port::Open { idle, .. } = port {
                let n = self.m.bytes_to_read(self.name(p)).ok();
                cmp(
                    format!("{}: bytes_to_read {n:?} / {}", self.name(p), idle.len()),
                    n == Some(idle.len() as u32),
                );
            }
        }
        out
    }
}

/// Handles a device should have open: reader + split writer for a clonable port (one
/// handle on Windows), none once its port is closed or gone.
fn expected_handles(model: &Model, dev: usize) -> usize {
    let open = model.ports.iter().find_map(|p| match p {
        Port::Open {
            dev: d, clonable, ..
        } if *d == dev => Some(*clonable),
        _ => None,
    });
    match open {
        Some(true) if !cfg!(windows) => 2,
        Some(_) => 1,
        None => 0,
    }
}

fn seen(state: PortStatus) -> State {
    match state {
        PortStatus::Opened => State::Opened,
        PortStatus::Closed => State::Closed,
        PortStatus::Disconnected { kind, reason } => {
            assert_eq!(kind, ErrorKind::Disconnected, "{reason}");
            State::Disconnected(reason)
        }
        other => panic!("unexpected port state {other:?}"),
    }
}

// ---------------------------------------------------------------- runner

fn bytes(len: std::ops::Range<usize>) -> impl Strategy<Value = Vec<u8>> {
    // No '\n' or '?': only an exchange reply ends a line.
    vec(b'a'..=b'h', len)
}

fn action() -> impl Strategy<Value = Action> {
    let p = || 0..PATHS;
    let clear = prop::sample::select(vec![
        ClearBuffer::Input,
        ClearBuffer::Output,
        ClearBuffer::All,
    ]);
    prop_oneof![
        4 => (p(), any::<bool>()).prop_map(|(p, clonable)| Action::Open { p, clonable }),
        1 => p().prop_map(Action::Close),
        1 => p().prop_map(Action::ForceClose),
        1 => Just(Action::CloseAll),
        1 => p().prop_map(Action::CloseRacingDisconnect),
        2 => p().prop_map(Action::Watch),
        1 => any::<usize>().prop_map(Action::Unwatch),
        2 => p().prop_map(Action::WatchCore),
        1 => any::<usize>().prop_map(Action::UnwatchCore),
        1 => Just(Action::Subscribe),
        1 => any::<usize>().prop_map(Action::Unsubscribe),
        10 => (p(), bytes(1..8)).prop_map(|(p, b)| Action::Push(p, b)),
        1 => (p(), 1usize..8, any::<bool>()).prop_map(|(p, max, fill)| Action::Read { p, max, fill }),
        3 => (p(), 1usize..8, any::<bool>(), bytes(1..8))
            .prop_map(|(p, max, fill, bytes)| Action::ReadThenPush { p, max, fill, bytes }),
        1 => p().prop_map(Action::CancelRead),
        2 => (p(), bytes(1..8)).prop_map(|(p, b)| Action::Write(p, b)),
        2 => (p(), bytes(0..6)).prop_map(|(p, b)| Action::Exchange(p, b)),
        1 => p().prop_map(Action::CancelExchange),
        1 => (p(), clear).prop_map(|(p, c)| Action::Clear(p, c)),
        1 => (p(), any::<u32>()).prop_map(|(p, b)| Action::SetBaud(p, b)),
        2 => (p(), any::<bool>()).prop_map(|(p, eof)| Action::Disconnect { p, eof }),
    ]
}

static CASE: AtomicUsize = AtomicUsize::new(0);

fn run_case(actions: &[Action]) {
    let mut model = Model::new();
    let mut sut = Sut::new(CASE.fetch_add(1, Ordering::SeqCst));
    for a in actions {
        let Some(a) = model.resolve(a) else {
            continue;
        };
        let Some(want) = model.step(&a) else {
            continue;
        };
        let got = sut.step(&a);
        assert_eq!(got, want, "result of {a:?}");
        sut.check(&model);
    }
    sut.m.close_all().unwrap();
}

/// Random sequences of manager calls and device events against the model.
#[test]
fn manager_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 12,
        failure_persistence: None,
        max_shrink_time: 4_000,
        ..Config::default()
    });
    if let Err(e) = runner.run(&vec(action(), 1..32), |actions| {
        run_case(&actions);
        Ok(())
    }) {
        panic!("{e}");
    }
}
