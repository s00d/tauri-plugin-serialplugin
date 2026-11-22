//! Device model: the plugin through IPC against real devices, PTY pairs (Unix) and `tcp://`
//! bridges, on two ports at once. After every step the plugin's view is compared with a
//! reference model: managed ports, the settings on the port handle, what each JS channel got
//! (data, `disconnect` exactly once, after the data), the watch registry, and the RX bytes
//! waiting for `read`.

use crate::common::*;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serde::de::DeserializeOwned;
use serde_json::{json, Value};
use serialplugin_testing::{eventually, peer};
use std::sync::atomic::{AtomicUsize, Ordering};
use tauri_plugin_serialplugin::error::ErrorKind;
use tauri_plugin_serialplugin::state as st;

const SLOTS: usize = 2;
/// Of a `read` that finds nothing, or less than it asked for.
const READ_TIMEOUT_MS: u64 = 10;
/// A batch window that never ends.
const NEVER_MS: u64 = 600_000;

type Name = &'static str;

#[derive(Clone, Debug)]
struct Settings {
    baud: u32,
    data_bits: Name,
    flow: Name,
    parity: Name,
    stop: Name,
}

/// Settings given to `open`; `None` is left out (the default).
#[derive(Clone, Debug)]
struct OpenArgs {
    baud: u32,
    data_bits: Option<Name>,
    flow: Option<Name>,
    parity: Option<Name>,
    stop: Option<Name>,
    dtr: Option<bool>,
    /// A key the plugin does not know.
    extra: bool,
}

#[derive(Clone, Copy, Debug)]
enum Setting {
    Baud(u32),
    DataBits(Name),
    Flow(Name),
    Parity(Name),
    Stop(Name),
    Timeout(u64),
}

#[derive(Clone, Copy, Debug)]
enum Misc {
    Clear(Name),
    BytesToWrite,
    CancelRead,
    CancelExchange,
    Break,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum WatchKind {
    Plain,
    LongBatch,
    /// A `readline` parser.
    Lines,
}

#[derive(Clone, Debug)]
enum Action {
    Open(usize, Kind, OpenArgs),
    /// `open` of a live TCP bridge with an argument guest-js never sends.
    OpenBadArgs(u8),
    Close(usize),
    ForceClose(usize),
    CloseAll,
    Write {
        slot: usize,
        data: Vec<u8>,
        binary: bool,
    },
    DeviceSend {
        slot: usize,
        data: Vec<u8>,
    },
    Read {
        slot: usize,
        size: usize,
        binary: bool,
    },
    Watch(usize, WatchKind),
    WatchBadParser(usize),
    Unwatch(usize),
    Set(usize, Setting),
    /// DTR (`true`) or RTS.
    Lines {
        slot: usize,
        dtr: bool,
        level: bool,
    },
    /// 0 CTS, 1 DSR, 2 RI, 3 CD.
    ReadLine(usize, usize),
    Misc(usize, Misc),
    /// Numeric result codes and a short default timeout for `at` on the port.
    ConfigureAt {
        slot: usize,
        timeout_ms: u64,
    },
    /// `at` the device answers (in the session's result format).
    At(usize),
    /// `at` with the session defaults; the device stays silent.
    SilentAt(usize),
    /// `exchange` the device answers with `reply` (0 OK, 1 ERROR, 2 +CME, 3 +CMS).
    Exchange {
        slot: usize,
        binary: bool,
        reply: usize,
    },
    /// The device disappears (PTY master closed / TCP peer dropped).
    DeviceGone(usize),
}

// ---------------------------------------------------------------- model

#[derive(Clone, Debug)]
struct PortM {
    kind: Kind,
    settings: Settings,
    /// Index of the channel watching the port.
    watch: Option<usize>,
    /// The device went away; the path stays managed until `close` (4.x).
    gone: bool,
    /// RX nobody consumed yet (what `read` returns).
    pending: Vec<u8>,
    /// AT session configured (numeric result codes).
    session: bool,
}

#[derive(Clone, Debug)]
struct ChanM {
    kind: WatchKind,
    /// Bytes it was given in all.
    fed: usize,
    /// Bytes that must have arrived; with a parser, its frames.
    delivered: Vec<u8>,
    frames: Vec<Vec<u8>>,
    /// Bytes of an open batch or an unfinished line: they must arrive when the watch ends.
    held: Vec<u8>,
    disconnects: usize,
    registered: bool,
}

impl ChanM {
    fn new(kind: WatchKind) -> Self {
        Self {
            kind,
            fed: 0,
            delivered: Vec::new(),
            frames: Vec::new(),
            held: Vec::new(),
            disconnects: 0,
            registered: false,
        }
    }

    fn feed(&mut self, data: &[u8]) {
        self.fed += data.len();
        match self.kind {
            WatchKind::Plain => self.delivered.extend(data),
            WatchKind::LongBatch => self.held.extend(data),
            WatchKind::Lines => {
                for &b in data {
                    match b {
                        b'\n' => self.end_line(),
                        b => self.held.push(b),
                    }
                }
            }
        }
    }

    /// An empty line is no frame.
    fn end_line(&mut self) {
        if !self.held.is_empty() {
            self.frames.push(std::mem::take(&mut self.held));
        }
    }

    /// The stream ends: what was held arrives.
    fn flush(&mut self) {
        match self.kind {
            WatchKind::Lines => self.end_line(),
            _ => self.delivered.append(&mut self.held),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Expect {
    Ok,
    Err(ErrorKind),
    BadArgs,
    /// A PTY may not support the call (modem lines, break): either is fine.
    #[cfg_attr(not(unix), allow(dead_code))]
    Either,
    Bytes(Vec<u8>),
    Count(usize),
    Level(bool),
}

#[derive(Debug, Default)]
struct Model {
    ports: [Option<PortM>; SLOTS],
    /// The device of the slot, kept when its port is closed.
    devices: [Option<Kind>; SLOTS],
    chans: Vec<ChanM>,
    /// The channel the last `Unwatch` stopped.
    target: Option<usize>,
}

impl Model {
    /// The port, or how a call on it fails.
    fn live(&mut self, slot: usize) -> Result<&mut PortM, Expect> {
        match self.ports[slot].as_mut() {
            None => Err(Expect::Err(ErrorKind::NotFound)),
            Some(p) if p.gone => Err(Expect::Err(ErrorKind::Disconnected)),
            Some(p) => Ok(p),
        }
    }

    /// Close: the watch gets its tail and its registration goes; no `disconnect`.
    fn close(&mut self, slot: usize) {
        let Some(port) = self.ports[slot].take() else {
            return;
        };
        if let Some(c) = port.watch {
            self.chans[c].flush();
            self.chans[c].registered = false;
        }
        if port.gone {
            self.devices[slot] = None;
        }
    }

    /// What the action should return, or `None` when it means nothing now.
    fn step(&mut self, a: &Action) -> Option<Expect> {
        self.target = None;
        match self.expect(a) {
            Ok(expect) => expect,
            Err(expect) => Some(expect),
        }
    }

    fn expect(&mut self, a: &Action) -> Result<Option<Expect>, Expect> {
        Ok(Some(match a {
            Action::Open(slot, kind, args) => {
                // A vanished device cannot be opened again.
                if self.ports[*slot].as_ref().is_some_and(|p| p.gone) {
                    return Ok(None);
                }
                // Opening an open path replaces the session.
                self.close(*slot);
                let kind = *self.devices[*slot].get_or_insert(*kind);
                self.ports[*slot] = Some(PortM {
                    kind,
                    settings: Settings {
                        baud: args.baud,
                        data_bits: args.data_bits.unwrap_or("Eight"),
                        flow: args.flow.unwrap_or("None"),
                        parity: args.parity.unwrap_or("None"),
                        stop: args.stop.unwrap_or("One"),
                    },
                    watch: None,
                    gone: false,
                    pending: Vec::new(),
                    session: false,
                });
                Expect::Ok
            }
            Action::OpenBadArgs(_) => Expect::BadArgs,
            Action::Close(slot) => {
                let open = self.ports[*slot].is_some();
                self.close(*slot);
                if open {
                    Expect::Ok
                } else {
                    Expect::Err(ErrorKind::NotFound)
                }
            }
            // An unknown path is no error for `force_close`.
            Action::ForceClose(slot) => {
                self.close(*slot);
                Expect::Ok
            }
            Action::CloseAll => {
                (0..SLOTS).for_each(|s| self.close(s));
                Expect::Ok
            }
            Action::Write { slot, data, .. } => {
                self.live(*slot)?;
                Expect::Count(data.len())
            }
            Action::DeviceSend { slot, data } => {
                let Ok(p) = self.live(*slot) else {
                    return Ok(None);
                };
                match p.watch {
                    Some(c) => self.chans[c].feed(data),
                    None => p.pending.extend(data),
                }
                Expect::Ok
            }
            Action::Read { slot, size, .. } => {
                let p = self.live(*slot)?;
                if p.watch.is_some() || p.pending.is_empty() {
                    return Err(Expect::Err(ErrorKind::Other));
                }
                // `read` returns what is there; `read_binary` waits for `size` bytes and
                // returns what came within the timeout: the same when nothing more comes.
                let n = (*size).min(p.pending.len());
                Expect::Bytes(p.pending.drain(..n).collect())
            }
            Action::Watch(slot, kind) => {
                let c = self.chans.len();
                self.chans.push(ChanM::new(*kind));
                let Some(p) = self.ports[*slot].as_mut() else {
                    return Err(Expect::Err(ErrorKind::NotFound));
                };
                if p.watch.is_some() {
                    return Err(Expect::Err(ErrorKind::Busy));
                }
                p.watch = Some(c);
                // Attaching drops RX nobody asked for.
                p.pending.clear();
                self.chans[c].registered = true;
                // A released port hands a late watch its `disconnect` (4.x).
                self.chans[c].disconnects = usize::from(p.gone);
                Expect::Ok
            }
            Action::WatchBadParser(_) => {
                self.chans.push(ChanM::new(WatchKind::Lines));
                Expect::Err(ErrorKind::Other)
            }
            Action::Unwatch(slot) => {
                let watch = self.ports[*slot].as_mut().and_then(|p| p.watch.take());
                let Some(c) = watch else {
                    return Err(Expect::Err(ErrorKind::NotFound));
                };
                self.chans[c].flush();
                self.chans[c].registered = false;
                self.target = Some(c);
                Expect::Ok
            }
            Action::Set(slot, setting) => {
                let p = self.live(*slot)?;
                let s = &mut p.settings;
                match *setting {
                    Setting::Baud(_) if p.kind != Kind::Tcp && PTY_KEEPS_BAUD => {
                        return Err(Expect::Err(ErrorKind::Other));
                    }
                    Setting::Baud(b) => s.baud = b,
                    Setting::DataBits(d) => s.data_bits = d,
                    Setting::Flow(f) => s.flow = f,
                    Setting::Parity(x) => s.parity = x,
                    Setting::Stop(x) => s.stop = x,
                    Setting::Timeout(_) => {}
                }
                Expect::Ok
            }
            Action::Lines { slot, .. } => match self.live(*slot)?.kind {
                Kind::Tcp => Expect::Ok,
                #[cfg(unix)]
                Kind::Pty => Expect::Either,
            },
            Action::ReadLine(slot, which) => match self.live(*slot)?.kind {
                // A connected raw bridge: CTS, DSR, CD asserted, no ring.
                Kind::Tcp => Expect::Level(*which != 2),
                #[cfg(unix)]
                Kind::Pty => Expect::Either,
            },
            Action::Misc(slot, op) => {
                let p = self.live(*slot)?;
                if matches!(op, Misc::Clear("Input" | "All")) {
                    p.pending.clear();
                }
                match (op, p.kind) {
                    #[cfg(unix)]
                    (Misc::Break, Kind::Pty) => Expect::Either,
                    _ => Expect::Ok,
                }
            }
            Action::ConfigureAt { slot, .. } => {
                // It says "not open" for a vanished device too.
                let Ok(p) = self.live(*slot) else {
                    return Err(Expect::Err(ErrorKind::NotFound));
                };
                p.session = true;
                Expect::Ok
            }
            Action::At(slot) | Action::Exchange { slot, .. } => {
                // Idle RX is replayed into the exchange (rxPrepare none).
                self.live(*slot)?.pending.clear();
                Expect::Ok
            }
            // Without a session a silent device would be waited for 5 s.
            Action::SilentAt(slot) => match self.live(*slot) {
                Ok(p) if p.session => {
                    p.pending.clear();
                    Expect::Err(ErrorKind::Timeout)
                }
                _ => return Ok(None),
            },
            Action::DeviceGone(slot) => {
                let Ok(p) = self.live(*slot) else {
                    return Ok(None);
                };
                p.gone = true;
                p.pending.clear();
                if let Some(c) = p.watch {
                    self.chans[c].flush();
                    self.chans[c].disconnects += 1;
                }
                Expect::Ok
            }
        }))
    }
}

// ---------------------------------------------------------------- strategies

fn pick(values: &'static [Name]) -> impl Strategy<Value = Name> {
    proptest::sample::select(values)
}

const DATA_BITS: &[Name] = &["Five", "Six", "Seven", "Eight"];
const FLOW: &[Name] = &["None", "Software", "Hardware"];
const PARITY: &[Name] = &["None", "Odd", "Even"];
const STOP: &[Name] = &["One", "Two"];

fn baud() -> impl Strategy<Value = u32> {
    prop_oneof![Just(9600u32), Just(19_200), Just(115_200)]
}

/// No XON/XOFF, CR or high bit for the PTY line discipline, and nothing that could read as a
/// result code when a later exchange gets it replayed.
fn rx() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(prop_oneof![4 => b'a'..=b'd', 1 => Just(b'\n')], 1..16)
}

fn tx() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(prop_oneof![b'a'..=b'z', b'0'..=b'9'], 1..16)
}

fn setting() -> impl Strategy<Value = Setting> {
    prop_oneof![
        baud().prop_map(Setting::Baud),
        pick(DATA_BITS).prop_map(Setting::DataBits),
        pick(FLOW).prop_map(Setting::Flow),
        pick(PARITY).prop_map(Setting::Parity),
        pick(STOP).prop_map(Setting::Stop),
        (1u64..2000).prop_map(Setting::Timeout),
    ]
}

fn misc() -> impl Strategy<Value = Misc> {
    prop_oneof![
        pick(&["Input", "Output", "All"]).prop_map(Misc::Clear),
        Just(Misc::BytesToWrite),
        Just(Misc::CancelRead),
        Just(Misc::CancelExchange),
        Just(Misc::Break),
    ]
}

fn open_args() -> impl Strategy<Value = OpenArgs> {
    use proptest::option::of;
    (
        baud(),
        of(pick(DATA_BITS)),
        of(pick(FLOW)),
        of(pick(PARITY)),
        of(pick(STOP)),
        of(any::<bool>()),
        any::<bool>(),
    )
        .prop_map(
            |(baud, data_bits, flow, parity, stop, dtr, extra)| OpenArgs {
                baud,
                data_bits,
                flow,
                parity,
                stop,
                dtr,
                extra,
            },
        )
}

fn watch_kind() -> impl Strategy<Value = WatchKind> {
    prop_oneof![
        Just(WatchKind::Plain),
        Just(WatchKind::LongBatch),
        Just(WatchKind::Lines)
    ]
}

fn action() -> impl Strategy<Value = Action> {
    let slot = || 0..SLOTS;
    prop_oneof![
        5 => (slot(), proptest::sample::select(KINDS), open_args())
            .prop_map(|(slot, kind, args)| Action::Open(slot, kind, args)),
        1 => (0u8..3).prop_map(Action::OpenBadArgs),
        1 => slot().prop_map(Action::Close),
        1 => slot().prop_map(Action::ForceClose),
        1 => Just(Action::CloseAll),
        4 => (slot(), tx(), any::<bool>())
            .prop_map(|(slot, data, binary)| Action::Write { slot, data, binary }),
        16 => (slot(), rx()).prop_map(|(slot, data)| Action::DeviceSend { slot, data }),
        5 => (slot(), 1usize..24, any::<bool>())
            .prop_map(|(slot, size, binary)| Action::Read { slot, size, binary }),
        12 => (slot(), watch_kind()).prop_map(|(slot, kind)| Action::Watch(slot, kind)),
        2 => slot().prop_map(Action::WatchBadParser),
        12 => slot().prop_map(Action::Unwatch),
        6 => (slot(), setting()).prop_map(|(s, x)| Action::Set(s, x)),
        1 => (slot(), any::<bool>(), any::<bool>())
            .prop_map(|(slot, dtr, level)| Action::Lines { slot, dtr, level }),
        2 => (slot(), 0usize..4).prop_map(|(slot, which)| Action::ReadLine(slot, which)),
        4 => (slot(), misc()).prop_map(|(s, m)| Action::Misc(s, m)),
        3 => (slot(), 20u64..40)
            .prop_map(|(slot, timeout_ms)| Action::ConfigureAt { slot, timeout_ms }),
        3 => slot().prop_map(Action::At),
        1 => slot().prop_map(Action::SilentAt),
        4 => (slot(), any::<bool>(), 0usize..4)
            .prop_map(|(slot, binary, reply)| Action::Exchange { slot, binary, reply }),
        1 => slot().prop_map(Action::DeviceGone),
    ]
}

/// Both ports open, then anything.
fn case() -> impl Strategy<Value = Vec<Action>> {
    let open = |slot| {
        (proptest::sample::select(KINDS), open_args())
            .prop_map(move |(kind, args)| Action::Open(slot, kind, args))
    };
    (open(0), open(1), proptest::collection::vec(action(), 1..40)).prop_map(
        |(first, second, mut actions)| {
            actions.splice(0..0, [first, second]);
            actions
        },
    )
}

// ---------------------------------------------------------------- system under test

struct ChanS {
    path: String,
    events: Events,
    seen: Vec<Event>,
}

struct Sut {
    plugin: Plugin,
    devices: [Option<Device>; SLOTS],
    /// The device's path; before there is one (and after it is gone) a path no device has.
    paths: [String; SLOTS],
    chans: Vec<ChanS>,
    /// Beside the watch of each port that has one.
    fences: [Option<Fence>; SLOTS],
}

impl Sut {
    fn new() -> Self {
        Self {
            plugin: Plugin::new(),
            devices: Default::default(),
            paths: std::array::from_fn(|_| Self::no_device()),
            chans: Vec::new(),
            fences: Default::default(),
        }
    }

    fn no_device() -> String {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        format!("/dev/no-device-{}", NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn plug_in(&mut self, slot: usize, kind: Kind) {
        // The system may hand out the name of a vanished device that is still managed.
        let mut taken = Vec::new();
        let device = loop {
            let device = Device::new(kind);
            if !self.paths.contains(&device.path) {
                break device;
            }
            taken.push(device);
        };
        self.paths[slot] = device.path.clone();
        self.devices[slot] = Some(device);
    }

    fn device(&mut self, slot: usize) -> &mut Device {
        self.devices[slot].as_mut().unwrap()
    }

    /// The port of `slot` was closed; a vanished device leaves its path for good.
    fn closed(&mut self, slot: usize) {
        if self.devices[slot].is_none() {
            self.paths[slot] = Self::no_device();
        }
    }

    fn watch(&mut self, slot: usize, extra: Value) -> Result<Value, Value> {
        let path = self.paths[slot].clone();
        let (events, result) = self.plugin.try_watch(&path, extra);
        if result.is_ok() {
            assert_eq!(result, Ok(json!(events.id)), "watch returns the channel id");
        }
        self.chans.push(ChanS {
            path,
            events,
            seen: Vec::new(),
        });
        result
    }

    /// Run `cmd` with the device answering `reply`; the command must reach it as `AT\r`.
    fn answered(
        &mut self,
        slot: usize,
        live: bool,
        reply: &'static [u8],
        cmd: &str,
        args: Value,
    ) -> Result<Value, Value> {
        if !live {
            return self.plugin.invoke(cmd, args);
        }
        let device = self.device(slot).answer(reply);
        let result = self.plugin.invoke(cmd, args);
        assert_eq!(device.join().unwrap(), b"AT\r", "{cmd}: command sent");
        result
    }

    /// The result of the action, or `None` when the device did it.
    fn apply(&mut self, m: &Model, expect: &Expect, a: &Action) -> Option<Result<Value, Value>> {
        let live = *expect == Expect::Ok;
        Some(match a {
            Action::Open(slot, kind, args) => {
                let slot = *slot;
                if self.devices[slot].is_none() {
                    self.plug_in(slot, *kind);
                }
                let device = self.devices[slot].as_mut().unwrap();
                let pty = device.kind != Kind::Tcp;
                let mut json = json!({ "path": device.path, "timeout": 200, "dtr": args.dtr });
                json["baudRate"] = json!(if pty && PTY_KEEPS_BAUD { 0 } else { args.baud });
                for (key, value) in [
                    ("dataBits", args.data_bits),
                    ("flowControl", args.flow),
                    ("parity", args.parity),
                    ("stopBits", args.stop),
                ] {
                    if let Some(value) = value {
                        json[key] = json!(value);
                    }
                }
                if !pty {
                    json["rts"] = json!(args.dtr);
                }
                if args.extra {
                    json["zzUnknownKey"] = json!(true);
                }
                let result = self.plugin.invoke("open", json);
                assert_eq!(result, Ok(json!(device.path)), "{a:?}: the session path");
                device.connected();
                result
            }
            Action::OpenBadArgs(which) => {
                let (listener, path) = peer::tcp_listener();
                let args = match which {
                    0 => json!({ "path": path, "baudRate": "fast" }),
                    1 => json!({ "path": path, "baudRate": 9600, "dataBits": "Nine" }),
                    _ => json!({ "path": path }),
                };
                let result = self.plugin.invoke("open", args);
                listener.set_nonblocking(true).unwrap();
                assert!(listener.accept().is_err(), "{a:?}: connected anyway");
                result
            }
            Action::Close(slot) | Action::ForceClose(slot) => {
                let cmd = match a {
                    Action::Close(_) => "close",
                    _ => "force_close",
                };
                let args = json!({ "path": self.paths[*slot] });
                let result = self.plugin.invoke(cmd, args);
                self.closed(*slot);
                result
            }
            Action::CloseAll => {
                let result = self.plugin.invoke("close_all", json!({}));
                (0..SLOTS).for_each(|s| self.closed(s));
                result
            }
            Action::Write { slot, data, binary } => {
                let path = &self.paths[*slot];
                let result = if *binary {
                    let args = json!({ "value": data, "path": path });
                    self.plugin.invoke("write_binary", args)
                } else {
                    let text = String::from_utf8(data.clone()).unwrap();
                    let args = json!({ "value": text, "path": path });
                    self.plugin.invoke("write", args)
                };
                if result.is_ok() {
                    let got = self.device(*slot).recv_until(|b| b.len() >= data.len());
                    assert_eq!(&got, data, "{a:?}: bytes on the device");
                }
                result
            }
            Action::DeviceSend { slot, data } => {
                self.device(*slot).send(data);
                return None;
            }
            Action::Read { slot, size, binary } => {
                let cmd = if *binary { "read_binary" } else { "read" };
                let path = &self.paths[*slot];
                let args = json!({ "path": path, "timeout": READ_TIMEOUT_MS, "size": size });
                self.plugin.invoke(cmd, args)
            }
            Action::Watch(slot, kind) => {
                let extra = match kind {
                    // `options` keeps its 4.0.0 keys: a `parser` in it is ignored.
                    WatchKind::Plain => json!({ "options": {
                        "routeUrc": false, "parser": { "type": "byteLength", "length": 0 } } }),
                    WatchKind::LongBatch => {
                        json!({ "options": { "serialDataFlushIntervalMs": NEVER_MS } })
                    }
                    WatchKind::Lines => json!({ "parser": { "type": "readline" } }),
                };
                self.watch(*slot, extra)
            }
            Action::WatchBadParser(slot) => {
                let parser = json!({ "type": "byteLength", "length": 0 });
                self.watch(*slot, json!({ "parser": parser }))
            }
            Action::Unwatch(slot) => {
                // No watch: an id nobody holds.
                let id = match m.target {
                    Some(c) => self.chans[c].events.id,
                    None => u32::MAX - *slot as u32,
                };
                self.plugin.invoke("unwatch", json!({ "channelId": id }))
            }
            Action::Set(slot, setting) => {
                let (cmd, key, value) = match *setting {
                    Setting::Baud(b) => ("set_baud_rate", "baudRate", json!(b)),
                    Setting::DataBits(d) => ("set_data_bits", "dataBits", json!(d)),
                    Setting::Flow(f) => ("set_flow_control", "flowControl", json!(f)),
                    Setting::Parity(x) => ("set_parity", "parity", json!(x)),
                    Setting::Stop(x) => ("set_stop_bits", "stopBits", json!(x)),
                    Setting::Timeout(t) => ("set_timeout", "timeout", json!(t)),
                };
                let mut args = json!({ "path": self.paths[*slot] });
                args[key] = value;
                self.plugin.invoke(cmd, args)
            }
            Action::Lines { slot, dtr, level } => {
                let cmd = if *dtr {
                    "write_data_terminal_ready"
                } else {
                    "write_request_to_send"
                };
                let args = json!({ "path": self.paths[*slot], "level": level });
                self.plugin.invoke(cmd, args)
            }
            Action::ReadLine(slot, which) => {
                let cmd = [
                    "read_clear_to_send",
                    "read_data_set_ready",
                    "read_ring_indicator",
                    "read_carrier_detect",
                ][*which];
                let args = json!({ "path": self.paths[*slot] });
                self.plugin.invoke(cmd, args)
            }
            Action::Misc(slot, op) => {
                let path = json!({ "path": self.paths[*slot] });
                let invoke = |cmd: &str| self.plugin.invoke(cmd, path.clone());
                match op {
                    Misc::Clear(buffer) => {
                        let args = json!({ "path": self.paths[*slot], "bufferType": buffer });
                        self.plugin.invoke("clear_buffer", args)
                    }
                    Misc::BytesToWrite => invoke("bytes_to_write"),
                    Misc::CancelRead => invoke("cancel_read"),
                    Misc::CancelExchange => invoke("cancel_exchange"),
                    Misc::Break => invoke("set_break").and(invoke("clear_break")),
                }
            }
            Action::ConfigureAt { slot, timeout_ms } => {
                let session = json!({ "defaultTimeoutMs": timeout_ms,
                    "defaultRxPrepare": "none", "resultFormat": "numeric" });
                let args = json!({ "path": self.paths[*slot], "session": session });
                self.plugin.invoke("configure_at_session", args)
            }
            Action::At(slot) => {
                let numeric = m.ports[*slot].as_ref().is_some_and(|p| p.session);
                let reply: &[u8] = if numeric { b"\r\n0\r\n" } else { b"\r\nOK\r\n" };
                let options = json!({ "timeoutMs": ANSWERED_MS, "rxPrepare": "none" });
                let args =
                    json!({ "path": self.paths[*slot], "command": "AT", "options": options });
                let result = self.answered(*slot, live, reply, "at", args);
                if let Ok(reply) = &result {
                    assert_eq!(reply["status"], "ok", "{a:?}");
                }
                result
            }
            Action::SilentAt(slot) => {
                let args = json!({ "path": self.paths[*slot], "command": "AT", "options": null });
                let result = self.plugin.invoke("at", args);
                assert_eq!(self.device(*slot).command(), b"AT\r", "{a:?}: sent");
                result
            }
            Action::Exchange {
                slot,
                binary,
                reply,
            } => {
                // The device's final line, the status and `matched` the frontend gets.
                let replies: [(&[u8], Name, Value); 4] = [
                    (b"\r\nOK\r\n", "ok", json!("ok")),
                    (b"\r\nERROR\r\n", "error", json!("error")),
                    (
                        b"\r\n+CME ERROR: 10\r\n",
                        "cme",
                        json!({ "cmeError": { "code": 10 } }),
                    ),
                    (
                        b"\r\n+CMS ERROR: 300\r\n",
                        "cms",
                        json!({ "cmsError": { "code": 300 } }),
                    ),
                ];
                let (bytes, status, matched) = replies[*reply].clone();
                let options = json!({ "timeoutMs": ANSWERED_MS, "rxPrepare": "none" });
                let mut args = json!({ "path": self.paths[*slot], "options": options });
                let cmd = if *binary {
                    args["value"] = json!(b"AT\r");
                    "exchange_binary"
                } else {
                    args["value"] = json!("AT\r");
                    "exchange"
                };
                let result = self.answered(*slot, live, bytes, cmd, args);
                if let Ok(reply) = &result {
                    assert_eq!(reply["status"], status, "{a:?}");
                    assert_eq!(reply["matched"], matched, "{a:?}");
                }
                result
            }
            Action::DeviceGone(slot) => {
                self.devices[*slot] = None;
                return None;
            }
        })
    }

    /// The fence comes and goes with the watch; a new watch is a new fence.
    fn set_fences(&mut self, m: &Model, a: &Action) {
        for slot in 0..SLOTS {
            let watched = m.ports[slot]
                .as_ref()
                .is_some_and(|p| p.watch.is_some() && !p.gone);
            let new = matches!(a, Action::Watch(s, _) | Action::Open(s, ..) if *s == slot);
            if !watched {
                self.fences[slot] = None;
            } else if new && self.fences[slot].is_none() {
                self.fences[slot] = Some(self.plugin.fence(&self.paths[slot]));
            }
        }
    }

    fn check_invariants(&mut self, m: &Model, n: usize) {
        let open = |s: &usize| m.ports[*s].is_some();
        let mut managed: Vec<String> = (0..SLOTS)
            .filter(open)
            .map(|s| self.paths[s].clone())
            .collect();
        managed.sort();
        assert_eq!(self.plugin.managed(), managed, "step {n}: managed ports");
        for (slot, (path, port)) in self.paths.iter().zip(&m.ports).enumerate() {
            let Some(port) = port else { continue };
            if port.gone {
                // The hub notices the loss on its next read.
                eventually(&format!("step {n}: {path} released"), || {
                    self.plugin
                        .invoke("bytes_to_read", json!({ "path": path }))
                        .is_err()
                });
            } else {
                self.check_settings(path, port, n);
                match port.watch {
                    Some(c) => self.fences[slot].as_ref().unwrap().wait(m.chans[c].fed),
                    None => self.plugin.settled(path, port.pending.len()),
                }
            }
        }
        for (chan, model) in self.chans.iter_mut().zip(&m.chans) {
            chan.check(model, n);
        }
    }

    /// The settings on the port's read handle.
    fn check_settings(&self, path: &str, port: &PortM, n: usize) {
        fn named<T: DeserializeOwned>(name: Name) -> T {
            serde_json::from_value(json!(name)).unwrap()
        }
        let want = &port.settings;
        let mut data_bits = serialport::DataBits::from(named::<st::DataBits>(want.data_bits));
        let mut parity = serialport::Parity::from(named::<st::Parity>(want.parity));
        let flow = serialport::FlowControl::from(named::<st::FlowControl>(want.flow));
        let stop = serialport::StopBits::from(named::<st::StopBits>(want.stop));
        let pty = port.kind != Kind::Tcp;
        // Linux's pty driver forces CS8 and clears PARENB on every termios change
        // (drivers/tty/pty.c, `pty_set_termios`).
        if pty && cfg!(target_os = "linux") {
            data_bits = serialport::DataBits::Eight;
            parity = serialport::Parity::None;
        }
        with_port(&self.plugin.sp(), path, |p| {
            if !(pty && PTY_KEEPS_BAUD) {
                assert_eq!(p.baud_rate().unwrap(), want.baud, "step {n}: {path}");
            }
            assert_eq!(p.data_bits().unwrap(), data_bits, "step {n}: {path}");
            assert_eq!(p.flow_control().unwrap(), flow, "step {n}: {path}");
            assert_eq!(p.parity().unwrap(), parity, "step {n}: {path}");
            assert_eq!(p.stop_bits().unwrap(), stop, "step {n}: {path}");
        });
    }
}

impl ChanS {
    /// The data that came, and the disconnects after it.
    fn seen(&self) -> (Vec<Vec<u8>>, usize) {
        let mut data = Vec::new();
        let mut disconnects = 0;
        for event in &self.seen {
            match event {
                Event::Data(d) => {
                    assert_eq!(disconnects, 0, "data after the disconnect");
                    data.push(d.clone());
                }
                Event::Disconnect => disconnects += 1,
                other => panic!("{}: {other:?}", self.path),
            }
        }
        (data, disconnects)
    }

    /// Everything the model delivered arrived, and nothing the model did not at least hold.
    fn check(&mut self, model: &ChanM, n: usize) {
        loop {
            while let Some(event) = self.events.try_next(&self.path) {
                self.seen.push(event);
            }
            let (data, disconnects) = self.seen();
            let what = || format!("step {n}: got {data:?} + {disconnects}, model {model:?}");
            let complete = if model.kind == WatchKind::Lines {
                assert!(model.frames.starts_with(&data), "{}", what());
                data.len() == model.frames.len()
            } else {
                let mut full = model.delivered.clone();
                full.extend(&model.held);
                let data = data.concat();
                assert!(full.starts_with(&data), "{}", what());
                data.len() >= model.delivered.len()
            };
            assert!(disconnects <= model.disconnects, "{}", what());
            if complete && disconnects == model.disconnects {
                break;
            }
            let event = self.events.next(&self.path);
            self.seen.push(event);
        }
        assert_eq!(
            tauri_plugin_serialplugin::port::watch_registry::contains(self.events.id),
            model.registered,
            "step {n}: registration of {}",
            self.events.id
        );
    }
}

fn check(what: &str, expect: &Expect, result: Result<Value, Value>) {
    match expect {
        Expect::Either => {}
        Expect::Err(want) => assert_eq!(kind(result), *want, "{what}"),
        Expect::BadArgs => assert!(bad_args(&result), "{what}: {result:?}"),
        _ => {
            let got = result.unwrap_or_else(|e| panic!("{what}: {e}"));
            match expect {
                Expect::Bytes(want) => {
                    let bytes = match got {
                        Value::String(s) => s.into_bytes(),
                        v => serde_json::from_value(v).unwrap(),
                    };
                    assert_eq!(&bytes, want, "{what}");
                }
                Expect::Count(n) => assert_eq!(got, json!(n), "{what}"),
                Expect::Level(level) => assert_eq!(got, json!(level), "{what}"),
                _ => {}
            }
        }
    }
}

fn run_case(actions: &[Action]) {
    let mut sut = Sut::new();
    let mut model = Model::default();
    for (n, a) in actions.iter().enumerate() {
        let Some(expect) = model.step(a) else {
            continue;
        };
        if let Some(result) = sut.apply(&model, &expect, a) {
            check(&format!("step {n}: {a:?}"), &expect, result);
        }
        sut.set_fences(&model, a);
        sut.check_invariants(&model, n);
    }
    // Close everything: every tail has arrived by then and nothing more can.
    model.step(&Action::CloseAll);
    sut.plugin.ok("close_all", json!({}));
    sut.check_invariants(&model, actions.len());
}

#[test]
fn device_plugin_matches_model() {
    let mut runner = TestRunner::new(Config {
        cases: 32,
        failure_persistence: None,
        // A failing case may wait out a bound per replay: report within the nextest limit.
        max_shrink_time: 30_000,
        ..Config::default()
    });
    runner
        .run(&case(), |actions| {
            run_case(&actions);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e}"));
}
