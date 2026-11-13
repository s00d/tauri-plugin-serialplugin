//! Driver model: one property drives random action sequences through all seven
//! drivers on `FakeTransport` and checks, after every step, that the recorded
//! control transfers, bulk-OUT payloads, delivered RX bytes and result kinds equal
//! the per-chip reference model (`common::Model`). Open sequences are pinned by
//! the golden fixtures (`golden_parity.rs`); the model's log starts after each open.

use super::common::*;

#[derive(Debug, Clone)]
pub enum Action {
    Reopen,
    Close,
    Line(LineConfig),
    Flow(FlowControl),
    Dtr(bool),
    Rts(bool),
    /// Break on/off; the bytes answer CH34x's read-modify-write control IN.
    Break(bool, Vec<u8>),
    Latency(u8),
    Purge(PurgeKind),
    /// Modem status; the bytes answer the chip's status control IN (when it has one).
    Modem(Vec<u8>),
    /// CDC SERIAL_STATE bitmap / PL2303 status byte on the interrupt endpoint,
    /// with host padding (CDC) and an optional junk notification first (CDC).
    Notify {
        state: u16,
        pad: u8,
        junk: Option<Junk>,
    },
    Write(Vec<u8>),
    /// The synchronous `Driver::read` (data only ever arrives via the reader).
    SyncRead,
    /// Incoming payload plus the two FTDI status bytes put in front of every packet.
    Rx(Vec<u8>, [u8; 2]),
    /// The next interrupt/bulk-IN read fails (a stall or an unplug).
    RxError(String),
}

/// A notification the CDC reader must skip.
#[derive(Debug, Clone, Copy)]
pub enum Junk {
    RequestType,
    Notification,
    Index,
    ShortPayload,
}

struct Sut {
    fake: FakeTransport,
    transport: SharedTransport,
    driver: Box<dyn Driver>,
    bulk: Option<Box<dyn BulkIn>>,
}

/// The model decides which actions it can predict in its current state; the
/// rest are skipped (the step returns `None`).
fn observable(m: &Model, a: &Action) -> bool {
    let pending = m.pending_rx_error.is_some();
    if !m.opened_once {
        return true;
    }
    match a {
        // An injected bulk error must be consumed by an Rx before the session ends.
        Action::Reopen | Action::Close => !pending,
        Action::Write(_) | Action::SyncRead => true,
        _ if !m.open => false,
        // Starting the PL2303 status thread would race the Rx for the injected error.
        Action::Modem(_) => !(pending && matches!(m.chip, Chip::Pl2303(_))),
        Action::Notify { state, .. } => {
            if !m.chip.notifies() || m.notify_dead {
                return false;
            }
            // Only a change in the visible lines proves the frame was consumed.
            match m.chip {
                Chip::CdcIad => (state ^ m.cdc_state) & 0b1011 != 0,
                _ => m.pl_status.is_some_and(|s| (s ^ *state as u8) & 0x8B != 0),
            }
        }
        Action::RxError(_) => !pending,
        _ => true,
    }
}

fn wait_lines(driver: &mut Box<dyn Driver>, want: Lines) -> Result<(), TestCaseError> {
    let mut last = None;
    let seen = eventually(|| {
        last = Some(driver.modem_status().map(lines).map_err(|e| kind(&e)));
        last == Some(Ok(want))
    });
    prop_assert!(seen, "modem lines never became {:?}; last {:?}", want, last);
    Ok(())
}

fn reopen(sut: &mut Sut, m: &mut Model) -> Result<(), TestCaseError> {
    if m.open {
        close(sut, m)?;
    }
    let dev = describe_device(&sut.transport).unwrap();
    prop_assert_eq!(dev.driver, m.chip.driver());
    prop_assert_eq!(dev.ports.len(), m.chip.port_count());
    sut.driver = create_driver(dev.driver, m.chip.port());
    sut.driver
        .open(&sut.transport)
        .map_err(|e| TestCaseError::fail(format!("open: {e:?}")))?;
    let mut claimed = sut.fake.claimed_interfaces();
    claimed.sort_unstable();
    prop_assert_eq!(claimed, m.chip.claims());
    prop_assert_eq!(usize::from(sut.driver.bulk_in_mps()), m.mps);
    sut.fake.clear_recorded();
    sut.bulk = sut.driver.take_bulk_in();
    prop_assert!(sut.bulk.is_some(), "bulk IN after open");
    m.reset_session();
    Ok(())
}

fn close(sut: &mut Sut, m: &mut Model) -> Result<(), TestCaseError> {
    sut.bulk = None;
    prop_assert!(sut.driver.close().is_ok());
    m.close();
    prop_assert!(
        sut.fake.claimed_interfaces().is_empty(),
        "close releases every interface"
    );
    if m.chip.notifies() {
        prop_assert!(
            sut.fake.open_interrupt_in(0x81, 64).is_ok(),
            "close stops the interrupt-endpoint thread"
        );
    }
    Ok(())
}

type Event = Result<Vec<u8>, std::mem::Discriminant<UsbSerialError>>;

/// Read bulk IN until it runs dry, through the driver's RX filters.
fn rx_read(sut: &mut Sut, m: &Model) -> Result<Vec<Event>, TestCaseError> {
    let mut filters = sut.driver.rx_filters();
    let bulk = sut.bulk.as_mut().unwrap();
    let mut buf = vec![0u8; (m.mps * 4).max(64)];
    let mut events = vec![];
    let mut data = vec![];
    for _ in 0..10_000 {
        match bulk.read(&mut buf, 0) {
            Ok(ReadOutcome::Data(d)) => data.extend(apply_filters(&mut filters, &d)),
            Ok(ReadOutcome::TimedOut) => {
                events.push(Ok(data));
                return Ok(events);
            }
            Ok(ReadOutcome::Cancelled) => return Err(TestCaseError::fail("cancelled")),
            Err(e) => events.push(Err(kind(&e))),
        }
    }
    Err(TestCaseError::fail("bulk IN never ran dry"))
}

fn junk_frame(j: Junk, index: u16) -> Vec<u8> {
    let mut f = vec![0xA1, 0x20, 0, 0];
    f.extend(index.to_le_bytes());
    f.extend([2, 0, 0x0B, 0]);
    match j {
        Junk::RequestType => f[0] = 0x21,
        Junk::Notification => f[1] = 0x2A,
        Junk::Index => f[4] ^= 1,
        Junk::ShortPayload => f[6] = 1,
    }
    // One full interrupt packet, so the reader reads it on its own.
    f.resize(64, 0);
    f
}

fn step(sut: &mut Sut, m: &mut Model, a: &Action) -> Result<Option<()>, TestCaseError> {
    if !observable(m, a) {
        return Ok(None);
    }
    if !m.opened_once {
        // A driver that was never opened has no transport: nothing may reach the
        // bus or panic, and a write is an error.
        match a {
            Action::Reopen => {
                reopen(sut, m)?;
                return Ok(Some(()));
            }
            Action::Write(d) => prop_assert!(sut.driver.write(d).is_err()),
            Action::SyncRead => prop_assert_eq!(sut.driver.read(&mut [0; 8]).ok(), Some(0)),
            Action::Line(c) => {
                let _ = sut.driver.set_line_config(*c);
            }
            Action::Flow(f) => {
                let _ = sut.driver.set_flow_control(*f);
            }
            Action::Dtr(v) => {
                let _ = sut.driver.set_dtr(*v);
            }
            Action::Rts(v) => {
                let _ = sut.driver.set_rts(*v);
            }
            Action::Break(v, _) => {
                let _ = sut.driver.set_break(*v);
            }
            Action::Purge(k) => {
                let _ = sut.driver.purge(*k);
            }
            Action::Modem(_) => {
                let _ = sut.driver.modem_status();
            }
            Action::Latency(ms) => {
                let _ = sut.driver.set_latency_timer(*ms);
            }
            _ => return Ok(None),
        }
        prop_assert!(
            ctls(&sut.fake).is_empty(),
            "unopened driver sent a transfer"
        );
        return Ok(Some(()));
    }
    match a {
        Action::Reopen => reopen(sut, m)?,
        Action::Close => close(sut, m)?,
        Action::Write(d) => {
            let r = sut.driver.write(d);
            if m.open {
                prop_assert_eq!(r.ok(), Some(d.len()));
                m.bulk_out.push((m.chip.out_ep(), d.clone()));
            } else {
                prop_assert_eq!(class(&r), Exp::Io, "write on a closed driver");
            }
        }
        Action::SyncRead => prop_assert_eq!(sut.driver.read(&mut [0; 8]).ok(), Some(0)),
        Action::Line(c) => {
            let want = m.line(c);
            prop_assert_eq!(class(&sut.driver.set_line_config(*c)), want, "{:?}", c);
        }
        Action::Flow(f) => {
            let want = m.flow(*f);
            prop_assert_eq!(class(&sut.driver.set_flow_control(*f)), want);
        }
        Action::Dtr(v) => {
            let want = m.line_bit(true, *v);
            prop_assert_eq!(class(&sut.driver.set_dtr(*v)), want);
        }
        Action::Rts(v) => {
            let want = m.line_bit(false, *v);
            prop_assert_eq!(class(&sut.driver.set_rts(*v)), want);
        }
        Action::Break(v, reply) => {
            if m.chip.family() == Family::Ch {
                sut.fake.script_control_in_response(reply.clone());
            }
            let want = m.brk(*v, reply);
            prop_assert_eq!(class(&sut.driver.set_break(*v)), want);
        }
        Action::Latency(ms) => {
            if m.chip.family() == Family::Ftdi {
                let w = m.widx();
                m.ftdi(9, u16::from((*ms).max(1)), w);
            }
            prop_assert!(sut.driver.set_latency_timer(*ms).is_ok());
        }
        Action::Purge(k) => {
            let want = m.purge(*k);
            prop_assert_eq!(class(&sut.driver.purge(*k)), want);
        }
        Action::Modem(reply) => {
            if m.modem_reads_control_in() {
                sut.fake.script_control_in_response(reply.clone());
            }
            let (want, want_lines) = m.modem(reply);
            let got = sut.driver.modem_status();
            prop_assert_eq!(class(&got), want);
            prop_assert_eq!(got.ok().map(lines), want_lines);
        }
        Action::Notify { state, pad, junk } => {
            let mut wire = vec![];
            if m.chip == Chip::CdcIad {
                if let Some(j) = junk {
                    wire.extend(junk_frame(*j, m.chip.cdc_index()));
                }
                m.cdc_state = *state;
                let mut frame = vec![0xA1, 0x20, 0, 0, 0, 0, 2, 0];
                frame.extend(state.to_le_bytes());
                frame.resize(10 + usize::from(*pad % 55), 0);
                wire.extend(frame);
            } else {
                m.pl_status = Some(*state as u8);
                wire = vec![0u8; 10];
                wire[0] = 0xA1;
                wire[8] = *state as u8;
            }
            let want = m.modem(&[]).1.unwrap();
            sut.fake.push_interrupt_in(&wire);
            wait_lines(&mut sut.driver, want)?;
        }
        Action::Rx(payload, status) => {
            let (wire, kept) = m.rx(payload, *status);
            sut.fake.push_rx(&wire);
            let events = rx_read(sut, m)?;
            let mut want: Vec<Event> = vec![];
            if m.pending_rx_error.take().is_some() {
                want.push(Err(kind(&UsbSerialError::Io(String::new()))));
            }
            want.push(Ok(kept));
            prop_assert_eq!(events, want);
        }
        Action::RxError(msg) => {
            sut.fake.inject_bulk_read_error(msg.clone());
            if m.interrupt_thread_live() {
                // The interrupt thread is the only reader: its failure reaches modem_status.
                let failed = eventually(|| sut.driver.modem_status().is_err());
                prop_assert!(failed, "interrupt read error never reached modem_status");
                m.notify_dead = true;
            } else {
                m.pending_rx_error = Some(msg.clone());
            }
        }
    }
    Ok(Some(()))
}

fn check(sut: &Sut, m: &Model) -> Result<(), TestCaseError> {
    prop_assert_eq!(ctls(&sut.fake), m.controls.clone(), "control transfers");
    let bulk: Vec<(u8, Vec<u8>)> = sut
        .fake
        .recorded_bulk_out()
        .into_iter()
        .map(|b| (b.endpoint, b.data))
        .collect();
    prop_assert_eq!(bulk, m.bulk_out.clone(), "bulk OUT");
    Ok(())
}

fn run(chip: Chip, mps: u16, start_open: bool, actions: &[Action]) -> Result<(), TestCaseError> {
    let fake = chip.fake();
    set_bulk_mps(&fake, mps);
    let mut sut = Sut {
        transport: shared(&fake),
        fake,
        driver: create_driver(chip.driver(), chip.port()),
        bulk: None,
    };
    let mut m = Model::new(chip, usize::from(mps));
    if start_open {
        reopen(&mut sut, &mut m)?;
    }
    for a in actions {
        if step(&mut sut, &mut m, a)?.is_some() {
            check(&sut, &m)?;
        }
    }
    sut.bulk = None;
    let _ = sut.driver.close();
    Ok(())
}

fn chip() -> impl Strategy<Value = Chip> {
    prop::sample::select(vec![
        Chip::CdcSingle,
        Chip::CdcIad,
        Chip::CdcMulti(0),
        Chip::CdcMulti(1),
        Chip::Ft232r,
        Chip::Ft232h(7),
        Chip::Ft232h(8),
        Chip::Ft232h(9),
        Chip::Ft2232(0),
        Chip::Ft2232(1),
        Chip::Ft2232b(0),
        Chip::Ft2232b(1),
        Chip::Cp2102,
        Chip::Cp2105(0),
        Chip::Cp2105(1),
        Chip::Ch340,
        Chip::Pl2303(PlType::Hx),
        Chip::Pl2303(PlType::Hxn),
        Chip::Pl2303(PlType::T01),
        Chip::Pl2303(PlType::Ta),
        Chip::Gsm,
        Chip::Ccd(0),
        Chip::Ccd(1),
        Chip::Ccd(2),
    ])
}

fn action() -> impl Strategy<Value = Action> {
    let flow = prop::sample::select(vec![
        FlowControl::None,
        FlowControl::RtsCts,
        FlowControl::DtrDsr,
        FlowControl::XonXoff,
        FlowControl::XonXoffInline,
    ]);
    let purge = prop::sample::select(vec![PurgeKind::Rx, PurgeKind::Tx, PurgeKind::Both]);
    let junk = prop::option::of(prop::sample::select(vec![
        Junk::RequestType,
        Junk::Notification,
        Junk::Index,
        Junk::ShortPayload,
    ]));
    // Payload bytes favour XON/XOFF so the inline filter has work.
    let byte = prop_oneof![3 => any::<u8>(), 1 => Just(CHAR_XON), 1 => Just(CHAR_XOFF)];
    prop_oneof![
        1 => Just(Action::Reopen).boxed(),
        1 => Just(Action::Close).boxed(),
        4 => line_config().prop_map(Action::Line).boxed(),
        2 => flow.prop_map(Action::Flow).boxed(),
        2 => any::<bool>().prop_map(Action::Dtr).boxed(),
        2 => any::<bool>().prop_map(Action::Rts).boxed(),
        2 => (any::<bool>(), prop::collection::vec(any::<u8>(), 0..3))
            .prop_map(|(v, r)| Action::Break(v, r))
            .boxed(),
        1 => any::<u8>().prop_map(Action::Latency).boxed(),
        1 => purge.prop_map(Action::Purge).boxed(),
        2 => prop::collection::vec(any::<u8>(), 0..3).prop_map(Action::Modem).boxed(),
        2 => (any::<u16>(), any::<u8>(), junk)
            .prop_map(|(state, pad, junk)| Action::Notify { state, pad, junk })
            .boxed(),
        3 => prop::collection::vec(any::<u8>(), 0..1100).prop_map(Action::Write).boxed(),
        1 => Just(Action::SyncRead).boxed(),
        3 => (prop::collection::vec(byte, 0..700), any::<[u8; 2]>())
            .prop_map(|(p, s)| Action::Rx(p, s))
            .boxed(),
        1 => prop::sample::select(vec!["stall", "disconnected"])
            .prop_map(|s| Action::RxError(s.to_string()))
            .boxed(),
    ]
}

#[test]
fn every_driver_matches_its_model() {
    let case = (
        chip(),
        prop::sample::select(vec![8u16, 16, 64, 512]),
        prop::bool::weighted(0.9),
        prop::collection::vec(action(), 1..40),
    );
    run_cases(32, case, |(chip, mps, start_open, actions)| {
        run(chip, mps, start_open, &actions)
    });
}
