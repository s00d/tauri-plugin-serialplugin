//! Model test of the hub's RX routing: random sequences of chunks and consumer
//! changes, checked after every step against a reference model of who gets each byte
//! (drain > exchange waiter > read slot > watches > idle buffer).
//!
//! In `src/` (not `tests/`): it drives private hooks no public entry reaches, the
//! `push_idle` race, `watch_core` slots, read / drain deadlines and the waiter's buffer.
//! The manager model in `tests/manager_model.rs` covers the same routing end to end.

use super::*;
use crate::error::ErrorKind;
use crate::events::{ExchangeCompletionMode, ExchangeOptions};
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use std::collections::BTreeMap;
use std::sync::mpsc::Receiver;
use std::thread::JoinHandle;

/// Read timeout of a started read: never reached, reads end by bytes, `ExpireRead`,
/// cancel or failure.
const READ_MS: u64 = 30_000;
const PATH: &str = "model";

#[derive(Clone, Debug)]
pub(super) enum Step {
    /// A chunk the hub read off the port.
    Feed(Vec<u8>),
    /// A chunk that reaches `push_idle` after `feed_bytes` saw no consumer: the race in
    /// which a drain, waiter or read slot is installed between the two.
    RaceFeed(Vec<u8>),
    SetWaiter,
    ClearWaiter,
    /// `cancel_exchange`: the waiter fails but stays installed until cleared.
    CancelExchange,
    /// `take_idle_bytes` (the drain prepare throws them away).
    TakeIdle,
    StartRead {
        max: usize,
        fill: bool,
    },
    /// The read's deadline passes and the hub ticks.
    ExpireRead,
    CancelRead,
    StartDrain,
    /// The drain's deadline passes and the hub ticks.
    EndDrain,
    AttachWatch,
    DetachWatch,
    AttachCore,
    DetachCore(usize),
    Purge,
    FailAll,
}

#[derive(Clone, Debug)]
pub(super) struct PendingRead {
    max: usize,
    fill: bool,
    buf: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Model {
    idle: Vec<u8>,
    drain: Option<Vec<u8>>,
    /// Bytes of the installed waiter and the failure it got, if any.
    waiter: Option<(Vec<u8>, Option<String>)>,
    read: Option<PendingRead>,
    /// Index of the current old watch in `watch_got`.
    watch: Option<usize>,
    cores: Vec<u64>,
    next_core: u64,
    watch_got: Vec<Vec<u8>>,
    core_got: BTreeMap<u64, Vec<u8>>,
    /// Bytes of each cleared waiter and its `wait` error.
    waiters_done: Vec<(Vec<u8>, String)>,
    taken: Vec<Vec<u8>>,
    reads_done: Vec<Result<Vec<u8>, String>>,
    drains_done: Vec<Result<Vec<u8>, String>>,
}

impl Model {
    fn has_watches(&self) -> bool {
        self.watch.is_some() || !self.cores.is_empty()
    }

    fn give_watches(&mut self, chunk: &[u8]) {
        if let Some(w) = self.watch {
            self.watch_got[w].extend_from_slice(chunk);
        }
        for id in &self.cores {
            self.core_got.get_mut(id).unwrap().extend_from_slice(chunk);
        }
    }

    /// The read slot takes what fits; a completed slot leaves the rest of the chunk.
    fn give_read<'a>(&mut self, chunk: &'a [u8]) -> &'a [u8] {
        let Some(r) = self.read.as_mut() else {
            return chunk;
        };
        let take = chunk.len().min(r.max - r.buf.len());
        r.buf.extend_from_slice(&chunk[..take]);
        if !r.fill || r.buf.len() >= r.max {
            let r = self.read.take().unwrap();
            self.reads_done.push(Ok(r.buf));
        }
        &chunk[take..]
    }

    fn give_idle(&mut self, chunk: &[u8]) {
        self.idle.extend_from_slice(chunk);
        let excess = self.idle.len().saturating_sub(IDLE_BUFFER_CAP);
        self.idle.drain(..excess);
    }

    fn feed(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }
        if let Some(d) = self.drain.as_mut() {
            d.extend_from_slice(chunk);
            return;
        }
        if let Some((w, _)) = self.waiter.as_mut() {
            w.extend_from_slice(chunk);
            return;
        }
        let had_read = self.read.is_some();
        let rest = self.give_read(chunk);
        if had_read && rest.is_empty() {
            return;
        }
        if self.has_watches() {
            self.give_watches(rest);
        } else {
            self.give_idle(rest);
        }
    }

    fn race_feed(&mut self, chunk: &[u8]) {
        // A drain installed in the gap takes the chunk: left in idle it would reach the
        // next exchange as a stale reply.
        if let Some(d) = self.drain.as_mut() {
            d.extend_from_slice(chunk);
            return;
        }
        if let Some((w, _)) = self.waiter.as_mut() {
            w.extend_from_slice(chunk);
            return;
        }
        let rest = self.give_read(chunk);
        self.give_idle(rest);
    }

    fn start_read(&mut self, max: usize, fill: bool) {
        if self.has_watches() {
            self.reads_done.push(Err(
                "Cannot read while watch is active; use watch or exchange".into(),
            ));
            return;
        }
        if self.read.is_some() {
            self.reads_done.push(Err("read already in progress".into()));
            return;
        }
        let n = max.min(self.idle.len());
        let buf: Vec<u8> = self.idle.drain(..n).collect();
        if (!fill && !buf.is_empty()) || buf.len() >= max {
            self.reads_done.push(Ok(buf));
        } else {
            self.read = Some(PendingRead { max, fill, buf });
        }
    }

    fn fail_all(&mut self, reason: &str) {
        if let Some((w, failed)) = self.waiter.take() {
            self.waiters_done
                .push((w, failed.unwrap_or_else(|| reason.into())));
        }
        if self.drain.take().is_some() {
            self.drains_done.push(Err(reason.into()));
        }
        if self.read.take().is_some() {
            self.reads_done.push(Err(reason.into()));
        }
    }
}

fn chunk(len: std::ops::Range<usize>) -> impl Strategy<Value = Vec<u8>> {
    // No newline, CR or '+': no line ever completes, so no URC is split off and the
    // test waiter (completes on "\n") never finishes on its own.
    proptest::collection::vec(b'a'..=b'h', len)
}

fn step_strategy() -> impl Strategy<Value = Step> {
    prop_oneof![
            6 => chunk(0..9).prop_map(Step::Feed),
            2 => chunk(1..9).prop_map(Step::RaceFeed),
            1 => Just(Step::SetWaiter),
            1 => Just(Step::ClearWaiter),
            1 => Just(Step::CancelExchange),
            1 => Just(Step::TakeIdle),
            2 => (1usize..8, any::<bool>()).prop_map(|(max, fill)| Step::StartRead { max, fill }),
            1 => Just(Step::ExpireRead),
            1 => Just(Step::CancelRead),
            1 => Just(Step::StartDrain),
            1 => Just(Step::EndDrain),
            1 => Just(Step::AttachWatch),
            1 => Just(Step::DetachWatch),
            1 => Just(Step::AttachCore),
            1 => (0usize..4).prop_map(Step::DetachCore),
            1 => Just(Step::Purge),
            1 => Just(Step::FailAll),
    ]
}

impl Model {
    /// Whether `step` can happen in this state (else the runner skips it).
    fn observable(&self, step: &Step) -> bool {
        let m = self;
        match step {
            Step::RaceFeed(_) => !m.has_watches(),
            Step::SetWaiter => m.waiter.is_none(),
            Step::ClearWaiter => m.waiter.is_some(),
            Step::ExpireRead => m.read.is_some(),
            Step::EndDrain => m.drain.is_some(),
            Step::AttachWatch => m.watch.is_none(),
            Step::DetachWatch => m.watch.is_some(),
            Step::DetachCore(_) => !m.cores.is_empty(),
            _ => true,
        }
    }

    fn apply(&mut self, step: &Step) {
        let m = self;
        match step {
            Step::Feed(c) => m.feed(c),
            Step::RaceFeed(c) => m.race_feed(c),
            Step::SetWaiter => m.waiter = Some((std::mem::take(&mut m.idle), None)),
            Step::ClearWaiter => {
                let (w, failed) = m.waiter.take().unwrap();
                let error = failed.unwrap_or_else(|| "exchange timed out after 0 ms".into());
                m.waiters_done.push((w, error));
            }
            Step::CancelExchange => {
                if let Some((_, failed)) = m.waiter.as_mut() {
                    failed.get_or_insert_with(|| "exchange cancelled".into());
                }
            }
            Step::TakeIdle => {
                let idle = std::mem::take(&mut m.idle);
                m.taken.push(idle);
            }
            Step::StartRead { max, fill } => m.start_read(*max, *fill),
            Step::ExpireRead => {
                let r = m.read.take().unwrap();
                m.reads_done.push(if r.buf.is_empty() {
                    Err(format!("no data received within {READ_MS} ms"))
                } else {
                    Ok(r.buf)
                });
            }
            Step::CancelRead => {
                if m.read.take().is_some() {
                    m.reads_done.push(Err("read cancelled".into()));
                }
            }
            Step::StartDrain if m.drain.is_some() => {
                m.drains_done.push(Err("drain already in progress".into()))
            }
            Step::StartDrain => {
                // Idle bytes are stale to a drain: dropped as its slot goes in.
                m.idle.clear();
                m.drain = Some(Vec::new());
            }
            Step::EndDrain => {
                let d = m.drain.take().unwrap();
                m.drains_done.push(Ok(d));
            }
            Step::AttachWatch => {
                m.idle.clear();
                m.watch = Some(m.watch_got.len());
                m.watch_got.push(Vec::new());
            }
            Step::DetachWatch => m.watch = None,
            Step::AttachCore => {
                m.idle.clear();
                m.next_core += 1;
                m.cores.push(m.next_core);
                m.core_got.insert(m.next_core, Vec::new());
            }
            Step::DetachCore(i) => {
                let i = i % m.cores.len();
                m.cores.remove(i);
            }
            Step::Purge => {
                m.idle.clear();
                if let Some(r) = m.read.as_mut() {
                    r.buf.clear();
                }
            }
            Step::FailAll => m.fail_all("gone"),
        }
    }
}

type Bytes = Arc<Mutex<Vec<u8>>>;
type Results = Receiver<Result<Vec<u8>, String>>;

pub(super) struct Sut {
    shared: Arc<RxHubShared>,
    routing: HubRoutingState,
    watches: Vec<Bytes>,
    cores: BTreeMap<u64, Bytes>,
    /// Attached core watch ids, in the model's order.
    attached: Vec<u64>,
    waiter: Option<Arc<ExchangeWaiter>>,
    waiters_done: Vec<(Vec<u8>, String)>,
    taken: Vec<Vec<u8>>,
    reads: (SyncSender<Result<Vec<u8>, String>>, Results),
    reads_done: Vec<Result<Vec<u8>, String>>,
    drains: (SyncSender<Result<Vec<u8>, String>>, Results),
    drains_done: Vec<Result<Vec<u8>, String>>,
    threads: Vec<JoinHandle<()>>,
}

/// A sink that records `Data` bytes; any other event fails the test.
fn recorder() -> (EventSink, Bytes) {
    let got: Bytes = Arc::default();
    let sink_got = got.clone();
    let sink: EventSink = Arc::new(move |ev| {
        match ev {
            SerialEvent::Data { data, .. } => sink_got.lock().unwrap().extend(data),
            other => panic!("unexpected event {other:?}"),
        }
        true
    });
    (sink, got)
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::yield_now();
    }
}

/// Results with errors as their kind (the hub's errors are text).
fn kinds(r: &[Result<Vec<u8>, String>]) -> Vec<Result<Vec<u8>, ErrorKind>> {
    r.iter()
        .map(|r| r.clone().map_err(|e| ErrorKind::from_message(&e)))
        .collect()
}

fn kinds_w(w: &[(Vec<u8>, String)]) -> Vec<(Vec<u8>, ErrorKind)> {
    w.iter()
        .map(|(b, e)| (b.clone(), ErrorKind::from_message(e)))
        .collect()
}

/// A cleared waiter's bytes and its `wait` error (it never completes: no "\n").
fn finished(w: &Arc<ExchangeWaiter>) -> (Vec<u8>, String) {
    let bytes = w.buffer.lock().unwrap().0.clone();
    (bytes, w.wait(0).expect_err("waiter completed"))
}

fn past() -> Instant {
    Instant::now()
        .checked_sub(Duration::from_millis(1))
        .unwrap_or_else(Instant::now)
}

impl Sut {
    /// Route the hub's queued events and run its timers (batch 0: every tick flushes).
    fn settle(&mut self) {
        self.shared
            .dispatch_queued_events(std::mem::take(&mut self.routing.pending_events));
        self.shared.tick(PATH, &mut self.routing);
        self.shared
            .dispatch_queued_events(std::mem::take(&mut self.routing.pending_events));
    }

    fn collect(rx: &Results, done: &mut Vec<Result<Vec<u8>, String>>, want: usize) {
        while done.len() < want {
            done.push(
                rx.recv_timeout(Duration::from_secs(3))
                    .expect("a result the model expects"),
            );
        }
        if let Ok(extra) = rx.try_recv() {
            panic!("unexpected result {extra:?}");
        }
    }
}

impl Sut {
    fn new() -> Sut {
        Sut {
            shared: Arc::new(RxHubShared::for_path(PATH.into())),
            routing: HubRoutingState::new(PATH.into()),
            watches: Vec::new(),
            cores: BTreeMap::new(),
            attached: Vec::new(),
            waiter: None,
            waiters_done: Vec::new(),
            taken: Vec::new(),
            reads: mpsc::sync_channel(64),
            reads_done: Vec::new(),
            drains: mpsc::sync_channel(64),
            drains_done: Vec::new(),
            threads: Vec::new(),
        }
    }

    fn apply(&mut self, m: &Model, step: Step) {
        let sut = self;
        let shared = sut.shared.clone();
        match step {
            Step::Feed(c) => shared.feed_bytes(&c, &mut sut.routing),
            Step::RaceFeed(c) => push_idle(&shared, &c),
            Step::SetWaiter => {
                let options = ExchangeOptions {
                    completion_mode: Some(ExchangeCompletionMode::Substring),
                    terminators: Some(vec!["\n".into()]),
                    ..Default::default()
                }
                .resolve();
                let w = ExchangeWaiter::new(options, Arc::new(AtomicBool::new(false)));
                shared.set_exchange_waiter(w.clone());
                sut.waiter = Some(w);
            }
            Step::ClearWaiter => {
                shared.clear_exchange_waiter();
                let w = sut.waiter.take().unwrap();
                sut.waiters_done.push(finished(&w));
            }
            Step::CancelExchange => shared.cancel_active_exchange(),
            Step::TakeIdle => sut.taken.push(shared.take_idle_bytes()),
            Step::StartRead { max, fill } => {
                let tx = sut.reads.0.clone();
                sut.threads.push(std::thread::spawn(move || {
                    let _ = tx.send(shared.read_request(max, READ_MS, fill));
                }));
                if m.read.is_some() {
                    // The model installed a slot (a busy read leaves the old one).
                    let s = sut.shared.clone();
                    wait_for("read slot", || s.read_slot.lock().unwrap().is_some());
                }
            }
            Step::ExpireRead => {
                if let Some(slot) = shared.read_slot.lock().unwrap().as_mut() {
                    slot.deadline = past();
                }
            }
            Step::CancelRead => shared.cancel_pending_read(),
            Step::StartDrain => {
                let tx = sut.drains.0.clone();
                let s = shared.clone();
                sut.threads.push(std::thread::spawn(move || {
                    let cancel = Arc::new(AtomicBool::new(false));
                    let _ = tx.send(s.drain(60_000, 60_000, cancel, Vec::new()));
                }));
                wait_for("drain slot", || shared.drain.lock().unwrap().is_some());
            }
            Step::EndDrain => {
                if let Some(d) = shared.drain.lock().unwrap().as_mut() {
                    d.deadline = past();
                }
            }
            Step::AttachWatch => {
                let (sink, got) = recorder();
                shared.attach_watch_with(sink, 0, 1024, false, None);
                sut.watches.push(got);
            }
            Step::DetachWatch => shared.detach_watch(),
            Step::AttachCore => {
                let (sink, got) = recorder();
                let id = m.next_core;
                let alive = Arc::new(AtomicBool::new(true));
                shared.attach_core_watch(CoreWatchSlot::new(id, sink, alive, 0, 1024, false, None));
                sut.cores.insert(id, got);
                sut.attached.push(id);
            }
            Step::DetachCore(i) => {
                let i = i % sut.attached.len();
                assert!(shared.detach_core_watch(sut.attached.remove(i)));
            }
            Step::Purge => shared.purge_buffers(),
            Step::FailAll => {
                shared.fail_all_waiters("gone");
                if let Some(w) = sut.waiter.take() {
                    sut.waiters_done.push(finished(&w));
                }
            }
        }
        sut.settle();
        Sut::collect(&sut.reads.1, &mut sut.reads_done, m.reads_done.len());
        Sut::collect(&sut.drains.1, &mut sut.drains_done, m.drains_done.len());
    }

    fn check(&self, m: &Model) {
        let sut = self;
        let shared = &sut.shared;
        for (i, got) in sut.watches.iter().enumerate() {
            assert_eq!(*got.lock().unwrap(), m.watch_got[i], "old watch #{i}");
        }
        for (id, got) in &sut.cores {
            assert_eq!(*got.lock().unwrap(), m.core_got[id], "core watch {id}");
        }
        assert_eq!(
            kinds_w(&sut.waiters_done),
            kinds_w(&m.waiters_done),
            "finished waiters"
        );
        let waiter = sut
            .waiter
            .as_ref()
            .map(|w| w.buffer.lock().unwrap().0.clone());
        let want = m.waiter.as_ref().map(|(w, _)| w.clone());
        assert_eq!(waiter, want, "current waiter");
        assert_eq!(sut.taken, m.taken, "taken idle bytes");
        assert_eq!(kinds(&sut.reads_done), kinds(&m.reads_done), "read results");
        assert_eq!(
            kinds(&sut.drains_done),
            kinds(&m.drains_done),
            "drain results"
        );
        assert_eq!(*shared.idle.lock().unwrap(), m.idle, "idle buffer");
        let slot = shared
            .read_slot
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| s.buffer.clone());
        assert_eq!(slot, m.read.as_ref().map(|r| r.buf.clone()), "read slot");
        assert_eq!(
            shared.buffered_len(),
            m.idle.len() + m.read.as_ref().map_or(0, |r| r.buf.len())
        );
        assert_eq!(shared.drain.lock().unwrap().is_some(), m.drain.is_some());
        let drained = shared
            .drain
            .lock()
            .unwrap()
            .as_ref()
            .map(|d| d.buffer.clone());
        assert_eq!(drained, m.drain, "drain buffer");
    }

    fn teardown(mut self) {
        let sut = &mut self;
        sut.shared.fail_all_waiters("teardown");
        for t in sut.threads.drain(..) {
            t.join().unwrap();
        }
    }
}

/// Random routing steps against the model.
#[test]
fn hub_routing_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 64,
        failure_persistence: None,
        ..Config::default()
    });
    let result = runner.run(&vec(step_strategy(), 1..40), |steps| {
        let mut model = Model::default();
        let mut sut = Sut::new();
        for step in steps {
            if !model.observable(&step) {
                continue;
            }
            model.apply(&step);
            sut.apply(&model, step);
            sut.check(&model);
        }
        sut.teardown();
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}
