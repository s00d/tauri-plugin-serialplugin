//! Model test of the multiplexer: `SerialManager` against a 27.010 modem. Random opens and
//! closes of channels (accepted, refused), DISC / DM and leaving from the
//! modem, flow control (FCoff / FCon, MSC FC per channel), control commands, data and
//! commands both ways, watches, unplug. The model predicts each result, what each watch
//! gets, the port states, and every frame the modem must have seen.

use crate::common::*;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::cmux::mux_path;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{CoreEvent, PortStatus, SerialEvent, SubscriptionId};
use serialplugin_core::SerialManager;
use std::collections::BTreeMap;
use std::io;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

const PATH: &str = "modem";
/// Channels 1..=3 answer; 4 refuses.
const GOOD: u8 = 3;
const REFUSED: u8 = 4;
/// T1 and T2 as long as 27.007 allows and no retransmission: the modem always answers.
const COMMAND: &str = "AT+CMUX=0,0,5,64,255,0,255";

#[derive(Clone, Copy, Debug)]
enum Leave {
    Disc,
    Dm,
    Cld,
}

#[derive(Clone, Debug)]
enum Action {
    Enable,
    Disable,
    Open(u8),
    Close(u8),
    Write(u8, Vec<u8>),
    /// Data from the modem.
    Data(u8, Vec<u8>),
    At(u8),
    Watch(u8, bool),
    Unwatch(u8),
    WatchCore(u8, bool),
    UnwatchCore(usize),
    /// DISC (or DM) from the modem on a channel.
    ModemCloses(u8, bool),
    ModemLeaves(Leave),
    FlowOff(bool),
    ChannelFlowOff(u8, bool),
    /// TEST, or a command nobody knows.
    Control(bool, Vec<u8>),
    Unplug,
}

#[derive(Clone, Debug, PartialEq)]
enum State {
    Opened,
    Closed,
    Disconnected,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct Got {
    data: Vec<u8>,
    /// `readline` frames (parser streams only).
    frames: Vec<Vec<u8>>,
    disconnects: usize,
}

#[derive(Clone, Debug)]
struct Stream {
    dlci: u8,
    framed: bool,
    /// Not framed: a batch whose window never ends.
    batched: bool,
    attached: bool,
    partial: Vec<u8>,
    got: Got,
}

impl Stream {
    fn feed(&mut self, bytes: &[u8]) {
        if !self.framed && !self.batched {
            self.got.data.extend(bytes);
            return;
        }
        self.partial.extend(bytes);
        if self.batched {
            return;
        }
        while let Some(end) = self.partial.iter().position(|b| *b == b'\n') {
            let line: Vec<u8> = self.partial.drain(..=end).collect();
            // `readline` has no empty lines.
            if end > 0 {
                self.got.frames.push(line[..end].to_vec());
            }
        }
    }

    fn finish(&mut self) {
        self.attached = false;
        let rest = std::mem::take(&mut self.partial);
        if self.batched {
            self.got.data.extend(rest);
        } else if !rest.is_empty() {
            self.got.frames.push(rest);
        }
    }
}

/// A frame the modem sees, as far as the model counts it.
type Seen = (u8, u8, Vec<u8>);

#[derive(Debug, Default)]
struct Model {
    unplugged: bool,
    enabled: bool,
    /// Channels open in the session.
    open: BTreeMap<u8, ()>,
    /// Channel paths the manager knows (open ones, and ones the modem closed).
    paths: BTreeMap<u8, ()>,
    flow_off: bool,
    channels_off: BTreeMap<u8, ()>,
    /// Writes waiting on flow control.
    waiting: BTreeMap<u8, Vec<u8>>,
    watches: Vec<Stream>,
    cores: Vec<Stream>,
    states: Vec<(u8, State)>,
    frames: Vec<Seen>,
    commands: Vec<(Option<u8>, String)>,
}

fn uih(dlci: u8, payload: &[u8]) -> Seen {
    (dlci, mux::UIH, payload.to_vec())
}

impl Model {
    fn streams(&mut self) -> impl Iterator<Item = &mut Stream> {
        self.watches.iter_mut().chain(&mut self.cores)
    }

    fn held(&self, dlci: u8) -> bool {
        self.flow_off || self.channels_off.contains_key(&dlci)
    }

    /// Writes that no longer wait go out.
    fn release(&mut self) {
        let waiting: Vec<u8> = self.waiting.keys().copied().collect();
        let free: Vec<u8> = waiting.into_iter().filter(|d| !self.held(*d)).collect();
        for dlci in free {
            let bytes = self.waiting.remove(&dlci).unwrap();
            self.frames.push(uih(dlci, &bytes));
        }
    }

    /// A channel of the session ends; its streams with a `Disconnect` when the other side
    /// ended it.
    fn end_channel(&mut self, dlci: u8, disconnect: bool) {
        self.open.remove(&dlci);
        self.waiting.remove(&dlci);
        // A reopened channel starts with flow control on.
        self.channels_off.remove(&dlci);
        for s in self.streams().filter(|s| s.dlci == dlci && s.attached) {
            s.finish();
            s.got.disconnects += usize::from(disconnect);
        }
    }

    fn end_session(&mut self) {
        self.enabled = false;
        self.flow_off = false;
        self.channels_off.clear();
    }

    /// The host closes a channel path.
    fn close(&mut self, dlci: u8) {
        self.paths.remove(&dlci);
        if self.open.contains_key(&dlci) {
            self.end_channel(dlci, false);
            self.frames.push((dlci, mux::DISC, Vec::new()));
        }
        self.states.push((dlci, State::Closed));
    }

    fn write(&mut self, dlci: u8, bytes: &[u8]) -> Result<(), ErrorKind> {
        if !self.paths.contains_key(&dlci) {
            return Err(ErrorKind::NotFound);
        }
        if !self.enabled {
            return Err(ErrorKind::NotFound);
        }
        if !self.open.contains_key(&dlci) {
            return Err(ErrorKind::Other);
        }
        self.frames.push(uih(dlci, bytes));
        Ok(())
    }

    /// What the call returns; `None`: not observable now, skipped.
    fn step(&mut self, a: &Action) -> Option<Result<(), ErrorKind>> {
        if self.unplugged {
            return None;
        }
        Some(match a {
            Action::Enable => {
                if self.enabled {
                    return None;
                }
                self.enabled = true;
                self.commands.push((None, COMMAND.into()));
                self.frames.push((0, mux::SABM, Vec::new()));
                Ok(())
            }
            Action::Disable => {
                let paths: Vec<u8> = self.paths.keys().copied().collect();
                // The map has no order: frames and states are compared sorted.
                for dlci in paths {
                    self.close(dlci);
                }
                if self.enabled {
                    self.frames.push(uih(0, &[mux::CLD, 1]));
                }
                self.end_session();
                Ok(())
            }
            Action::Open(dlci) => {
                if !(1..=63).contains(dlci) {
                    return Some(Err(ErrorKind::Other));
                }
                if !self.enabled {
                    return Some(Err(ErrorKind::NotFound));
                }
                if self.open.contains_key(dlci) {
                    return Some(Ok(()));
                }
                self.frames.push((*dlci, mux::SABM, Vec::new()));
                if *dlci > GOOD {
                    // The half-open channel is taken back.
                    self.frames.push((*dlci, mux::DISC, Vec::new()));
                    return Some(Err(ErrorKind::Other));
                }
                // V.24 signals of the new channel: ready.
                self.frames
                    .push(uih(0, &[mux::MSC, 0x05, dlci << 2 | 3, 0x8D]));
                self.open.insert(*dlci, ());
                self.paths.insert(*dlci, ());
                self.states.push((*dlci, State::Opened));
                Ok(())
            }
            Action::Close(dlci) => {
                if !self.paths.contains_key(dlci) {
                    return Some(Err(ErrorKind::NotFound));
                }
                self.close(*dlci);
                Ok(())
            }
            Action::Write(dlci, bytes) => {
                if self.enabled && self.open.contains_key(dlci) && self.held(*dlci) {
                    // One waiting write per channel: two have no order.
                    if self.waiting.contains_key(dlci) {
                        return None;
                    }
                    self.waiting.insert(*dlci, bytes.clone());
                    return Some(Ok(()));
                }
                self.write(*dlci, bytes)
            }
            Action::Data(dlci, bytes) => {
                if !self.enabled {
                    return None;
                }
                let open = self.open.contains_key(dlci);
                for s in self.streams().filter(|s| s.dlci == *dlci && s.attached) {
                    assert!(open);
                    s.feed(bytes);
                }
                Ok(())
            }
            Action::At(dlci) => {
                if self.held(*dlci) {
                    return None;
                }
                self.write(*dlci, b"AT\r").map(|()| {
                    self.commands.push((Some(*dlci), "AT".into()));
                })
            }
            Action::Watch(dlci, framed) => {
                if !self.open.contains_key(dlci)
                    || self.watches.iter().any(|s| s.dlci == *dlci && s.attached)
                {
                    return None;
                }
                self.watches.push(Stream {
                    dlci: *dlci,
                    framed: *framed,
                    batched: !framed,
                    attached: true,
                    partial: Vec::new(),
                    got: Got::default(),
                });
                Ok(())
            }
            Action::Unwatch(dlci) => {
                let watch = self
                    .watches
                    .iter_mut()
                    .find(|s| s.dlci == *dlci && s.attached)?;
                watch.finish();
                Ok(())
            }
            Action::WatchCore(dlci, framed) => {
                if !self.open.contains_key(dlci) || self.cores.len() > 4 {
                    return None;
                }
                self.cores.push(Stream {
                    dlci: *dlci,
                    framed: *framed,
                    batched: false,
                    attached: true,
                    partial: Vec::new(),
                    got: Got::default(),
                });
                Ok(())
            }
            Action::UnwatchCore(i) => {
                let n = self.cores.len().max(1);
                let core = self.cores.get_mut(i % n).filter(|s| s.attached)?;
                core.finish();
                Ok(())
            }
            Action::ModemCloses(dlci, disc) => {
                if !self.enabled {
                    return None;
                }
                let open = self.open.contains_key(dlci);
                if *disc {
                    // UA for an open channel, DM for any other.
                    let answer = if open { mux::UA } else { mux::DM };
                    self.frames.push((*dlci, answer, Vec::new()));
                }
                if open {
                    self.end_channel(*dlci, true);
                    self.states.push((*dlci, State::Disconnected));
                }
                Ok(())
            }
            Action::ModemLeaves(how) => {
                if !self.enabled {
                    return None;
                }
                match how {
                    Leave::Disc => self.frames.push((0, mux::UA, Vec::new())),
                    Leave::Cld => self.frames.push(uih(0, &[mux::CLD & !2, 1])),
                    Leave::Dm => {}
                }
                let open: Vec<u8> = self.open.keys().copied().collect();
                for dlci in open {
                    self.end_channel(dlci, true);
                    self.states.push((dlci, State::Disconnected));
                }
                self.end_session();
                Ok(())
            }
            Action::FlowOff(on) => {
                if !self.enabled {
                    return None;
                }
                let command = if *on { mux::FCOFF } else { mux::FCON };
                self.frames.push(uih(0, &[command & !2, 1]));
                self.flow_off = *on;
                self.release();
                Ok(())
            }
            Action::ChannelFlowOff(dlci, on) => {
                if !self.enabled || !self.open.contains_key(dlci) {
                    return None;
                }
                let v24 = if *on { 0x8F } else { 0x8D };
                self.frames
                    .push(uih(0, &[mux::MSC & !2, 0x05, dlci << 2 | 3, v24]));
                if *on {
                    self.channels_off.insert(*dlci, ());
                } else {
                    self.channels_off.remove(dlci);
                }
                self.release();
                Ok(())
            }
            Action::Control(known, value) => {
                if !self.enabled {
                    return None;
                }
                let reply = if *known {
                    // TEST is echoed as a response.
                    [&[0x21, (value.len() as u8) << 1 | 1], value.as_slice()].concat()
                } else {
                    vec![0x11, 0x03, 0xF3]
                };
                self.frames.push(uih(0, &reply));
                Ok(())
            }
            Action::Unplug => {
                let open: Vec<u8> = self.open.keys().copied().collect();
                for dlci in open {
                    self.end_channel(dlci, true);
                    self.states.push((dlci, State::Disconnected));
                }
                self.unplugged = true;
                self.end_session();
                Ok(())
            }
        })
    }
}

type Shared = Arc<Mutex<Got>>;

fn recorder(framed: bool) -> (impl Fn(SerialEvent) -> bool + Send + Sync, Shared) {
    let got = Shared::default();
    let sink_got = got.clone();
    let sink = move |event| {
        let mut got = sink_got.lock().unwrap();
        match event {
            SerialEvent::Data { data, .. } if framed => got.frames.push(data),
            SerialEvent::Data { data, .. } => got.data.extend(data),
            SerialEvent::Disconnect { .. } => got.disconnects += 1,
            other => panic!("unexpected {other:?}"),
        }
        true
    };
    (sink, got)
}

struct Sut {
    m: SerialManager,
    device: FakeDevice,
    /// One modem per multiplexer session.
    logs: Vec<ModemLog>,
    states: Receiver<CoreEvent>,
    seen_states: Vec<(u8, State)>,
    waiting: BTreeMap<u8, JoinHandle<Result<(), ErrorKind>>>,
    watches: Vec<(u32, u8, Shared)>,
    cores: Vec<(SubscriptionId, Shared)>,
}

impl Drop for Sut {
    fn drop(&mut self) {
        let _ = self.m.close_all();
    }
}

impl Sut {
    fn new() -> Self {
        let device = FakeDevice::new();
        let m = SerialManager::new();
        m.insert_port(PATH.into(), Box::new(device.port())).unwrap();
        // A write that waits on flow control may wait for twice its wire time: at 1 baud
        // longer than any case takes.
        m.set_baud_rate(PATH.into(), 1).unwrap();
        let (sink, states) = sink();
        m.subscribe_core(sink);
        Self {
            m,
            device,
            logs: Vec::new(),
            states,
            seen_states: Vec::new(),
            waiting: BTreeMap::new(),
            watches: Vec::new(),
            cores: Vec::new(),
        }
    }

    fn frames(&self) -> Vec<Seen> {
        let all = self.logs.iter().flat_map(ModemLog::frames);
        all.map(|f| (f.dlci, f.kind, f.payload)).collect()
    }

    fn commands(&self) -> Vec<(Option<u8>, String)> {
        let all = self.logs.iter().flat_map(ModemLog::commands);
        all.filter(|c| c.dlci.is_none() || c.line == "AT")
            .map(|c| (c.dlci, c.line))
            .collect()
    }

    fn step(&mut self, a: &Action, model: &Model) -> Result<(), ErrorKind> {
        let m = self.m.clone();
        let channel = |dlci: u8| mux_path(PATH, dlci);
        match a {
            Action::Enable => {
                let options = MuxOptions::default().refuse([REFUSED]);
                let modem = Modem::new().silent("AT+W").mux(options);
                self.logs.push(modem.log());
                self.device.modem(modem);
                to_kind(m.enable_mux(PATH.into(), COMMAND.into(), LONG_MS))
            }
            Action::Disable => to_kind(m.disable_mux(PATH.into())),
            Action::Open(dlci) => {
                let r = m.open_mux_channel(PATH.into(), *dlci);
                if let Ok(path) = &r {
                    assert_eq!(*path, channel(*dlci));
                }
                to_kind(r)
            }
            Action::Close(dlci) => to_kind(m.close(channel(*dlci))),
            Action::Write(dlci, bytes) => {
                let (path, bytes) = (channel(*dlci), bytes.clone());
                if model.waiting.contains_key(dlci) {
                    let write = background(&m, move |m| to_kind(m.write_binary(path, bytes)));
                    self.waiting.insert(*dlci, write);
                    return Ok(());
                }
                to_kind(m.write_binary(path, bytes))
            }
            Action::Data(dlci, bytes) => {
                self.device.send(&mux::data(*dlci, bytes));
                Ok(())
            }
            Action::At(dlci) => to_kind(m.at(channel(*dlci), "AT".into(), None).map(|r| {
                assert_eq!(r.status, AtParseStatus::Ok);
            })),
            Action::Watch(dlci, framed) => {
                let (sink, got) = recorder(*framed);
                let id = m.watch_framed(
                    channel(*dlci),
                    watch_opts(NEVER, false),
                    framed.then(|| readline().unwrap()),
                    Arc::new(sink),
                );
                let id = id.map_err(kind)?;
                self.watches.push((id, *dlci, got));
                Ok(())
            }
            Action::Unwatch(dlci) => {
                let last = self.watches.iter().rev().find(|w| w.1 == *dlci);
                to_kind(m.unwatch(last.unwrap().0))
            }
            Action::WatchCore(dlci, framed) => {
                let (sink, got) = recorder(*framed);
                let sink = Arc::new(move |e| match e {
                    CoreEvent::Event(e) => sink(e),
                    other => panic!("unexpected {other:?}"),
                });
                let parser = framed.then(|| readline().unwrap());
                let id = m.watch_core(channel(*dlci), watch_opts(0, false), parser, sink);
                self.cores.push((id.map_err(|e| e.kind())?, got));
                Ok(())
            }
            Action::UnwatchCore(i) => {
                let i = i % self.cores.len();
                core_kind(m.unwatch_core(self.cores[i].0))
            }
            Action::ModemCloses(dlci, disc) => {
                // A command, or an (unsolicited) response.
                let kind = if *disc { mux::DISC } else { mux::DM };
                self.device
                    .send(&mux::frame(*dlci, !*disc, kind | mux::PF, &[]));
                Ok(())
            }
            Action::ModemLeaves(how) => {
                self.device.send(&match how {
                    Leave::Disc => mux::frame(0, false, mux::DISC | mux::PF, &[]),
                    Leave::Dm => mux::frame(0, true, mux::DM | mux::PF, &[]),
                    Leave::Cld => mux::control(mux::CLD, &[]),
                });
                Ok(())
            }
            Action::FlowOff(on) => {
                let command = if *on { mux::FCOFF } else { mux::FCON };
                self.device.send(&mux::control(command, &[]));
                Ok(())
            }
            Action::ChannelFlowOff(dlci, on) => {
                let v24 = if *on { 0x8F } else { 0x8D };
                self.device
                    .send(&mux::control(mux::MSC, &[dlci << 2 | 3, v24]));
                Ok(())
            }
            Action::Control(known, value) => {
                self.device.send(&if *known {
                    mux::control(0x23, value)
                } else {
                    mux::control(0xF3, &[])
                });
                Ok(())
            }
            Action::Unplug => {
                self.device.fail_reads(io::ErrorKind::BrokenPipe);
                Ok(())
            }
        }
    }

    fn check(&mut self, model: &Model) {
        if model.unplugged {
            self.device.wait_closed();
        } else {
            self.device.wait_handled();
        }
        // Writes that no longer wait: sent, or failed with their channel.
        let done: Vec<u8> = self
            .waiting
            .keys()
            .copied()
            .filter(|d| !model.waiting.contains_key(d))
            .collect();
        for dlci in done {
            let result = self.waiting.remove(&dlci).unwrap().join().unwrap();
            assert_eq!(
                result.is_ok(),
                model.open.contains_key(&dlci),
                "waiting write on {dlci}: {result:?}"
            );
        }
        settle(|| {
            while let Ok(event) = self.states.try_recv() {
                let CoreEvent::PortState { path, state } = event else {
                    panic!("unexpected {event:?}");
                };
                // The port's own state: the manager model checks it.
                let Some((_, dlci)) = serialplugin_core::cmux::parse_mux_path(&path) else {
                    continue;
                };
                self.seen_states.push((
                    dlci,
                    match state {
                        PortStatus::Opened => State::Opened,
                        PortStatus::Closed => State::Closed,
                        PortStatus::Disconnected { kind, .. } => {
                            assert_eq!(kind, ErrorKind::Disconnected);
                            State::Disconnected
                        }
                        other => panic!("unexpected {other:?}"),
                    },
                ));
            }
            self.seen_states.len() >= model.states.len()
                && self.frames().len() >= model.frames.len()
                && self.streams(model).all(|(got, want)| {
                    let got = got.lock().unwrap();
                    got.data.len() >= want.got.data.len()
                        && got.frames.len() >= want.got.frames.len()
                        && got.disconnects >= want.got.disconnects
                })
        });
        for (got, want) in self.streams(model) {
            assert_eq!(*got.lock().unwrap(), want.got, "stream on {}", want.dlci);
        }
        // Closing several channels goes in no particular order.
        let sorted = |v: &[(u8, State)]| {
            let mut v: Vec<_> = v.iter().map(|(d, s)| (*d, format!("{s:?}"))).collect();
            v.sort_by_key(|s| s.0);
            v
        };
        assert_eq!(sorted(&self.seen_states), sorted(&model.states));
        let by_channel = |v: &[Seen]| {
            let mut v = v.to_vec();
            v.sort_by_key(|f| f.0);
            v
        };
        assert_eq!(by_channel(&self.frames()), by_channel(&model.frames));
        assert_eq!(self.commands(), model.commands);
        assert!(self.logs.iter().all(|l| !l.cld_before_ua()));
        let mut managed: Vec<String> = model.paths.keys().map(|d| mux_path(PATH, *d)).collect();
        managed.insert(0, PATH.into());
        assert_eq!(self.m.managed_ports().unwrap(), managed);
    }

    fn streams<'a>(&'a self, model: &'a Model) -> impl Iterator<Item = (&'a Shared, &'a Stream)> {
        let watches = self.watches.iter().map(|w| &w.2);
        let got = watches.chain(self.cores.iter().map(|c| &c.1));
        got.zip(model.watches.iter().chain(&model.cores))
    }
}

fn action() -> impl Strategy<Value = Action> {
    let good = || 1..=GOOD;
    let bytes = || vec(prop::sample::select(b"abc\n".to_vec()), 1..20);
    let leave = prop::sample::select(vec![Leave::Disc, Leave::Dm, Leave::Cld]);
    prop_oneof![
        6 => Just(Action::Enable),
        1 => Just(Action::Disable),
        8 => prop_oneof![8 => good(), 1 => Just(REFUSED), 1 => Just(0), 1 => Just(64)]
            .prop_map(Action::Open),
        2 => good().prop_map(Action::Close),
        // Commands the modem takes without an answer.
        5 => (good(), 1usize..4).prop_map(|(d, n)| Action::Write(d, b"AT+W\r".repeat(n))),
        6 => (good(), bytes()).prop_map(|(d, b)| Action::Data(d, b)),
        3 => good().prop_map(Action::At),
        3 => (good(), any::<bool>()).prop_map(|(d, f)| Action::Watch(d, f)),
        1 => good().prop_map(Action::Unwatch),
        3 => (good(), any::<bool>()).prop_map(|(d, f)| Action::WatchCore(d, f)),
        1 => any::<usize>().prop_map(Action::UnwatchCore),
        2 => (good(), any::<bool>()).prop_map(|(d, disc)| Action::ModemCloses(d, disc)),
        1 => leave.prop_map(Action::ModemLeaves),
        3 => any::<bool>().prop_map(Action::FlowOff),
        3 => (good(), any::<bool>()).prop_map(|(d, on)| Action::ChannelFlowOff(d, on)),
        2 => (any::<bool>(), vec(any::<u8>(), 0..5)).prop_map(|(k, v)| Action::Control(k, v)),
        1 => Just(Action::Unplug),
    ]
}

#[test]
fn the_multiplexer_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 32,
        failure_persistence: None,
        max_shrink_time: 20_000,
        ..Config::default()
    });
    let result = runner.run(&vec(action(), 4..40), |actions| {
        let mut model = Model::default();
        let mut sut = Sut::new();
        for a in &actions {
            let Some(want) = model.step(a) else {
                continue;
            };
            let got = sut.step(a, &model);
            assert_eq!(got, want, "result of {a:?}");
            sut.check(&model);
        }
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}
