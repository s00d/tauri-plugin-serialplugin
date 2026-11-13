//! Model test of `TcpSerialPort` against a real localhost bridge: random bridge sends,
//! port writes, reads (also with an empty buffer or nothing pending), buffer clears,
//! timeouts, settings and the bridge hanging up, checked after every step.

use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serialplugin_core::port::tcp::TcpSerialPort;
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

type Settings = (u32, DataBits, FlowControl, Parity, StopBits);

#[derive(Clone, Debug)]
enum Action {
    /// The bridge sends bytes to the port.
    PeerSend(Vec<u8>),
    /// The port writes; the bridge must get exactly these bytes.
    Write(Vec<u8>),
    Read(usize),
    Clear(ClearBuffer),
    SetTimeout(u64),
    Settings(Settings),
    /// The bridge closes the connection.
    PeerClose,
}

/// What a read returns.
#[derive(Debug, PartialEq)]
enum ReadKind {
    Bytes(Vec<u8>),
    Err(io::ErrorKind),
}

struct Model {
    /// Sent by the bridge, not read or cleared yet (all of it has arrived: the SUT
    /// waits for that).
    pending: Vec<u8>,
    closed: bool,
    timeout_ms: u64,
    settings: Settings,
}

impl Model {
    /// The expected read result for `Read`, `None` for other steps; `Err(())` when
    /// the step is not observable now.
    fn step(&mut self, a: &Action) -> Result<Option<ReadKind>, ()> {
        match a {
            Action::PeerSend(_) | Action::Write(_) | Action::PeerClose if self.closed => {
                return Err(())
            }
            Action::PeerSend(b) => self.pending.extend_from_slice(b),
            Action::Write(_) | Action::Clear(ClearBuffer::Output) => {}
            Action::Read(0) => return Ok(Some(ReadKind::Bytes(Vec::new()))),
            Action::Read(n) => {
                let k = (*n).min(self.pending.len());
                return Ok(Some(match k {
                    // Nothing pending: a closed bridge is EOF, an open one a timeout.
                    0 if self.closed => ReadKind::Err(io::ErrorKind::UnexpectedEof),
                    0 => ReadKind::Err(io::ErrorKind::TimedOut),
                    k => ReadKind::Bytes(self.pending.drain(..k).collect()),
                }));
            }
            Action::Clear(_) => self.pending.clear(),
            Action::SetTimeout(ms) => self.timeout_ms = *ms,
            Action::Settings(s) => self.settings = *s,
            Action::PeerClose => self.closed = true,
        }
        Ok(None)
    }
}

struct Sut {
    port: TcpSerialPort,
    peer: Option<TcpStream>,
}

impl Sut {
    fn new() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let path = format!("tcp://{}", listener.local_addr().unwrap());
        // A short timeout: every read with nothing pending waits it out.
        let port = TcpSerialPort::connect(
            &path,
            115_200,
            DataBits::Eight,
            FlowControl::None,
            Parity::None,
            StopBits::One,
            Duration::from_millis(5),
        )
        .unwrap();
        let (peer, _) = listener.accept().unwrap();
        Self {
            port,
            peer: Some(peer),
        }
    }

    fn step(&mut self, a: &Action, model: &Model) -> Option<ReadKind> {
        match a {
            Action::PeerSend(b) => {
                self.peer.as_mut().unwrap().write_all(b).unwrap();
                let deadline = Instant::now() + Duration::from_secs(3);
                while (self.port.bytes_to_read().unwrap() as usize) < model.pending.len() {
                    assert!(Instant::now() < deadline, "bytes never arrived");
                    std::thread::yield_now();
                }
            }
            Action::Write(b) => {
                assert_eq!(self.port.write(b).unwrap(), b.len());
                let peer = self.peer.as_mut().unwrap();
                peer.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
                let mut got = vec![0; b.len()];
                peer.read_exact(&mut got).unwrap();
                assert_eq!(&got, b);
            }
            Action::Read(n) => {
                let mut buf = vec![0; *n];
                return Some(match self.port.read(&mut buf) {
                    Ok(k) => ReadKind::Bytes(buf[..k].to_vec()),
                    Err(e) => ReadKind::Err(e.kind()),
                });
            }
            Action::Clear(which) => self.port.clear(*which).unwrap(),
            Action::SetTimeout(ms) => self.port.set_timeout(Duration::from_millis(*ms)).unwrap(),
            Action::Settings((b, d, f, p, s)) => {
                self.port.set_baud_rate(*b).unwrap();
                self.port.set_data_bits(*d).unwrap();
                self.port.set_flow_control(*f).unwrap();
                self.port.set_parity(*p).unwrap();
                self.port.set_stop_bits(*s).unwrap();
            }
            Action::PeerClose => drop(self.peer.take()),
        }
        None
    }

    fn check(&self, m: &Model) {
        assert_eq!(self.port.bytes_to_read().unwrap() as usize, m.pending.len());
        assert_eq!(self.port.bytes_to_write().unwrap(), 0);
        let (b, d, f, p, s) = m.settings;
        // A clone shares the socket and keeps settings and timeout.
        let mut clone = self.port.try_clone().unwrap();
        for port in [&self.port as &dyn SerialPort, clone.as_ref()] {
            assert_eq!(port.baud_rate().unwrap(), b);
            assert_eq!(port.data_bits().unwrap(), d);
            assert_eq!(port.flow_control().unwrap(), f);
            assert_eq!(port.parity().unwrap(), p);
            assert_eq!(port.stop_bits().unwrap(), s);
            assert_eq!(port.timeout(), Duration::from_millis(m.timeout_ms));
        }
        // A raw bridge: the lines read as a connected cable.
        assert!(clone.read_clear_to_send().unwrap());
        assert!(clone.read_data_set_ready().unwrap());
        assert!(!clone.read_ring_indicator().unwrap());
        assert!(clone.read_carrier_detect().unwrap());
        clone.write_request_to_send(true).unwrap();
        clone.write_data_terminal_ready(false).unwrap();
        clone.set_break().unwrap();
        clone.clear_break().unwrap();
        clone.flush().unwrap();
    }
}

fn action() -> impl Strategy<Value = Action> {
    let bytes = || vec(any::<u8>(), 1..16);
    let clear = prop::sample::select(vec![
        ClearBuffer::Input,
        ClearBuffer::Output,
        ClearBuffer::All,
    ]);
    let settings = (
        any::<u32>(),
        prop::sample::select(vec![
            DataBits::Five,
            DataBits::Six,
            DataBits::Seven,
            DataBits::Eight,
        ]),
        prop::sample::select(vec![
            FlowControl::None,
            FlowControl::Software,
            FlowControl::Hardware,
        ]),
        prop::sample::select(vec![Parity::None, Parity::Odd, Parity::Even]),
        prop::sample::select(vec![StopBits::One, StopBits::Two]),
    );
    prop_oneof![
        4 => bytes().prop_map(Action::PeerSend),
        3 => bytes().prop_map(Action::Write),
        4 => (0usize..20).prop_map(Action::Read),
        2 => clear.prop_map(Action::Clear),
        1 => (1u64..8).prop_map(Action::SetTimeout),
        1 => settings.prop_map(Action::Settings),
        1 => Just(Action::PeerClose),
    ]
}

#[test]
fn tcp_port_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 24,
        failure_persistence: None,
        ..Config::default()
    });
    let result = runner.run(&vec(action(), 1..24), |actions| {
        let mut model = Model {
            pending: Vec::new(),
            closed: false,
            timeout_ms: 5,
            settings: (
                115_200,
                DataBits::Eight,
                FlowControl::None,
                Parity::None,
                StopBits::One,
            ),
        };
        let mut sut = Sut::new();
        for a in &actions {
            let Ok(want) = model.step(a) else { continue };
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
