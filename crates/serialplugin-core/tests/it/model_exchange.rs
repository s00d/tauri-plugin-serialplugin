//! Model test of one port in use: lines and URCs cut into random reads, exchanges (each
//! RX preparation, completion mode and result format; answered, cancelled, timed out, cut
//! off by the device or queued behind another), reads, watches and `watch_core` streams
//! with and without a parser or `route_urc`, unplug and reopen.
//!
//! The model knows who gets each byte (exchange > watches > idle buffer), what a stream
//! makes of it (raw bytes, or lines and URCs), and what the exchange answers.

use crate::common::*;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serialplugin_core::at::parse::{AtParseStatus, ExchangeMatch, ExchangeResponse};
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{
    AtResultFormat, CoreEvent, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode, SerialEvent,
    SubscriptionId,
};
use serialplugin_core::hub::ExchangeWaiter;
use serialplugin_core::SerialManager;
use std::io;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const PATH: &str = "port";
const COMMAND: &str = "AT+CSQ";
const URCS: [&str; 4] = ["+CREG: 1,1", "^CARDLOCK: 1", "RING", "+CMTI: \"SM\",3"];

/// What a stream makes of its bytes.
#[derive(Clone, Copy, Debug, PartialEq)]
enum View {
    /// Batches with no window: bytes as they come.
    Raw,
    /// A batch whose window never ends: everything comes when the stream ends.
    Batched,
    /// A `readline` parser behind a window that never ends, so frames go out at once.
    Lines,
    /// The same with `route_urc`.
    Urcs,
}

#[derive(Clone, Debug)]
enum Line {
    Text(String),
    Urc(usize),
}

impl Line {
    fn text(&self) -> &str {
        match self {
            Line::Text(t) => t,
            Line::Urc(i) => URCS[*i],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum End {
    Ok,
    Error,
    Cme,
    Cancel,
    Unplug,
    /// Nothing ever comes back.
    Silence,
}

#[derive(Clone, Debug)]
struct Call {
    prepare: RxPrepareMode,
    substring: bool,
    numeric: bool,
    /// Reply lines before the final result: a reading, or a URC in between.
    body: Vec<Result<u8, usize>>,
    end: End,
    cuts: Vec<usize>,
}

#[derive(Clone, Debug)]
enum Action {
    Open,
    /// Whole lines, cut into reads.
    Rx(Vec<Line>, Vec<usize>),
    /// An unfinished line.
    Tail(String),
    Watch(View),
    Unwatch,
    WatchCore(View),
    UnwatchCore(usize),
    Read(usize),
    Exchange(Call),
    /// Two exchanges at once: the second waits for the first.
    Pair(Call, Call),
    Unplug,
    Close,
}

/// What an exchange answers: raw bytes, status, body and URC lines (`None`: not predicted,
/// stale bytes came first).
type Answer = Result<(Vec<u8>, AtParseStatus, Option<(Vec<String>, Vec<String>)>), ErrorKind>;

impl Call {
    /// The reply up to the final result, and the final result.
    fn reply(&self) -> (Vec<u8>, Vec<u8>) {
        let mut body = Vec::new();
        for line in self.lines() {
            body.extend(format!("\r\n{line}\r\n").bytes());
        }
        let end = match (self.end, self.numeric) {
            (End::Ok, true) => "0\r",
            (End::Error, true) => "4\r",
            (End::Ok, false) => "\r\nOK\r\n",
            (End::Error, false) => "\r\nERROR\r\n",
            (End::Cme, _) => "\r\n+CME ERROR: 10\r\n",
            _ => "",
        };
        if self.end == End::Silence {
            body.clear();
        }
        (body, end.as_bytes().to_vec())
    }

    fn lines(&self) -> Vec<String> {
        self.body
            .iter()
            .map(|l| match l {
                Ok(n) => format!("+CSQ: {n},99"),
                Err(urc) => URCS[*urc].to_string(),
            })
            .collect()
    }

    fn urcs(&self) -> Vec<String> {
        let urcs = self.body.iter().filter_map(|l| l.err());
        match self.end {
            End::Silence => Vec::new(),
            _ => urcs.map(|i| URCS[i].to_string()).collect(),
        }
    }

    fn options(&self) -> ExchangeOptions {
        ExchangeOptions {
            timeout_ms: Some(if self.end == End::Silence {
                20
            } else {
                LONG_MS
            }),
            rx_prepare: Some(self.prepare.clone()),
            drain_idle_ms: Some(1),
            completion_mode: self.substring.then_some(ExchangeCompletionMode::Substring),
            result_format: self.numeric.then_some(AtResultFormat::Numeric),
            ..Default::default()
        }
    }

    fn answer(&self, stale: Vec<u8>) -> Answer {
        let status = match self.end {
            End::Ok => AtParseStatus::Ok,
            End::Error => AtParseStatus::Error,
            End::Cme => AtParseStatus::Cme,
            End::Cancel => return Err(ErrorKind::Cancelled),
            End::Unplug => return Err(ErrorKind::Disconnected),
            End::Silence => return Err(ErrorKind::Timeout),
        };
        let lines = stale.is_empty().then(|| {
            let readings = self.body.iter().filter_map(|l| l.ok());
            (
                readings.map(|n| format!("+CSQ: {n},99")).collect(),
                self.urcs(),
            )
        });
        let (body, end) = self.reply();
        Ok(([stale, body, end].concat(), status, lines))
    }
}

/// What a stream got.
#[derive(Clone, Debug, Default, PartialEq)]
struct Got {
    raw: Vec<u8>,
    frames: Vec<Vec<u8>>,
    urcs: Vec<String>,
    disconnects: usize,
}

#[derive(Clone, Debug)]
struct Stream {
    view: View,
    attached: bool,
    partial: Vec<u8>,
    got: Got,
}

impl Stream {
    fn new(view: View) -> Self {
        Self {
            view,
            attached: true,
            partial: Vec::new(),
            got: Got::default(),
        }
    }

    fn feed(&mut self, bytes: &[u8]) {
        if self.view == View::Raw {
            self.got.raw.extend(bytes);
            return;
        }
        self.partial.extend(bytes);
        if self.view == View::Batched {
            return;
        }
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let mut line: Vec<u8> = self.partial.drain(..=end).collect();
            line.pop();
            let text = String::from_utf8(line.clone()).unwrap().trim().to_string();
            if self.view == View::Urcs && URCS.contains(&text.as_str()) {
                self.got.urcs.push(text);
            } else {
                self.got.frames.push(line);
            }
        }
    }

    fn detach(&mut self) {
        self.attached = false;
        let rest = std::mem::take(&mut self.partial);
        if self.view == View::Batched {
            self.got.raw.extend(rest);
        } else if !rest.is_empty() {
            self.got.frames.push(rest);
        }
    }
}

#[derive(Debug, Default)]
struct Model {
    open: bool,
    /// Unplugged and not reopened.
    gone: bool,
    idle: Vec<u8>,
    /// Old watches, the last one may be attached.
    watches: Vec<Stream>,
    cores: Vec<Stream>,
    written: Vec<u8>,
    answers: Vec<Answer>,
    reads: Vec<Result<Vec<u8>, ErrorKind>>,
}

impl Model {
    fn attached(&mut self) -> impl Iterator<Item = &mut Stream> {
        let all = self.watches.iter_mut().chain(&mut self.cores);
        all.filter(|s| s.attached)
    }

    fn rx(&mut self, bytes: &[u8]) {
        let mut taken = false;
        for s in self.attached() {
            s.feed(bytes);
            taken = true;
        }
        if !taken {
            self.idle.extend(bytes);
        }
    }

    fn not_open(&self) -> ErrorKind {
        if self.gone {
            ErrorKind::Disconnected
        } else {
            ErrorKind::NotFound
        }
    }

    fn unplug(&mut self) {
        for s in self.attached() {
            s.detach();
            s.got.disconnects += 1;
        }
        self.open = false;
        self.gone = true;
        self.idle.clear();
    }

    fn exchange(&mut self, call: &Call) {
        if !self.open {
            let e = self.not_open();
            self.answers.push(Err(e));
            return;
        }
        // Drained and purged bytes are gone; else they come first.
        let mut stale = std::mem::take(&mut self.idle);
        if call.prepare != RxPrepareMode::None {
            stale.clear();
        }
        self.written.extend(format!("{COMMAND}\r").bytes());
        for watch in self.watches.iter_mut().filter(|s| s.attached) {
            watch.got.urcs.extend(call.urcs());
        }
        for core in self.cores.iter_mut().filter(|s| s.attached) {
            if core.view == View::Urcs {
                core.got.urcs.extend(call.urcs());
            }
        }
        self.answers.push(call.answer(stale));
        if call.end == End::Unplug {
            self.unplug();
        }
    }

    /// A numeric result has no line start of its own: behind an unfinished stale line it
    /// is part of that line.
    fn glued(&self, call: &Call) -> bool {
        call.numeric
            && call.body.is_empty()
            && call.prepare == RxPrepareMode::None
            && self.idle.last().is_some_and(|b| *b != b'\n')
    }

    /// `false`: the step shows nothing now and is skipped.
    fn step(&mut self, a: &Action) -> bool {
        match a {
            Action::Exchange(call) | Action::Pair(call, _) if self.glued(call) => return false,
            Action::Open => {
                if self.open {
                    return false;
                }
                self.open = true;
                self.gone = false;
                self.written.clear();
                // A watch on the dead port stays registered until the port is reopened.
                self.watches.iter_mut().for_each(|w| w.attached = false);
            }
            Action::Rx(lines, _) => {
                if !self.open {
                    return false;
                }
                let text: String = lines.iter().map(|l| format!("{}\r\n", l.text())).collect();
                self.rx(text.as_bytes());
            }
            Action::Tail(text) => {
                if !self.open {
                    return false;
                }
                self.rx(text.as_bytes());
            }
            Action::Watch(view) => {
                if !self.open || self.watches.iter().any(|w| w.attached) {
                    return false;
                }
                self.idle.clear();
                self.watches.push(Stream::new(*view));
            }
            Action::Unwatch => match self.watches.iter_mut().find(|w| w.attached) {
                Some(w) if self.open => w.detach(),
                _ => return false,
            },
            Action::WatchCore(view) => {
                if !self.open || self.cores.len() > 3 {
                    return false;
                }
                self.idle.clear();
                self.cores.push(Stream::new(*view));
            }
            Action::UnwatchCore(i) => {
                let n = self.cores.len().max(1);
                match self.cores.get_mut(i % n) {
                    Some(s) if s.attached => s.detach(),
                    _ => return false,
                }
            }
            Action::Read(max) => {
                let watched = self.attached().next().is_some();
                // An empty read waits for its time: the manager model does that.
                if self.open && !watched && self.idle.is_empty() {
                    return false;
                }
                let result = if !self.open {
                    Err(self.not_open())
                } else if watched {
                    Err(ErrorKind::Other)
                } else {
                    let n = (*max).min(self.idle.len());
                    Ok(self.idle.drain(..n).collect())
                };
                self.reads.push(result);
            }
            Action::Exchange(call) => self.exchange(call),
            Action::Pair(a, b) => {
                let answered = |c: &Call| matches!(c.end, End::Ok | End::Error | End::Cme);
                if !self.open || !answered(a) || !answered(b) {
                    return false;
                }
                self.exchange(a);
                self.exchange(b);
            }
            Action::Unplug => {
                if !self.open {
                    return false;
                }
                self.unplug();
            }
            Action::Close => {
                if !self.open {
                    return false;
                }
                self.attached().for_each(Stream::detach);
                self.open = false;
                self.idle.clear();
            }
        }
        true
    }
}

type Shared = Arc<Mutex<Got>>;

struct Sut {
    m: SerialManager,
    device: FakeDevice,
    watches: Vec<(Option<u32>, Shared)>,
    /// What an old watch had when `unwatch` or `close` returned.
    ended: Vec<(usize, Got)>,
    cores: Vec<(SubscriptionId, Shared)>,
    answers: Vec<Answer>,
    reads: Vec<Result<Vec<u8>, ErrorKind>>,
}

impl Drop for Sut {
    fn drop(&mut self) {
        let _ = self.m.close_all();
    }
}

/// Records events the way `view` shows them.
fn recorder(view: View) -> (impl Fn(SerialEvent) -> bool + Send + Sync, Shared) {
    let got = Shared::default();
    let sink_got = got.clone();
    let sink = move |event| {
        let mut got = sink_got.lock().unwrap();
        match event {
            SerialEvent::Data { data, .. } => match view {
                View::Raw | View::Batched => got.raw.extend(data),
                View::Lines | View::Urcs => got.frames.push(data),
            },
            SerialEvent::Urc { line, .. } => got.urcs.push(line),
            SerialEvent::Disconnect { .. } => got.disconnects += 1,
            SerialEvent::Error { message, .. } => panic!("unexpected error event {message}"),
        }
        true
    };
    (sink, got)
}

fn view_options(view: View) -> serialplugin_core::events::WatchOptions {
    match view {
        View::Raw => watch_opts(0, false),
        // Nor does it fill up.
        View::Batched => serialplugin_core::events::WatchOptions {
            size: Some(1 << 16),
            ..watch_opts(NEVER, false)
        },
        View::Lines => watch_opts(NEVER, false),
        View::Urcs => watch_opts(NEVER, true),
    }
}

fn view_parser(view: View) -> Option<serial_codec::framing::FramerConfig> {
    matches!(view, View::Lines | View::Urcs).then(|| readline().unwrap())
}

type Running = JoinHandle<Result<ExchangeResponse, ErrorKind>>;

impl Sut {
    /// `cancel_read` does nothing but fail on a path that is not open.
    fn is_open(&self) -> bool {
        self.m.cancel_read(PATH.into()).is_ok()
    }

    /// Send `bytes` as reads cut at `cuts`.
    fn send(&self, bytes: &[u8], cuts: &[usize]) {
        let mut rest = bytes;
        for cut in cuts.iter().chain([&usize::MAX]) {
            let (head, tail) = rest.split_at((*cut).min(rest.len()));
            self.device.send(head);
            self.device.wait_read();
            rest = tail;
        }
    }

    /// The old watch, if any, has got its last event.
    fn note_end(&mut self) {
        if let Some((_, got)) = self.watches.last() {
            let got = got.lock().unwrap().clone();
            self.ended.push((self.watches.len() - 1, got));
        }
    }

    fn start(&self, call: &Call) -> Running {
        let options = call.options();
        background(&self.m, move |m| {
            m.exchange(PATH.into(), format!("{COMMAND}\r"), options)
                .map_err(kind)
        })
    }

    /// Wait for the `n`th command, answer it, and take the result.
    fn answer(&mut self, call: &Call, running: Running, n: usize) {
        let want = format!("{COMMAND}\r").len() * n;
        eventually("the command, or the end of its exchange", || {
            self.device.written().len() >= want || running.is_finished()
        });
        if self.device.written().len() < want {
            panic!("ended before its command: {:?}", running.join().unwrap());
        }
        assert_eq!(self.device.written().len(), want, "one command at a time");
        let (body, end) = call.reply();
        match call.end {
            End::Cancel => {
                self.send(&body, &call.cuts);
                self.device.wait_handled();
                self.m.cancel_exchange(PATH.into()).unwrap();
            }
            End::Unplug => {
                self.send(&body, &call.cuts);
                self.device.wait_handled();
                self.device.fail_reads(io::ErrorKind::BrokenPipe);
            }
            End::Silence => {}
            _ => self.send(&[body, end].concat(), &call.cuts),
        }
        self.take(call, running);
    }

    fn take(&mut self, call: &Call, running: Running) {
        let result = running.join().unwrap();
        if let Ok(r) = &result {
            match call.end {
                _ if call.substring => {
                    assert!(matches!(r.matched, ExchangeMatch::Substring { .. }))
                }
                End::Ok => assert_eq!(r.matched, ExchangeMatch::Ok),
                End::Error => assert_eq!(r.matched, ExchangeMatch::Error),
                _ => assert_eq!(r.matched, ExchangeMatch::CmeError { code: Some(10) }),
            }
        }
        self.answers
            .push(result.map(|r| (r.raw, r.status, Some((r.solicited_body, r.urc_lines)))));
    }

    fn step(&mut self, a: &Action, sent: usize) {
        let m = self.m.clone();
        match a {
            Action::Open => {
                self.device = FakeDevice::new();
                m.insert_port(PATH.into(), Box::new(self.device.port()))
                    .unwrap();
            }
            Action::Rx(lines, cuts) => {
                let text: String = lines.iter().map(|l| format!("{}\r\n", l.text())).collect();
                self.send(text.as_bytes(), cuts);
                self.device.wait_handled();
            }
            Action::Tail(text) => {
                self.device.send(text.as_bytes());
                self.device.wait_handled();
            }
            Action::Watch(view) => {
                let (sink, got) = recorder(*view);
                let id = m
                    .watch_framed(
                        PATH.into(),
                        view_options(*view),
                        view_parser(*view),
                        Arc::new(sink),
                    )
                    .unwrap();
                self.watches.push((Some(id), got));
            }
            Action::Unwatch => {
                let id = self.watches.last_mut().unwrap().0.take().unwrap();
                m.unwatch(id).unwrap();
                self.note_end();
            }
            Action::WatchCore(view) => {
                let (sink, got) = recorder(*view);
                let sink = Arc::new(move |e| match e {
                    CoreEvent::Event(e) => sink(e),
                    other => panic!("unexpected {other:?}"),
                });
                let id = m
                    .watch_core(PATH.into(), view_options(*view), view_parser(*view), sink)
                    .unwrap();
                self.cores.push((id, got));
            }
            Action::UnwatchCore(i) => {
                let i = i % self.cores.len();
                m.unwatch_core(self.cores[i].0).unwrap();
            }
            Action::Read(max) => {
                let r = m.read(PATH.into(), Some(LONG_MS), Some(*max));
                self.reads
                    .push(r.map(String::into_bytes).map_err(crate::common::kind));
            }
            Action::Exchange(call) => {
                let running = self.start(call);
                if self.is_open() {
                    self.answer(call, running, sent + 1);
                } else {
                    self.take(call, running);
                }
            }
            Action::Pair(a, b) => {
                let first = self.start(a);
                let want = format!("{COMMAND}\r").len() * (sent + 1);
                self.device.wait_written("the command", |w| w.len() >= want);
                let second = self.start(b);
                self.answer(a, first, sent + 1);
                self.answer(b, second, sent + 2);
            }
            Action::Unplug => self.device.fail_reads(io::ErrorKind::BrokenPipe),
            Action::Close => {
                m.close(PATH.into()).unwrap();
                self.note_end();
            }
        }
    }

    fn check(&self, model: &Model) {
        let streams = || {
            let watches = self.watches.iter().map(|(_, g)| g);
            let got = watches.chain(self.cores.iter().map(|(_, g)| g));
            let want = model.watches.iter().chain(&model.cores);
            got.zip(want)
        };
        settle(|| {
            streams().all(|(got, want)| {
                let got = got.lock().unwrap();
                got.raw.len() >= want.got.raw.len()
                    && got.frames.len() >= want.got.frames.len()
                    && got.urcs.len() >= want.got.urcs.len()
                    && got.disconnects >= want.got.disconnects
            }) && self.is_open() == model.open
        });
        assert_eq!(self.is_open(), model.open);
        for (got, want) in streams() {
            assert_eq!(*got.lock().unwrap(), want.got, "{:?}", want.view);
        }
        for (i, got) in &self.ended {
            assert_eq!(*got, model.watches[*i].got, "watch {i} as it ended");
        }
        assert_eq!(self.answers.len(), model.answers.len());
        for (got, want) in self.answers.iter().zip(&model.answers) {
            match (got, want) {
                // With stale bytes in front only the bytes and the status are the rule.
                (Ok(got), Ok((raw, status, None))) => assert_eq!((&got.0, &got.1), (raw, status)),
                (got, want) => assert_eq!(got, want),
            }
        }
        assert_eq!(self.reads, model.reads);
        if model.open {
            assert_eq!(self.device.written(), model.written);
            assert_eq!(
                self.m.bytes_to_read(PATH.into()).unwrap() as usize,
                model.idle.len()
            );
        }
    }
}

fn view() -> impl Strategy<Value = View> {
    prop::sample::select(vec![View::Raw, View::Batched, View::Lines, View::Urcs])
}

fn cuts() -> impl Strategy<Value = Vec<usize>> {
    vec(1usize..12, 0..6)
}

fn line() -> impl Strategy<Value = Line> {
    prop_oneof![
        "[a-h]{1,6}".prop_map(Line::Text),
        (0..URCS.len()).prop_map(Line::Urc)
    ]
}

fn call() -> impl Strategy<Value = Call> {
    let prepare = prop::sample::select(vec![
        RxPrepareMode::None,
        RxPrepareMode::Purge,
        RxPrepareMode::Drain,
    ]);
    let end = prop::sample::select(vec![
        End::Ok,
        End::Ok,
        End::Ok,
        End::Error,
        End::Cme,
        End::Cancel,
        End::Unplug,
        End::Silence,
    ]);
    let body = vec(
        prop_oneof![(0u8..32).prop_map(Ok), (0..URCS.len()).prop_map(Err)],
        0..4,
    );
    (prepare, 0..3, body, end, cuts()).prop_map(|(prepare, mode, body, end, cuts)| {
        // CME answers have no numeric form and no default terminator.
        let plain = end == End::Cme;
        Call {
            prepare,
            substring: mode == 1 && !plain,
            numeric: mode == 2 && !plain,
            body,
            end,
            cuts,
        }
    })
}

fn action() -> impl Strategy<Value = Action> {
    prop_oneof![
        3 => Just(Action::Open),
        6 => (vec(line(), 1..4), cuts()).prop_map(|(l, c)| Action::Rx(l, c)),
        2 => "[a-h]{1,5}".prop_map(Action::Tail),
        2 => view().prop_map(Action::Watch),
        2 => Just(Action::Unwatch),
        3 => view().prop_map(Action::WatchCore),
        1 => any::<usize>().prop_map(Action::UnwatchCore),
        2 => (1usize..12).prop_map(Action::Read),
        6 => call().prop_map(Action::Exchange),
        1 => (call(), call()).prop_map(|(a, b)| Action::Pair(a, b)),
        1 => Just(Action::Unplug),
        1 => Just(Action::Close),
    ]
}

#[test]
fn a_port_in_use_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 64,
        failure_persistence: None,
        max_shrink_time: 20_000,
        ..Config::default()
    });
    let result = runner.run(&vec(action(), 1..30), |actions| {
        let mut model = Model::default();
        let mut sut = Sut {
            m: SerialManager::new(),
            device: FakeDevice::new(),
            watches: Vec::new(),
            ended: Vec::new(),
            cores: Vec::new(),
            answers: Vec::new(),
            reads: Vec::new(),
        };
        for a in &actions {
            let sent = model.written.len() / format!("{COMMAND}\r").len();
            if model.step(a) {
                sut.step(a, sent);
                sut.check(&model);
            }
        }
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// Given a reply of random lines and a final result,
/// when a waiter gets it in one piece, and another gets it in random pieces,
/// then both complete with the same bytes and the same match, whatever comes after it.
#[test]
fn any_cut_of_a_reply_completes_the_same() {
    let mut runner = TestRunner::new(Config {
        cases: 128,
        failure_persistence: None,
        ..Config::default()
    });
    // Up to 64 phone-book sized lines; `a_long_reply_completes` has the long ones.
    let input = (call(), 0usize..64, vec(1usize..70, 1..40));
    let result = runner.run(&input, |(call, filler, cuts)| {
        let line = "\r\n+CPBR: 1,\"+10000000001\",145,\"Name\"\r\n";
        let (body, end) = call.reply();
        let reply = [line.repeat(filler).into_bytes(), body, end].concat();
        let waiter = || {
            let options = ExchangeOptions {
                command: Some(COMMAND.into()),
                max_bytes: Some(1 << 20),
                ..call.options()
            };
            ExchangeWaiter::new(options.resolve(), Arc::new(AtomicBool::new(false)))
        };
        let whole = waiter();
        whole.push_bytes(&reply);
        let pieces = waiter();
        let mut rest = &reply[..];
        for cut in cuts.iter().cycle() {
            let (head, tail) = rest.split_at((*cut).min(rest.len()));
            pieces.push_bytes(head);
            rest = tail;
            if rest.is_empty() {
                break;
            }
        }
        // The first result wins: a late failure changes nothing.
        let complete = matches!(call.end, End::Ok | End::Error | End::Cme);
        if complete {
            pieces.fail_with_reason("late failure".into());
        }
        let (whole, pieces) = (whole.wait(0), pieces.wait(0));
        prop_assert_eq!(whole.is_ok(), complete);
        prop_assert_eq!(&whole, &pieces);
        prop_assert!(whole.is_err() || whole.unwrap().0 == reply);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}

/// Given a reply of a mebibyte that comes in small reads,
/// when a waiter collects it, in each completion mode,
/// then it completes with every byte. (Completion looks at what is new only: looking at the
/// whole reply with each read would take minutes here, past the per-test limit.)
#[test]
fn a_long_reply_completes() {
    let line = b"\r\n+CPBR: 1,\"+10000000001\",145,\"Name\"\r\n";
    let modes = [
        (None, None, &b"\r\nOK\r\n"[..]),
        (None, Some(AtResultFormat::Numeric), b"0\r"),
        (Some(ExchangeCompletionMode::Substring), None, b"\r\nOK\r\n"),
    ];
    for (completion_mode, result_format, end) in modes {
        let reply = [&line.repeat((1 << 20) / line.len())[..], end].concat();
        let options = ExchangeOptions {
            command: Some("AT+CPBR=1,9999".into()),
            max_bytes: Some(2 << 20),
            completion_mode,
            result_format,
            ..Default::default()
        };
        let waiter = ExchangeWaiter::new(options.resolve(), Arc::new(AtomicBool::new(false)));
        for read in reply.chunks(64) {
            waiter.push_bytes(read);
        }
        assert_eq!(waiter.wait(0).unwrap().0.len(), reply.len());
    }
}
