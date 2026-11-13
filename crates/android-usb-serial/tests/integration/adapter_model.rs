#![cfg(feature = "serialport-compat")]

//! `SerialPortAdapter` model over `FakeTransport` (CP2102): reads, writes,
//! settings, modem lines, clears, unplug and shutdown across clones, checked
//! after every step against a model of the ring buffer plus the driver model's
//! control transfers (`common::Model`).

use super::common::*;
use android_usb_serial::serialport_compat::{
    flow_from_serialport, line_config_from_serialport, SerialPortAdapter,
};
use serialport::{
    ClearBuffer, DataBits as SpDataBits, FlowControl as SpFlowControl, Parity as SpParity,
    SerialPort, StopBits as SpStopBits,
};
use std::collections::VecDeque;
use std::io::{ErrorKind, Read, Write};

#[derive(Debug, Clone)]
pub enum Act {
    Baud(u32),
    Data(SpDataBits),
    Parity(SpParity),
    Stop(SpStopBits),
    Flow(SpFlowControl),
    Dtr(bool),
    Rts(bool),
    Break(bool),
    /// Read CTS / DSR / RI / CD (0..4) with this control-IN reply.
    Line(u8, Vec<u8>),
    Write(Vec<u8>),
    PushRx(Vec<u8>),
    Read(usize),
    Clear(ClearBuffer),
    Unplug,
    Shutdown,
    Clone,
    /// Act through clone `n` (modulo the clones made so far).
    Use(usize),
}

struct AModel {
    drv: Model,
    line: LineConfig,
    flow: FlowControl,
    ring: VecDeque<u8>,
    /// Pushed bytes the reader has not handed over yet (at most one chunk).
    in_flight: Option<Vec<u8>>,
    unplugged: bool,
    shut: bool,
    chunk: usize,
}

fn sp_flow(f: FlowControl) -> SpFlowControl {
    match f {
        FlowControl::None => SpFlowControl::None,
        FlowControl::RtsCts | FlowControl::DtrDsr => SpFlowControl::Hardware,
        FlowControl::XonXoff | FlowControl::XonXoffInline => SpFlowControl::Software,
    }
}

fn check_getters(p: &SerialPortAdapter, m: &AModel) -> Result<(), TestCaseError> {
    let l = m.line;
    prop_assert_eq!(p.baud_rate().unwrap(), l.baud_rate);
    let data = match l.data_bits {
        DataBits::Five => SpDataBits::Five,
        DataBits::Six => SpDataBits::Six,
        DataBits::Seven => SpDataBits::Seven,
        DataBits::Eight => SpDataBits::Eight,
    };
    prop_assert_eq!(p.data_bits().unwrap(), data);
    let parity = match l.parity {
        Parity::Odd => SpParity::Odd,
        Parity::Even => SpParity::Even,
        Parity::None | Parity::Mark | Parity::Space => SpParity::None,
    };
    prop_assert_eq!(p.parity().unwrap(), parity);
    let stop = if l.stop_bits == StopBits::Two {
        SpStopBits::Two
    } else {
        SpStopBits::One
    };
    prop_assert_eq!(p.stop_bits().unwrap(), stop);
    prop_assert_eq!(p.flow_control().unwrap(), sp_flow(m.flow));
    prop_assert_eq!(p.bytes_to_read().unwrap() as usize, m.ring.len());
    prop_assert_eq!(p.bytes_to_write().unwrap(), 0);
    prop_assert_eq!(p.name(), Some("usb#0".to_string()));
    Ok(())
}

/// Adapter results compare by kind only: every driver failure surfaces as
/// `serialport::ErrorKind::Unknown`; the text is never compared.
fn sp_kind<T>(r: &serialport::Result<T>) -> Result<(), serialport::ErrorKind> {
    r.as_ref().map(|_| ()).map_err(|e| e.kind)
}

fn exp_kind(e: Exp) -> Result<(), serialport::ErrorKind> {
    if e == Exp::Ok {
        Ok(())
    } else {
        Err(serialport::ErrorKind::Unknown)
    }
}

/// Steps the model cannot predict are skipped: a second chunk while one is in
/// flight (the reader could merge them), a clear or an unplug that could race
/// an in-flight chunk, data after an unplug.
fn observable(m: &AModel, a: &Act) -> bool {
    match a {
        Act::PushRx(_) => m.in_flight.is_none() && !m.unplugged && !m.shut,
        Act::Clear(_) => m.in_flight.is_none(),
        Act::Unplug => m.in_flight.is_none() && !m.unplugged,
        _ => true,
    }
}

/// Apply `next` as the adapter does: the driver sees it, and only an accepted
/// configuration becomes the adapter's.
fn apply_line(m: &mut AModel, next: LineConfig) -> Exp {
    if m.shut {
        return Exp::Io;
    }
    let want = m.drv.line(&next);
    if want == Exp::Ok {
        m.line = next;
    }
    want
}

fn step(
    ports: &mut Vec<SerialPortAdapter>,
    cur: &mut usize,
    fake: &FakeTransport,
    m: &mut AModel,
    a: &Act,
) -> Result<Option<()>, TestCaseError> {
    if !observable(m, a) {
        return Ok(None);
    }
    let p = &mut ports[*cur];
    match a {
        Act::Baud(b) => {
            let want = apply_line(
                m,
                LineConfig {
                    baud_rate: *b,
                    ..m.line
                },
            );
            prop_assert_eq!(sp_kind(&p.set_baud_rate(*b)), exp_kind(want));
        }
        Act::Data(d) => {
            let cfg =
                line_config_from_serialport(m.line.baud_rate, *d, SpParity::None, SpStopBits::One);
            let want = apply_line(
                m,
                LineConfig {
                    data_bits: cfg.data_bits,
                    ..m.line
                },
            );
            prop_assert_eq!(sp_kind(&p.set_data_bits(*d)), exp_kind(want));
        }
        Act::Parity(par) => {
            let cfg = line_config_from_serialport(0, SpDataBits::Eight, *par, SpStopBits::One);
            let want = apply_line(
                m,
                LineConfig {
                    parity: cfg.parity,
                    ..m.line
                },
            );
            prop_assert_eq!(sp_kind(&p.set_parity(*par)), exp_kind(want));
        }
        Act::Stop(s) => {
            let cfg = line_config_from_serialport(0, SpDataBits::Eight, SpParity::None, *s);
            let want = apply_line(
                m,
                LineConfig {
                    stop_bits: cfg.stop_bits,
                    ..m.line
                },
            );
            prop_assert_eq!(sp_kind(&p.set_stop_bits(*s)), exp_kind(want));
        }
        Act::Flow(f) => {
            let flow = flow_from_serialport(*f);
            let want = if m.shut { Exp::Io } else { m.drv.flow(flow) };
            if want == Exp::Ok {
                m.flow = flow;
            }
            prop_assert_eq!(sp_kind(&p.set_flow_control(*f)), exp_kind(want));
        }
        Act::Dtr(v) => {
            let want = if m.shut {
                Exp::Io
            } else {
                m.drv.line_bit(true, *v)
            };
            prop_assert_eq!(sp_kind(&p.write_data_terminal_ready(*v)), exp_kind(want));
        }
        Act::Rts(v) => {
            let want = if m.shut {
                Exp::Io
            } else {
                m.drv.line_bit(false, *v)
            };
            prop_assert_eq!(sp_kind(&p.write_request_to_send(*v)), exp_kind(want));
        }
        Act::Break(on) => {
            let want = if m.shut { Exp::Io } else { m.drv.brk(*on, &[]) };
            let got = if *on { p.set_break() } else { p.clear_break() };
            prop_assert_eq!(sp_kind(&got), exp_kind(want));
        }
        Act::Line(which, reply) => {
            let (want, lines) = if m.shut {
                (Exp::Io, None)
            } else {
                fake.script_control_in_response(reply.clone());
                m.drv.modem(reply)
            };
            let got = match which {
                0 => p.read_clear_to_send(),
                1 => p.read_data_set_ready(),
                2 => p.read_ring_indicator(),
                _ => p.read_carrier_detect(),
            };
            prop_assert_eq!(sp_kind(&got), exp_kind(want));
            let bit =
                lines.map(|(cts, dsr, ri, cd)| [cts, dsr, ri, cd][usize::from(*which).min(3)]);
            prop_assert_eq!(got.ok(), bit);
        }
        Act::Write(data) => {
            let got = p.write(data);
            if data.is_empty() {
                prop_assert_eq!(got.ok(), Some(0));
            } else if m.shut {
                prop_assert_eq!(got.map_err(|e| e.kind()).err(), Some(ErrorKind::BrokenPipe));
            } else {
                prop_assert_eq!(got.ok(), Some(data.len()));
                for c in data.chunks(m.chunk) {
                    m.drv.bulk_out.push((0x02, c.to_vec()));
                }
            }
        }
        Act::PushRx(data) => {
            // One chunk in flight at a time: the reader hands it over whole.
            fake.push_rx(data);
            m.in_flight = Some(data.clone());
        }
        Act::Read(n) => {
            let mut buf = vec![0u8; *n];
            let expect_now = *n == 0 || m.shut || !m.ring.is_empty();
            let waits = !expect_now && (m.in_flight.is_some() || m.unplugged);
            p.set_timeout(Duration::from_secs(if waits { 5 } else { 0 }))
                .unwrap();
            let got = p
                .read(&mut buf)
                .map(|k| buf[..k].to_vec())
                .map_err(|e| e.kind());
            let want = if *n == 0 {
                Ok(vec![])
            } else if m.shut {
                Err(ErrorKind::BrokenPipe)
            } else {
                if m.ring.is_empty() {
                    if let Some(c) = m.in_flight.take() {
                        m.ring.extend(c);
                    }
                }
                if m.ring.is_empty() && m.unplugged {
                    Err(ErrorKind::BrokenPipe)
                } else {
                    let k = (*n).min(m.ring.len());
                    Ok(m.ring.drain(..k).collect())
                }
            };
            prop_assert_eq!(got, want);
        }
        Act::Clear(kind) => {
            if !m.shut && matches!(kind, ClearBuffer::Input | ClearBuffer::All) {
                m.ring.clear();
            }
            let want = if m.shut {
                Exp::Io
            } else {
                m.drv.purge(match kind {
                    ClearBuffer::Input => PurgeKind::Rx,
                    ClearBuffer::Output => PurgeKind::Tx,
                    ClearBuffer::All => PurgeKind::Both,
                })
            };
            prop_assert_eq!(sp_kind(&p.clear(*kind)), exp_kind(want));
        }
        Act::Unplug => {
            fake.inject_bulk_read_error("unplugged");
            m.unplugged = true;
        }
        Act::Shutdown => {
            p.shutdown();
            if !m.shut {
                m.drv.close();
            }
            m.shut = true;
            m.ring.clear();
            prop_assert!(
                fake.open_bulk_in(0x81, 64).is_ok(),
                "shutdown frees bulk IN"
            );
        }
        Act::Clone => {
            let c = p.try_clone().unwrap();
            drop(c);
            let clone = ports[*cur].clone();
            ports.push(clone);
        }
        Act::Use(n) => *cur = n % ports.len(),
    }
    Ok(Some(()))
}

fn act() -> impl Strategy<Value = Act> {
    let baud = prop_oneof![Just(0u32), Just(9600), Just(115_200), 1u32..4_000_000];
    let data = prop::sample::select(vec![
        SpDataBits::Five,
        SpDataBits::Six,
        SpDataBits::Seven,
        SpDataBits::Eight,
    ]);
    let parity = prop::sample::select(vec![SpParity::None, SpParity::Odd, SpParity::Even]);
    let stop = prop::sample::select(vec![SpStopBits::One, SpStopBits::Two]);
    let flow = prop::sample::select(vec![
        SpFlowControl::None,
        SpFlowControl::Hardware,
        SpFlowControl::Software,
    ]);
    let clear = prop::sample::select(vec![
        ClearBuffer::Input,
        ClearBuffer::Output,
        ClearBuffer::All,
    ]);
    prop_oneof![
        2 => baud.prop_map(Act::Baud).boxed(),
        1 => data.prop_map(Act::Data).boxed(),
        1 => parity.prop_map(Act::Parity).boxed(),
        1 => stop.prop_map(Act::Stop).boxed(),
        1 => flow.prop_map(Act::Flow).boxed(),
        1 => any::<bool>().prop_map(Act::Dtr).boxed(),
        1 => any::<bool>().prop_map(Act::Rts).boxed(),
        1 => any::<bool>().prop_map(Act::Break).boxed(),
        1 => (0u8..4, prop::collection::vec(any::<u8>(), 0..2)).prop_map(|(w, r)| Act::Line(w, r)).boxed(),
        2 => prop::collection::vec(any::<u8>(), 0..1500).prop_map(Act::Write).boxed(),
        3 => prop::collection::vec(any::<u8>(), 1..=256).prop_map(Act::PushRx).boxed(),
        4 => (0usize..300).prop_map(Act::Read).boxed(),
        1 => clear.prop_map(Act::Clear).boxed(),
        1 => Just(Act::Unplug).boxed(),
        1 => Just(Act::Shutdown).boxed(),
        1 => Just(Act::Clone).boxed(),
        1 => any::<usize>().prop_map(Act::Use).boxed(),
    ]
}

fn initial_line() -> impl Strategy<Value = (LineConfig, FlowControl)> {
    (
        prop::sample::select(vec![9600u32, 115_200]),
        prop::sample::select(vec![DataBits::Seven, DataBits::Eight]),
        prop::sample::select(vec![Parity::None, Parity::Odd, Parity::Mark, Parity::Space]),
        prop::sample::select(vec![StopBits::One, StopBits::OnePointFive, StopBits::Two]),
        prop::sample::select(vec![
            FlowControl::None,
            FlowControl::RtsCts,
            FlowControl::DtrDsr,
            FlowControl::XonXoff,
            FlowControl::XonXoffInline,
        ]),
    )
        .prop_map(|(baud_rate, data_bits, parity, stop_bits, flow)| {
            (
                LineConfig {
                    baud_rate,
                    data_bits,
                    parity,
                    stop_bits,
                },
                flow,
            )
        })
}

#[test]
fn serialport_adapter_matches_its_model() {
    let case = (
        initial_line(),
        0usize..700,
        prop::collection::vec(act(), 1..30),
    );
    run_cases(48, case, |((line, flow), chunk, actions)| {
        let fake = FakeTransport::cp2102();
        let handle = open_port(shared(&fake), 0).unwrap();
        fake.clear_recorded();
        let adapter = SerialPortAdapter::new(handle, "usb#0", line, flow)
            .unwrap()
            .with_write_chunk(chunk);
        let mut drv = Model::new(Chip::Cp2102, 64);
        drv.reset_session();
        let mut m = AModel {
            drv,
            line,
            flow,
            ring: VecDeque::new(),
            in_flight: None,
            unplugged: false,
            shut: false,
            chunk: chunk.max(64),
        };
        let mut ports = vec![adapter];
        let mut cur = 0;
        for a in &actions {
            if step(&mut ports, &mut cur, &fake, &mut m, a)?.is_none() {
                continue;
            }
            check_getters(&ports[cur], &m)?;
            prop_assert_eq!(
                ctls(&fake),
                m.drv.controls.clone(),
                "control transfers after {:?}",
                a
            );
            let bulk: Vec<(u8, Vec<u8>)> = fake
                .recorded_bulk_out()
                .into_iter()
                .map(|b| (b.endpoint, b.data))
                .collect();
            prop_assert_eq!(&bulk, &m.drv.bulk_out, "bulk OUT after {:?}", a);
        }
        ports[0].shutdown();
        Ok(())
    });
}
