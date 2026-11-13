//! Device model: the plugin through IPC against real devices — PTY pairs (Unix) and
//! `tcp://` bridges to a localhost listener — on two ports at once. After every step
//! the plugin's view is compared with a reference model: managed ports, the settings
//! on the port handle, what each watch received (data, `Disconnect` exactly once, after
//! the data), the watch registry, and the RX bytes waiting for `read`.
//!
//! Watches over IPC check ids and registration; their events go to the webview, which
//! the mock runtime does not keep, so event streams are checked on Rust-side watches
//! (`SerialPort::watch` with a channel into mpsc), the same core path.

use crate::common::*;

const SLOTS: usize = 2;
const READ_TIMEOUT_MS: u64 = 30;
/// serialport on macOS applies termios to a PTY, then fails on `IOSSIOSPEED` (ENOTTY):
/// a setting comes back as an error but is in effect (a baud rate is not applied).
#[cfg(unix)]
const PTY_SET_FAILS: bool = cfg!(any(target_os = "macos", target_os = "ios"));

#[derive(Clone, Copy, Debug, PartialEq)]
enum Kind {
    #[cfg(unix)]
    Pty,
    Tcp,
}

#[derive(Clone, Copy, Debug)]
enum Setting {
    Baud(u32),
    DataBits(&'static str),
    Flow(&'static str),
    Parity(&'static str),
    Stop(&'static str),
    Timeout(u64),
}

#[derive(Clone, Copy, Debug)]
enum Misc {
    Clear(&'static str),
    BytesToWrite,
    CancelRead,
    CancelExchange,
    Break,
}

#[derive(Clone, Copy, Debug)]
enum WatchVia {
    /// The `watch` command over IPC (`parser`: a readline parser as a top-level arg).
    Ipc { parser: bool },
    /// `SerialPort::watch` with a channel into mpsc (`long_batch`: a 60 s window).
    Rust { long_batch: bool },
}

#[derive(Clone, Debug)]
enum Action {
    Open {
        slot: usize,
        kind: Kind,
        baud: u32,
        data_bits: Option<&'static str>,
        flow: Option<&'static str>,
        parity: Option<&'static str>,
        stop: Option<&'static str>,
        dtr: Option<bool>,
        extra: bool,
    },
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
    /// The device sends bytes to the port.
    DeviceSend {
        slot: usize,
        data: Vec<u8>,
    },
    Read {
        slot: usize,
        size: usize,
        binary: bool,
    },
    Watch {
        slot: usize,
        via: WatchVia,
    },
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
    ReadLine {
        slot: usize,
        which: u8,
    },
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
        reply: u8,
    },
    /// The device disappears (PTY master closed / TCP peer dropped).
    DeviceGone(usize),
}

// ---------------------------------------------------------------- model

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    baud: u32,
    data_bits: &'static str,
    flow: &'static str,
    parity: &'static str,
    stop: &'static str,
}

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
    /// An answered `at` showed the session config is in effect.
    probed: bool,
}

#[derive(Clone, Debug, Default)]
struct ChanM {
    long_batch: bool,
    /// Bytes that must have arrived.
    delivered: Vec<u8>,
    /// Bytes held in a long batch window (may arrive any time, must by close).
    buffered: Vec<u8>,
    disconnects: u32,
    registered: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Expect {
    Ok,
    Err,
    /// A PTY may not support the call (modem lines, break): either is fine.
    #[cfg_attr(not(unix), allow(dead_code))]
    Either,
    Bytes(Vec<u8>),
    Count(usize),
    Level(bool),
    /// The `at` fails (no reply within the session timeout).
    Timeout,
}

#[derive(Debug, Default)]
struct Model {
    ports: [Option<PortM>; SLOTS],
    chans: Vec<ChanM>,
    /// The channel the last `Unwatch` stopped.
    target: Option<usize>,
}

impl Model {
    fn live(&mut self, slot: usize) -> Option<&mut PortM> {
        self.ports[slot].as_mut().filter(|p| !p.gone)
    }

    /// Close semantics: the watch's tail arrives, its registration goes, no `Disconnect`.
    fn end_session(&mut self, slot: usize) {
        if let Some(c) = self.ports[slot].as_ref().and_then(|p| p.watch) {
            let chan = &mut self.chans[c];
            let tail = std::mem::take(&mut chan.buffered);
            chan.delivered.extend(tail);
            chan.registered = false;
        }
        self.ports[slot] = None;
    }

    /// What the action should return, or `None` when it means nothing now.
    fn step(&mut self, a: &Action) -> Option<Expect> {
        self.target = None;
        Some(match a {
            Action::Open {
                slot,
                kind,
                baud,
                data_bits,
                flow,
                parity,
                stop,
                ..
            } => {
                // A vanished device cannot be opened again.
                if self.ports[*slot].as_ref().is_some_and(|p| p.gone) {
                    return None;
                }
                // Re-opening an open path replaces the session (same device).
                let kind = self.ports[*slot].as_ref().map_or(*kind, |p| p.kind);
                self.end_session(*slot);
                self.ports[*slot] = Some(PortM {
                    kind,
                    settings: Settings {
                        baud: *baud,
                        data_bits: data_bits.unwrap_or("Eight"),
                        flow: flow.unwrap_or("None"),
                        parity: parity.unwrap_or("None"),
                        stop: stop.unwrap_or("One"),
                    },
                    watch: None,
                    gone: false,
                    pending: Vec::new(),
                    session: false,
                    probed: false,
                });
                Expect::Ok
            }
            Action::OpenBadArgs(_) => Expect::Err,
            Action::Close(slot) => {
                if self.ports[*slot].is_none() {
                    Expect::Err
                } else {
                    self.end_session(*slot);
                    Expect::Ok
                }
            }
            // An unknown path is no error for `force_close`.
            Action::ForceClose(slot) => {
                self.end_session(*slot);
                Expect::Ok
            }
            Action::CloseAll => {
                for s in 0..SLOTS {
                    self.end_session(s);
                }
                Expect::Ok
            }
            Action::Write { slot, data, .. } => match self.live(*slot) {
                Some(_) => Expect::Count(data.len()),
                None => Expect::Err,
            },
            Action::DeviceSend { slot, data } => {
                let p = self.ports[*slot].as_mut().filter(|p| !p.gone)?;
                match p.watch {
                    Some(c) if self.chans[c].long_batch => self.chans[c].buffered.extend(data),
                    Some(c) => self.chans[c].delivered.extend(data),
                    None => p.pending.extend(data),
                }
                Expect::Ok
            }
            Action::Read { slot, size, binary } => {
                match self.live(*slot).filter(|p| p.watch.is_none()) {
                    Some(p) if p.pending.is_empty() => Expect::Err,
                    Some(p) => {
                        // `read` returns what is there; `read_binary` fills up to `size`
                        // or returns what came within the timeout.
                        let n = if *binary && p.pending.len() < *size {
                            p.pending.len()
                        } else {
                            (*size).min(p.pending.len())
                        };
                        Expect::Bytes(p.pending.drain(..n).collect())
                    }
                    None => Expect::Err,
                }
            }
            Action::Watch { slot, via } => {
                let c = self.chans.len();
                self.chans.push(ChanM {
                    long_batch: matches!(via, WatchVia::Rust { long_batch: true }),
                    ..Default::default()
                });
                match self.ports[*slot].as_mut() {
                    Some(p) if p.watch.is_none() => {
                        p.watch = Some(c);
                        // Attaching drops RX nobody asked for.
                        p.pending.clear();
                        self.chans[c].registered = true;
                        // A released port hands a late watch its `Disconnect` (4.x).
                        if p.gone {
                            self.chans[c].disconnects = 1;
                        }
                        Expect::Ok
                    }
                    _ => Expect::Err,
                }
            }
            Action::WatchBadParser(_) => {
                self.chans.push(ChanM::default());
                Expect::Err
            }
            Action::Unwatch(slot) => {
                match self.ports[*slot].as_mut().and_then(|p| p.watch.take()) {
                    Some(c) => {
                        let chan = &mut self.chans[c];
                        let tail = std::mem::take(&mut chan.buffered);
                        chan.delivered.extend(tail);
                        chan.registered = false;
                        self.target = Some(c);
                        Expect::Ok
                    }
                    None => Expect::Err,
                }
            }
            Action::Set(slot, setting) => match self.live(*slot) {
                Some(p) => {
                    #[cfg(unix)]
                    let pty_fails = PTY_SET_FAILS && p.kind == Kind::Pty;
                    #[cfg(not(unix))]
                    let pty_fails = false;
                    let s = &mut p.settings;
                    match *setting {
                        Setting::Baud(_) if pty_fails => {}
                        Setting::Baud(b) => s.baud = b,
                        Setting::DataBits(d) => s.data_bits = d,
                        Setting::Flow(f) => s.flow = f,
                        Setting::Parity(x) => s.parity = x,
                        Setting::Stop(x) => s.stop = x,
                        Setting::Timeout(_) => {}
                    }
                    if pty_fails && !matches!(setting, Setting::Timeout(_)) {
                        Expect::Err
                    } else {
                        Expect::Ok
                    }
                }
                None => Expect::Err,
            },
            Action::Lines { slot, .. } => match self.live(*slot).map(|p| p.kind) {
                Some(Kind::Tcp) => Expect::Ok,
                #[cfg(unix)]
                Some(Kind::Pty) => Expect::Either,
                None => Expect::Err,
            },
            Action::ReadLine { slot, which } => match self.live(*slot).map(|p| p.kind) {
                // A connected raw bridge: CTS, DSR, CD asserted, no ring.
                Some(Kind::Tcp) => Expect::Level(*which != 2),
                #[cfg(unix)]
                Some(Kind::Pty) => Expect::Either,
                None => Expect::Err,
            },
            Action::Misc(slot, op) => match self.live(*slot) {
                Some(p) => {
                    if matches!(op, Misc::Clear("Input" | "All")) {
                        p.pending.clear();
                    }
                    match (op, p.kind) {
                        #[cfg(unix)]
                        (Misc::Break, Kind::Pty) => Expect::Either,
                        _ => Expect::Ok,
                    }
                }
                None => Expect::Err,
            },
            Action::ConfigureAt { slot, .. } => match self.live(*slot) {
                Some(p) => {
                    p.session = true;
                    p.probed = false;
                    Expect::Ok
                }
                None => Expect::Err,
            },
            Action::At(slot) => match self.live(*slot) {
                Some(p) => {
                    // Idle RX is replayed into the exchange (rxPrepare none).
                    p.pending.clear();
                    p.probed = p.session;
                    Expect::Ok
                }
                None => Expect::Err,
            },
            // Only once an answered `at` proved the session timeout is configured:
            // otherwise a silent device would wait for the 5 s default.
            Action::SilentAt(slot) => {
                let p = self.live(*slot).filter(|p| p.probed)?;
                p.pending.clear();
                Expect::Timeout
            }
            Action::Exchange { slot, .. } => match self.live(*slot) {
                Some(p) => {
                    p.pending.clear();
                    Expect::Ok
                }
                None => Expect::Err,
            },
            Action::DeviceGone(slot) => {
                let p = self.live(*slot)?;
                p.gone = true;
                p.pending.clear();
                p.session = false;
                if let Some(c) = p.watch {
                    let chan = &mut self.chans[c];
                    let tail = std::mem::take(&mut chan.buffered);
                    chan.delivered.extend(tail);
                    chan.disconnects += 1;
                }
                Expect::Ok
            }
        })
    }
}

// ---------------------------------------------------------------- strategies

fn kind() -> BoxedStrategy<Kind> {
    #[cfg(unix)]
    return prop_oneof![Just(Kind::Pty), Just(Kind::Tcp)].boxed();
    #[cfg(not(unix))]
    Just(Kind::Tcp).boxed()
}

fn pick(values: &'static [&'static str]) -> impl Strategy<Value = &'static str> {
    proptest::sample::select(values)
}

const DATA_BITS: &[&str] = &["Five", "Six", "Seven", "Eight"];
const FLOW: &[&str] = &["None", "Software", "Hardware"];
const PARITY: &[&str] = &["None", "Odd", "Even"];
const STOP: &[&str] = &["One", "Two"];

fn baud() -> impl Strategy<Value = u32> {
    prop_oneof![Just(9600u32), Just(19_200), Just(115_200)]
}

/// Letters and digits: no XON/XOFF, CR/LF or high bit for the PTY line discipline.
fn payload() -> impl Strategy<Value = Vec<u8>> {
    proptest::collection::vec(prop_oneof![b'a'..=b'z', b'A'..=b'Z', b'0'..=b'9'], 1..16)
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

fn action() -> impl Strategy<Value = Action> {
    let slot = || 0..SLOTS;
    prop_oneof![
        5 => (
            slot(),
            kind(),
            baud(),
            proptest::option::of(pick(DATA_BITS)),
            proptest::option::of(pick(FLOW)),
            proptest::option::of(pick(PARITY)),
            proptest::option::of(pick(STOP)),
            proptest::option::of(any::<bool>()),
            any::<bool>(),
        )
            .prop_map(|(slot, kind, baud, data_bits, flow, parity, stop, dtr, extra)| {
                Action::Open { slot, kind, baud, data_bits, flow, parity, stop, dtr, extra }
            }),
        1 => (0u8..3).prop_map(Action::OpenBadArgs),
        3 => slot().prop_map(Action::Close),
        1 => slot().prop_map(Action::ForceClose),
        1 => Just(Action::CloseAll),
        4 => (slot(), payload(), any::<bool>())
            .prop_map(|(slot, data, binary)| Action::Write { slot, data, binary }),
        6 => (slot(), payload()).prop_map(|(slot, data)| Action::DeviceSend { slot, data }),
        3 => (slot(), 1usize..24, any::<bool>())
            .prop_map(|(slot, size, binary)| Action::Read { slot, size, binary }),
        4 => (
            slot(),
            prop_oneof![
                any::<bool>().prop_map(|parser| WatchVia::Ipc { parser }),
                any::<bool>().prop_map(|long_batch| WatchVia::Rust { long_batch }),
            ],
        )
            .prop_map(|(slot, via)| Action::Watch { slot, via }),
        1 => slot().prop_map(Action::WatchBadParser),
        2 => slot().prop_map(Action::Unwatch),
        4 => (slot(), setting()).prop_map(|(s, x)| Action::Set(s, x)),
        1 => (slot(), any::<bool>(), any::<bool>())
            .prop_map(|(slot, dtr, level)| Action::Lines { slot, dtr, level }),
        1 => (slot(), 0u8..4).prop_map(|(slot, which)| Action::ReadLine { slot, which }),
        2 => (slot(), misc()).prop_map(|(s, m)| Action::Misc(s, m)),
        2 => (slot(), prop_oneof![Just(80u64), Just(120)])
            .prop_map(|(slot, timeout_ms)| Action::ConfigureAt { slot, timeout_ms }),
        3 => slot().prop_map(Action::At),
        1 => slot().prop_map(Action::SilentAt),
        2 => (slot(), any::<bool>(), 0u8..4)
            .prop_map(|(slot, binary, reply)| Action::Exchange { slot, binary, reply }),
        1 => slot().prop_map(Action::DeviceGone),
    ]
}

// ---------------------------------------------------------------- system under test

enum Device {
    #[cfg(unix)]
    Pty {
        master: serialport::TTYPort,
        /// Cloned into the plugin by `insert_port`.
        slave: serialport::TTYPort,
    },
    Tcp {
        listener: TcpListener,
        /// The plugin's connection, accepted after `open`.
        peer: Option<TcpStream>,
    },
}

impl Device {
    fn new(kind: Kind) -> (Self, String) {
        match kind {
            #[cfg(unix)]
            Kind::Pty => {
                let (master, slave, path) = pty();
                (Device::Pty { master, slave }, path)
            }
            Kind::Tcp => {
                let (listener, path) = tcp_listener();
                (
                    Device::Tcp {
                        listener,
                        peer: None,
                    },
                    path,
                )
            }
        }
    }

    fn io(&mut self) -> &mut dyn Io {
        match self {
            #[cfg(unix)]
            Device::Pty { master, .. } => master,
            Device::Tcp { peer, .. } => peer.as_mut().expect("plugin connected"),
        }
    }

    fn clone_io(&self) -> Box<dyn Io> {
        match self {
            #[cfg(unix)]
            Device::Pty { master, .. } => Box::new(master.try_clone_native().unwrap()),
            Device::Tcp { peer, .. } => Box::new(peer.as_ref().unwrap().try_clone().unwrap()),
        }
    }
}

struct ChanS {
    id: u32,
    events: Option<Receiver<SerialEvent>>,
    seen: Vec<SerialEvent>,
}

struct Sut {
    plugin: Plugin,
    case: usize,
    devices: [Option<Device>; SLOTS],
    paths: [String; SLOTS],
    /// The slave of a vanished PTY, kept until its port is closed so the next pair
    /// cannot reuse its name while the path is still managed.
    #[cfg(unix)]
    remains: [Option<serialport::TTYPort>; SLOTS],
    chans: Vec<ChanS>,
}

impl Sut {
    fn synthetic_path(case: usize, slot: usize) -> String {
        format!("/dev/case{case}_slot{slot}")
    }

    /// The port of `slot` is closed: its device goes, and the slot gets a path no
    /// device has (a freed PTY name or TCP port may be handed out again).
    fn closed(&mut self, slot: usize) {
        self.devices[slot] = None;
        #[cfg(unix)]
        {
            self.remains[slot] = None;
        }
        self.paths[slot] = Self::synthetic_path(self.case, slot);
    }

    fn invoke(&self, cmd: &str, args: Value) -> Result<Value, Value> {
        self.plugin.invoke(cmd, args)
    }
}

fn check(what: &str, expect: &Expect, res: &Result<Value, Value>) {
    match (expect, res) {
        (Expect::Either, _) => {}
        (Expect::Err | Expect::Timeout, Err(_)) => {}
        (Expect::Err | Expect::Timeout, Ok(v)) => panic!("{what}: expected an error, got {v}"),
        (_, Err(e)) => panic!("{what}: unexpected error {e}"),
        (Expect::Bytes(want), Ok(v)) => {
            let got: Vec<u8> = match v {
                Value::String(s) => s.clone().into_bytes(),
                v => serde_json::from_value(v.clone()).expect("byte array"),
            };
            assert_eq!(&got, want, "{what}: bytes read");
        }
        (Expect::Count(n), Ok(v)) => assert_eq!(v, &json!(n), "{what}: byte count"),
        (Expect::Level(l), Ok(v)) => assert_eq!(v, &json!(l), "{what}: line level"),
        (Expect::Ok, Ok(_)) => {}
    }
}

fn at_reply(session: bool) -> &'static [u8] {
    if session {
        b"\r\n0\r\n"
    } else {
        b"\r\nOK\r\n"
    }
}

fn apply(sut: &mut Sut, m: &Model, e: &Expect, a: &Action) {
    let what = format!("{a:?}");
    let sp = sut.plugin.sp();
    let path = |sut: &Sut, s: usize| sut.paths[s].clone();
    let res = match a {
        Action::Open {
            slot,
            kind,
            baud,
            data_bits,
            flow,
            parity,
            stop,
            dtr,
            extra,
        } => {
            let slot = *slot;
            if sut.devices[slot].is_none() {
                let (dev, path) = Device::new(*kind);
                sut.devices[slot] = Some(dev);
                sut.paths[slot] = path;
            }
            let p = path(sut, slot);
            let res = match sut.devices[slot].as_ref().unwrap() {
                // Plugging the PTY in (no IPC way on macOS), with the open arguments set
                // on the device; macOS reports an error after applying them.
                #[cfg(unix)]
                Device::Pty { slave, .. } => {
                    use serialport::SerialPort as _;
                    let mut port = slave.try_clone_native().unwrap();
                    if !PTY_SET_FAILS {
                        port.set_baud_rate(*baud).unwrap();
                    }
                    let de = |v: Option<&str>, d: &str| json!(v.unwrap_or(d));
                    let _ = port.set_data_bits(
                        serde_json::from_value::<tauri_plugin_serialplugin::state::DataBits>(
                            de(*data_bits, "Eight"),
                        )
                        .unwrap()
                        .into(),
                    );
                    let _ = port.set_flow_control(
                        serde_json::from_value::<tauri_plugin_serialplugin::state::FlowControl>(
                            de(*flow, "None"),
                        )
                        .unwrap()
                        .into(),
                    );
                    let _ = port.set_parity(
                        serde_json::from_value::<tauri_plugin_serialplugin::state::Parity>(de(
                            *parity, "None",
                        ))
                        .unwrap()
                        .into(),
                    );
                    let _ = port.set_stop_bits(
                        serde_json::from_value::<tauri_plugin_serialplugin::state::StopBits>(de(
                            *stop, "One",
                        ))
                        .unwrap()
                        .into(),
                    );
                    port.set_timeout(Duration::from_millis(200)).unwrap();
                    sp.insert_port(p.clone(), Box::new(port))
                        .map(|()| json!(p))
                        .map_err(|e| json!(e.to_string()))
                }
                Device::Tcp { .. } => {
                    let mut args = json!({ "path": p, "baudRate": baud, "timeout": 200 });
                    for (k, v) in [
                        ("dataBits", data_bits),
                        ("flowControl", flow),
                        ("parity", parity),
                        ("stopBits", stop),
                    ] {
                        if let Some(v) = v {
                            args[k] = json!(v);
                        }
                    }
                    args["dtr"] = json!(dtr);
                    if *extra {
                        args["zzUnknownKey"] = json!(true);
                    }
                    sut.invoke("open", args)
                }
            };
            assert_eq!(res, Ok(json!(p)), "{what}: open returns the session path");
            // Without PTYs (Windows) every device is a TCP bridge.
            #[allow(irrefutable_let_patterns)]
            if let Device::Tcp { listener, peer } = sut.devices[slot].as_mut().unwrap() {
                // A re-open replaces the old connection.
                *peer = Some(accept(listener));
            }
            return;
        }
        Action::OpenBadArgs(which) => {
            let (listener, p) = tcp_listener();
            let args = match which {
                0 => json!({ "path": p, "baudRate": "fast" }),
                1 => json!({ "path": p, "baudRate": 9600, "dataBits": "Nine" }),
                _ => json!({ "path": p }),
            };
            let res = sut.invoke("open", args);
            // Refused before anything connects.
            listener.set_nonblocking(true).unwrap();
            assert!(listener.accept().is_err(), "{what}: connected anyway");
            res
        }
        Action::Close(s) => {
            let res = sut.invoke("close", json!({ "path": path(sut, *s) }));
            sut.closed(*s);
            res
        }
        Action::ForceClose(s) => {
            let res = sut.invoke("force_close", json!({ "path": path(sut, *s) }));
            sut.closed(*s);
            res
        }
        Action::CloseAll => {
            let res = sut.invoke("close_all", json!({}));
            for s in 0..SLOTS {
                sut.closed(s);
            }
            res
        }
        Action::Write { slot, data, binary } => {
            let p = path(sut, *slot);
            let res = if *binary {
                sut.invoke("write_binary", json!({ "value": data, "path": p }))
            } else {
                let text = String::from_utf8(data.clone()).unwrap();
                sut.invoke("write", json!({ "value": text, "path": p }))
            };
            if res.is_ok() && matches!(e, Expect::Count(_)) {
                let dev = sut.devices[*slot].as_mut().unwrap();
                let got = recv_until(dev.io(), |b| b.len() >= data.len());
                assert_eq!(&got, data, "{what}: bytes on the device");
            }
            res
        }
        Action::DeviceSend { slot, data } => {
            let io = sut.devices[*slot].as_mut().unwrap().io();
            io.write_all(data).unwrap();
            io.flush().unwrap();
            return;
        }
        Action::Read { slot, size, binary } => {
            let cmd = if *binary { "read_binary" } else { "read" };
            let args = json!({ "path": path(sut, *slot), "timeout": READ_TIMEOUT_MS, "size": size });
            sut.invoke(cmd, args)
        }
        Action::Watch { slot, via } => {
            let p = path(sut, *slot);
            match via {
                WatchVia::Ipc { parser } => {
                    let (id, channel) = ipc_channel();
                    let mut args = json!({ "path": p, "options": { "routeUrc": false }, "channel": channel });
                    if *parser {
                        args["parser"] = json!({ "type": "readline" });
                    } else {
                        // `options` keeps its 4.0.0 keys: a `parser` in it is ignored.
                        args["options"]["parser"] = json!({ "type": "byteLength", "length": 0 });
                    }
                    sut.chans.push(ChanS {
                        id,
                        events: None,
                        seen: Vec::new(),
                    });
                    let res = sut.invoke("watch", args);
                    if res.is_ok() {
                        assert_eq!(res, Ok(json!(id)), "{what}: returns the channel id");
                    }
                    res
                }
                WatchVia::Rust { long_batch } => {
                    let (channel, rx) = event_channel();
                    let id = channel.id();
                    let options = WatchOptions {
                        serial_data_flush_interval_ms: long_batch.then_some(60_000),
                        ..Default::default()
                    };
                    sut.chans.push(ChanS {
                        id,
                        events: Some(rx),
                        seen: Vec::new(),
                    });
                    match sp.watch(p, options, channel) {
                        Ok(got) => {
                            assert_eq!(got, id, "{what}: returns the channel id");
                            Ok(json!(got))
                        }
                        Err(e) => Err(json!(e.to_string())),
                    }
                }
            }
        }
        Action::WatchBadParser(slot) => {
            let (id, channel) = ipc_channel();
            sut.chans.push(ChanS {
                id,
                events: None,
                seen: Vec::new(),
            });
            sut.invoke(
                "watch",
                json!({ "path": path(sut, *slot), "options": {}, "channel": channel,
                        "parser": { "type": "byteLength", "length": 0 } }),
            )
        }
        Action::Unwatch(slot) => {
            // No watch: an id nobody holds.
            let id = m.target.map_or(u32::MAX - *slot as u32, |c| sut.chans[c].id);
            sut.invoke("unwatch", json!({ "channelId": id }))
        }
        Action::Set(slot, setting) => {
            let p = path(sut, *slot);
            let (cmd, args) = match *setting {
                Setting::Baud(b) => ("set_baud_rate", json!({ "path": p, "baudRate": b })),
                Setting::DataBits(d) => ("set_data_bits", json!({ "path": p, "dataBits": d })),
                Setting::Flow(f) => ("set_flow_control", json!({ "path": p, "flowControl": f })),
                Setting::Parity(x) => ("set_parity", json!({ "path": p, "parity": x })),
                Setting::Stop(x) => ("set_stop_bits", json!({ "path": p, "stopBits": x })),
                Setting::Timeout(t) => ("set_timeout", json!({ "path": p, "timeout": t })),
            };
            sut.invoke(cmd, args)
        }
        Action::Lines { slot, dtr, level } => {
            let cmd = if *dtr {
                "write_data_terminal_ready"
            } else {
                "write_request_to_send"
            };
            sut.invoke(cmd, json!({ "path": path(sut, *slot), "level": level }))
        }
        Action::ReadLine { slot, which } => {
            let cmd = [
                "read_clear_to_send",
                "read_data_set_ready",
                "read_ring_indicator",
                "read_carrier_detect",
            ][*which as usize];
            sut.invoke(cmd, json!({ "path": path(sut, *slot) }))
        }
        Action::Misc(slot, op) => {
            let p = path(sut, *slot);
            match op {
                Misc::Clear(b) => sut.invoke("clear_buffer", json!({ "path": p, "bufferType": b })),
                Misc::BytesToWrite => sut.invoke("bytes_to_write", json!({ "path": p })),
                Misc::CancelRead => sut.invoke("cancel_read", json!({ "path": p })),
                Misc::CancelExchange => sut.invoke("cancel_exchange", json!({ "path": p })),
                Misc::Break => sut
                    .invoke("set_break", json!({ "path": p }))
                    .and(sut.invoke("clear_break", json!({ "path": p }))),
            }
        }
        Action::ConfigureAt { slot, timeout_ms } => sut.invoke(
            "configure_at_session",
            json!({ "path": path(sut, *slot), "session": {
                "defaultTimeoutMs": timeout_ms, "defaultRxPrepare": "none", "resultFormat": "numeric"
            } }),
        ),
        Action::At(slot) => {
            let session = m.ports[*slot].as_ref().is_some_and(|p| p.session);
            let device = sut.devices[*slot]
                .as_ref()
                .filter(|_| *e == Expect::Ok)
                .map(|d| responder(d.clone_io(), at_reply(session)));
            let res = sut.invoke(
                "at",
                json!({ "path": path(sut, *slot), "command": "AT",
                        "options": { "timeoutMs": 1000, "rxPrepare": "none" } }),
            );
            if let Some(device) = device {
                assert_eq!(device.join().unwrap(), b"AT\r", "{what}: command sent");
                let status = res.as_ref().map(|r| r["status"].clone());
                assert_eq!(status, Ok(json!("ok")), "{what}: status");
            }
            res
        }
        Action::SilentAt(slot) => {
            let res = sut.invoke(
                "at",
                json!({ "path": path(sut, *slot), "command": "AT", "options": null }),
            );
            let dev = sut.devices[*slot].as_mut().unwrap();
            let cmd = recv_until(dev.io(), |b| b.contains(&b'\r'));
            assert_eq!(cmd, b"AT\r", "{what}: command sent");
            res
        }
        Action::Exchange {
            slot,
            binary,
            reply,
        } => {
            // The device's final line, the status and `matched` the frontend gets.
            let (text, status, matched): (&'static [u8], _, _) = match reply {
                0 => (b"\r\nOK\r\n", "ok", json!("ok")),
                1 => (b"\r\nERROR\r\n", "error", json!("error")),
                2 => (b"\r\n+CME ERROR: 10\r\n", "cme", json!({ "cmeError": { "code": 10 } })),
                _ => (b"\r\n+CMS ERROR: 300\r\n", "cms", json!({ "cmsError": { "code": 300 } })),
            };
            let device = sut.devices[*slot]
                .as_ref()
                .filter(|_| *e == Expect::Ok)
                .map(|d| responder(d.clone_io(), text));
            let p = path(sut, *slot);
            let options = json!({ "timeoutMs": 1000, "rxPrepare": "none" });
            let res = if *binary {
                sut.invoke("exchange_binary", json!({ "path": p, "value": b"AT\r", "options": options }))
            } else {
                sut.invoke("exchange", json!({ "path": p, "value": "AT\r", "options": options }))
            };
            if let Some(device) = device {
                assert_eq!(device.join().unwrap(), b"AT\r", "{what}: command sent");
                let got = res.as_ref().map(|r| (r["status"].clone(), r["matched"].clone()));
                assert_eq!(got, Ok((json!(status), matched)), "{what}: status, matched");
            }
            res
        }
        Action::DeviceGone(slot) => {
            let dev = sut.devices[*slot].take();
            #[cfg(unix)]
            if let Some(Device::Pty { slave, .. }) = dev {
                sut.remains[*slot] = Some(slave);
            }
            #[cfg(not(unix))]
            drop(dev);
            return;
        }
    };
    check(&what, e, &res);
}

fn check_invariants(sut: &mut Sut, m: &Model, n: usize) {
    let sp = sut.plugin.sp();
    let mut want: Vec<String> = (0..SLOTS)
        .filter(|s| m.ports[*s].is_some())
        .map(|s| sut.paths[s].clone())
        .collect();
    want.sort();
    assert_eq!(sut.plugin.managed(), want, "step {n}: managed ports");

    for (s, port) in m.ports.iter().enumerate() {
        let Some(port) = port else { continue };
        let path = sut.paths[s].clone();
        if port.gone {
            // The hub notices the loss on its next read.
            eventually(&format!("step {n}: port {s} released"), || {
                sut.plugin
                    .invoke("bytes_to_read", json!({ "path": path }))
                    .is_err()
            });
            continue;
        }
        check_settings(&sp, &path, port, n);
        // The device's bytes reached the hub: it holds exactly `pending` for `read`,
        // and the fd has nothing left (`bytes_to_read` counts both).
        let pending = port.pending.len();
        eventually(
            &format!("step {n}: {path} RX settles at {pending} bytes"),
            || {
                hub_buffered(&sp, &path) == pending
                    && sut.plugin.invoke("bytes_to_read", json!({ "path": path }))
                        == Ok(json!(pending))
            },
        );
    }

    assert_eq!(sut.chans.len(), m.chans.len(), "step {n}: channels");
    for (c, chan) in m.chans.iter().enumerate() {
        check_channel(&mut sut.chans[c], chan, c, n);
    }
}

/// Settings on the port's read handle equal the model's.
fn check_settings(sp: &SerialPort<MockRuntime>, path: &str, port: &PortM, n: usize) {
    let got = handle_settings(sp, path);
    let s = &port.settings;
    let as_json = |v: &str| json!(v);
    // macOS keeps the baud rate per handle (the write handle got it); Linux reads termios.
    #[cfg(unix)]
    let baud_shared = port.kind == Kind::Tcp || cfg!(target_os = "linux");
    #[cfg(not(unix))]
    let baud_shared = true;
    if baud_shared {
        assert_eq!(got.baud, s.baud, "step {n}: {path} baud rate");
    }
    let core = |v: &str| as_json(v);
    use tauri_plugin_serialplugin::state as st;
    let db: serialport::DataBits = serde_json::from_value::<st::DataBits>(core(s.data_bits))
        .unwrap()
        .into();
    let fc: serialport::FlowControl = serde_json::from_value::<st::FlowControl>(core(s.flow))
        .unwrap()
        .into();
    let pa: serialport::Parity = serde_json::from_value::<st::Parity>(core(s.parity))
        .unwrap()
        .into();
    let sb: serialport::StopBits = serde_json::from_value::<st::StopBits>(core(s.stop))
        .unwrap()
        .into();
    // Linux's pty driver forces CS8 and clears PARENB on every termios change
    // (drivers/tty/pty.c, `pty_set_termios`): a PTY never keeps other data bits or parity.
    let linux_pty = cfg!(target_os = "linux") && port.kind == Kind::Pty;
    let (db, pa) = if linux_pty {
        (serialport::DataBits::Eight, serialport::Parity::None)
    } else {
        (db, pa)
    };
    assert_eq!(got.data_bits, db, "step {n}: {path} data bits");
    assert_eq!(got.flow, fc, "step {n}: {path} flow control");
    assert_eq!(got.parity, pa, "step {n}: {path} parity");
    assert_eq!(got.stop, sb, "step {n}: {path} stop bits");
}

/// An observed channel received a prefix of the model's stream (at least `delivered`),
/// exactly the expected `Disconnect`s after the data, and nothing else. Every channel
/// is registered exactly when the model says.
fn check_channel(sut: &mut ChanS, chan: &ChanM, c: usize, n: usize) {
    if let Some(rx) = &sut.events {
        let full: Vec<u8> = chan
            .delivered
            .iter()
            .chain(&chan.buffered)
            .copied()
            .collect();
        let deadline = Instant::now() + WAIT;
        loop {
            let mut data = Vec::new();
            let mut disconnects = 0;
            for ev in &sut.seen {
                match ev {
                    SerialEvent::Data { data: d, .. } => {
                        assert_eq!(
                            disconnects, 0,
                            "step {n}: channel {c}: data after Disconnect"
                        );
                        data.extend_from_slice(d);
                    }
                    SerialEvent::Disconnect { .. } => disconnects += 1,
                    other => panic!("step {n}: channel {c}: unexpected {other:?}"),
                }
            }
            let msg = || {
                format!(
                    "step {n}: channel {c}: got {:?} + {disconnects} disconnects, model {chan:?}",
                    String::from_utf8_lossy(&data)
                )
            };
            assert!(
                full.starts_with(&data) && disconnects <= chan.disconnects,
                "{}",
                msg()
            );
            if data.len() >= chan.delivered.len() && disconnects == chan.disconnects {
                break;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(ev) => sut.seen.push(ev),
                Err(_) => panic!("{}", msg()),
            }
        }
        // Whatever else already arrived is checked next step (or at teardown).
        while let Ok(ev) = rx.try_recv() {
            sut.seen.push(ev);
        }
    }
    assert_eq!(
        tauri_plugin_serialplugin::port::watch_registry::contains(sut.id),
        chan.registered,
        "step {n}: channel {c} registration"
    );
}

static CASE: AtomicUsize = AtomicUsize::new(0);

fn run_case(actions: &[Action]) {
    let case = CASE.fetch_add(1, Ordering::Relaxed);
    let mut sut = Sut {
        plugin: Plugin::new(),
        case,
        devices: Default::default(),
        paths: std::array::from_fn(|s| Sut::synthetic_path(case, s)),
        #[cfg(unix)]
        remains: Default::default(),
        chans: Vec::new(),
    };
    let mut model = Model::default();
    for (n, a) in actions.iter().enumerate() {
        let Some(expect) = model.step(a) else {
            continue;
        };
        apply(&mut sut, &model, &expect, a);
        check_invariants(&mut sut, &model, n);
    }
    // Teardown: close everything; every tail arrives, no second `Disconnect`.
    sut.invoke("close_all", json!({})).expect("close_all");
    for s in 0..SLOTS {
        model.end_session(s);
    }
    for (c, chan) in model.chans.iter().enumerate() {
        check_channel(&mut sut.chans[c], chan, c, actions.len());
    }
}

#[test]
fn device_plugin_matches_model() {
    let mut runner = TestRunner::new(Config {
        cases: 8,
        failure_persistence: None,
        // Report within the nextest limit even when shrinking replays slow sequences.
        max_shrink_time: 3_000,
        ..Config::default()
    });
    let result = runner.run(&proptest::collection::vec(action(), 1..24), |actions| {
        run_case(&actions);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}
