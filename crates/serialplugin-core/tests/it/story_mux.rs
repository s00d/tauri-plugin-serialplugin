//! Stories: the multiplexer beyond the model: starting it, timers, flow control that
//! outlasts a write, exchanges cut off, closing while the modem is slow, big frames.

use crate::common::*;
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::at::session::AtSessionConfig;
use serialplugin_core::cmux::{CmuxPhysicalIo, CmuxSession};
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{CoreEvent, PortStatus, SerialEvent, WatchOptions};
use serialplugin_core::hub::ExchangeWaiter;
use serialplugin_core::SerialManager;
use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// T1, N2 and T2 so long that the modem always answers in time.
const PATIENT: &str = "AT+CMUX=0,0,5,64,255,3,255";

/// A muxed port on a fresh modem.
fn muxed(path: &str, modem: Modem, options: MuxOptions) -> (SerialManager, FakeDevice, ModemLog) {
    let modem = modem.mux(options);
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    let m = manager_with(path, device.port());
    m.enable_mux(path.into(), PATIENT.into(), LONG_MS).unwrap();
    (m, device, log)
}

/// Given a Quectel M26, which loses the first frame after its AT+CMUX OK and is slow to
/// answer a CLD that overtakes a DISC,
/// when an app starts the multiplexer, talks AT on one channel and raw on another, cancels
/// a command that hangs, sets the channel's AT session and stops the multiplexer,
/// then every step works, each channel keeps its own traffic, CLD waits for the DISC
/// answer, and the port talks plain AT again.
#[test]
fn a_session_on_an_m26() {
    let options = MuxOptions::default().lost_first_frames(1);
    let modem = Modem::new()
        .silent("AT+COPS=?")
        .fails("AT+BAD", "ERROR")
        .mux(options);
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    let m = manager_with("m26", device.port());
    // T1 100 ms: the lost SABM is sent again.
    m.enable_mux("m26".into(), "AT+CMUX=0,0,5,127,10,3,255".into(), LONG_MS)
        .unwrap();
    let at = m.open_mux_channel("m26".into(), 1).unwrap();
    let data = m.open_mux_channel("m26".into(), 2).unwrap();
    assert_eq!(m.write(data.clone(), "hello".into()).unwrap(), 5);
    assert_eq!(m.write_binary(data.clone(), vec![1, 2, 3]).unwrap(), 3);
    let reply = m.at(at.clone(), "AT+CSQ".into(), None).unwrap();
    assert_eq!(reply.solicited_body, ["+CSQ: 20,99"]);
    assert!(log
        .commands()
        .iter()
        .any(|c| c.dlci == Some(1) && c.line == "AT+CSQ"));
    let payloads: Vec<_> = log
        .frames()
        .into_iter()
        .filter(|f| f.dlci == 2 && f.kind == mux::UIH)
        .map(|f| f.payload)
        .collect();
    assert_eq!(payloads, [b"hello".to_vec(), vec![1, 2, 3]]);

    let hanging = background(&m, {
        let at = at.clone();
        move |m| m.exchange(at, "AT+COPS=?\r".into(), no_prepare(LONG_MS))
    });
    log.wait_command("AT+COPS=?");
    m.cancel_exchange(at.clone()).unwrap();
    assert_eq!(
        kind(hanging.join().unwrap().unwrap_err()),
        ErrorKind::Cancelled
    );

    let strict = AtSessionConfig {
        expect_ok: Some(true),
        ..Default::default()
    };
    let bad = || m.at(at.clone(), "AT+BAD".into(), None);
    assert_eq!(bad().unwrap().status, AtParseStatus::Error);
    m.configure_at_session(at.clone(), strict).unwrap();
    assert!(bad().is_err());

    m.disable_mux("m26".into()).unwrap();
    assert!(!log.cld_before_ua(), "CLD overtook a DISC");
    let m26 = m.at("m26".into(), "AT".into(), None).unwrap();
    assert_eq!(m26.status, AtParseStatus::Ok);
    assert_eq!(kind(bad().unwrap_err()), ErrorKind::NotFound);
    m.close("m26".into()).unwrap();
    device.wait_closed();
}

/// Given modems that stay silent, refuse AT+CMUX, or say OK without multiplexing,
/// when the multiplexer is started,
/// then each failure has its kind (the caller's timeout, the modem's refusal, the control
/// channel), an OK already waiting counts as the answer, and the port is plain afterwards.
#[test]
fn starting_fails_three_ways() {
    let silent = FakeDevice::new();
    let m = manager_with("mute", silent.port());
    let start = |timeout| m.enable_mux("mute".into(), "AT+CMUX=0,0,5,31,1,0,2".into(), timeout);
    let e = start(50).unwrap_err();
    assert_eq!(e.to_string(), "exchange timed out after 50 ms");
    assert_eq!(kind(e), ErrorKind::Timeout);

    silent.send(b"\r\nOK\r\n");
    wait_idle(&m, &silent, "mute", 6);
    let e = start(50).unwrap_err();
    assert_eq!(e.to_string(), "no UA for CMUX DLCI 0 within 500 ms");
    let written = silent.written();
    assert!(written.starts_with(b"AT+CMUX="));
    // It may be multiplexing with its UA lost: CLD takes it back to AT mode.
    assert!(written.ends_with(&mux::frame(0, true, mux::UIH, &[mux::CLD, 1])));

    silent.modem(Modem::new());
    let e = start(LONG_MS).unwrap_err();
    assert_eq!(
        e.to_string(),
        "modem rejected AT+CMUX=0,0,5,31,1,0,2: ERROR"
    );
    let reply = m.at("mute".into(), "AT".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Ok);
    m.close("mute".into()).unwrap();
}

/// Frames a [`CmuxSession`] wrote, and how long it waits out flow control.
struct Wire {
    frames: Mutex<mux::Reader>,
    tx: Mutex<mpsc::Sender<mux::Frame>>,
    write_timeout: Duration,
    /// A channel write asked how long it may wait: it does so right before it looks at the
    /// flow control.
    asked: Mutex<mpsc::Sender<()>>,
}

impl CmuxPhysicalIo for Wire {
    fn write_all(&self, data: &[u8]) -> Result<(), String> {
        for frame in self.frames.lock().unwrap().feed(data) {
            let _ = self.tx.lock().unwrap().send(frame);
        }
        Ok(())
    }
    fn write_timeout(&self, _: usize) -> Duration {
        let _ = self.asked.lock().unwrap().send(());
        self.write_timeout
    }
}

fn wire(write_timeout: Duration) -> (Arc<Wire>, mpsc::Receiver<mux::Frame>, mpsc::Receiver<()>) {
    let (tx, rx) = mpsc::channel();
    let (asked, asks) = mpsc::channel();
    let wire = Wire {
        frames: Mutex::default(),
        tx: Mutex::new(tx),
        write_timeout,
        asked: Mutex::new(asked),
    };
    (Arc::new(wire), rx, asks)
}

fn session(
    command: &str,
    write_timeout: Duration,
) -> (Arc<CmuxSession>, mpsc::Receiver<mux::Frame>) {
    let (wire, rx, _) = wire(write_timeout);
    (CmuxSession::for_command("p".into(), wire, command), rx)
}

/// Given sessions with and without T1 / N2 in their AT+CMUX command,
/// when DLCs are opened on a modem that answers late, on the second try, or never,
/// then SABM goes out again every T1 at most N2 times, only once without them (4.x), and
/// the error names the DLC and the wait.
#[test]
fn sabm_is_retried_as_the_command_says() {
    let ua = mux::frame(1, true, mux::UA | mux::PF, &[]);
    let sabms = |rx: &mpsc::Receiver<mux::Frame>| {
        rx.try_iter()
            .filter(|f| f.kind == mux::SABM && f.dlci == 1)
            .count()
    };
    // T1 10 ms, N2 2.
    let retrying = "AT+CMUX=0,0,5,31,1,2";
    let (s, rx) = session(retrying, Duration::ZERO);
    assert_eq!(s.physical_path(), "p");
    s.register_dlci(1, "p#dlci=1".into());
    let e = s.open_dlci(1, Duration::from_millis(1)).unwrap_err();
    assert_eq!(e, "no UA for CMUX DLCI 1 within 30 ms");
    assert_eq!(sabms(&rx), 3);

    // T1 2.55 s: the answer comes to the first SABM, late or not.
    for command in ["AT+CMUX=0,0,5,31,255,2", "AT+CMUX=0"] {
        let (s, rx) = session(command, Duration::ZERO);
        s.register_dlci(1, "p#dlci=1".into());
        let opening = std::thread::spawn({
            let s = s.clone();
            move || s.open_dlci(1, WAIT)
        });
        assert_eq!(next(&rx, "SABM").kind, mux::SABM);
        s.feed_physical_rx(&ua);
        opening.join().unwrap().unwrap();
        // The V.24 signals follow; without T1 / N2 there is no second SABM whatever the
        // clock says (with them a stalled test may send one).
        let msc = pick(&rx, "MSC", |f| (f.dlci == 0).then(|| f.payload.clone()));
        assert_eq!(msc, [mux::MSC, 0x05, 0x07, 0x8D]);
        if command == "AT+CMUX=0" {
            assert_eq!(sabms(&rx), 0);
        }
    }

    let (s, rx) = session(retrying, Duration::ZERO);
    s.register_dlci(1, "p#dlci=1".into());
    let opening = std::thread::spawn({
        let s = s.clone();
        move || s.open_dlci(1, WAIT)
    });
    next(&rx, "first SABM");
    assert_eq!(next(&rx, "second SABM").kind, mux::SABM);
    s.feed_physical_rx(&ua);
    opening.join().unwrap().unwrap();

    for s in [session("AT+CMUX=0", Duration::ZERO), {
        let (wire, rx, _) = wire(Duration::ZERO);
        (CmuxSession::new("p".into(), wire), rx)
    }] {
        s.0.register_dlci(1, "p#dlci=1".into());
        let e = s.0.open_dlci(1, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e, "no UA for CMUX DLCI 1 within 100 ms");
        assert_eq!(sabms(&s.1), 1);
    }
}

/// Given a session with an exchange waiting on a channel, and a watch with a batch,
/// when the channel is unregistered, or registered again over the old one,
/// then the exchange fails, the watch gets its batch, and only the new channel is live.
#[test]
fn a_channel_that_goes_takes_its_exchange_and_watch() {
    for reopen in [false, true] {
        let (s, _rx) = session("AT+CMUX=0", Duration::ZERO);
        s.register_dlci(1, "p#dlci=1".into());
        let (events, events_rx) = sink::<SerialEvent>();
        s.set_watch_with(1, events, NEVER, None);
        s.feed_physical_rx(&mux::data(1, b"batch"));
        let options = no_prepare(LONG_MS).resolve();
        let waiter = ExchangeWaiter::new(options, Arc::default());
        s.set_exchange_waiter(1, waiter.clone());
        if reopen {
            s.register_dlci(1, "p#dlci=1".into());
            s.feed_physical_rx(&mux::data(1, b"nobody's"));
        } else {
            s.unregister_dlci(1);
            s.tick();
        }
        assert_eq!(waiter.wait(LONG_MS).unwrap_err(), "port closed");
        let events = rest(events_rx);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(data(&events[0]).unwrap(), b"batch");
    }
}

/// Given a modem that stops the flow for longer than a write may take,
/// when channels are written to, closed by the modem and flow comes back,
/// then a write fails at its timeout and is never sent later, a waiting write fails with
/// its channel, FCon is answered before the first data, and a reopened channel is not held.
#[test]
fn flow_control_outlasts_a_write() {
    let data =
        |rx: &mpsc::Receiver<mux::Frame>| pick(rx, "data", |f| (f.dlci != 0).then(|| f.clone()));
    let (s, rx) = session("AT+CMUX=0", Duration::from_millis(5));
    for dlci in [1, 2] {
        s.register_dlci(dlci, format!("p#dlci={dlci}"));
    }
    s.feed_physical_rx(&mux::control(mux::FCOFF, &[]));
    assert_eq!(
        s.send_uih(1, b"stale").unwrap_err(),
        "CMUX flow control off (FCoff)"
    );
    s.feed_physical_rx(&mux::control(mux::FCON, &[]));
    // Only DLCI 1: FC in its MSC.
    s.feed_physical_rx(&mux::control(mux::MSC, &[0x07, 0x8F]));
    assert_eq!(
        s.send_uih(1, b"stale").unwrap_err(),
        "CMUX DLCI 1 flow control off (MSC FC)"
    );
    assert_eq!(s.send_uih(2, b"now"), Ok(3));
    let first = data(&rx);
    assert_eq!((first.dlci, first.payload), (2, b"now".to_vec()));
    // A reopened channel is not held by what its previous life was told.
    s.feed_physical_rx(&mux::frame(1, false, mux::DISC | mux::PF, &[]));
    s.register_dlci(1, "p#dlci=1".into());
    assert_eq!(s.send_uih(1, b"again"), Ok(5));

    let (wire, rx, asks) = wire(Duration::from_millis(LONG_MS));
    let s = CmuxSession::for_command("p".into(), wire, "AT+CMUX=0");
    s.register_dlci(1, "p#dlci=1".into());
    s.feed_physical_rx(&mux::control(mux::FCOFF, &[]));
    assert_eq!(next(&rx, "FCoff answer").payload, [mux::FCOFF & !2, 1]);
    let write = |bytes: &'static [u8]| {
        let writer = s.clone();
        let write = std::thread::spawn(move || writer.send_uih(1, bytes));
        next(&asks, "the write");
        // It neither sends nor fails: a command answered later shows no data before it.
        meanwhile();
        s.feed_physical_rx(&mux::control(0x23, b"t"));
        assert_eq!(next(&rx, "TEST answer").payload, [0x21, 3, b't']);
        assert!(!write.is_finished(), "the write did not wait");
        write
    };
    let waiting = write(b"a");
    s.feed_physical_rx(&mux::control(mux::FCON, &[]));
    assert_eq!(waiting.join().unwrap(), Ok(1));
    assert_eq!(next(&rx, "FCon answer").payload, [mux::FCON & !2, 1]);
    assert_eq!(next(&rx, "data").payload, b"a");

    s.feed_physical_rx(&mux::control(mux::FCOFF, &[]));
    assert_eq!(next(&rx, "FCoff answer").payload, [mux::FCOFF & !2, 1]);
    let waiting = write(b"b");
    s.feed_physical_rx(&mux::frame(1, false, mux::DISC | mux::PF, &[]));
    assert_eq!(
        waiting.join().unwrap().unwrap_err(),
        "CMUX DLCI 1 closed by the modem"
    );
    s.feed_physical_rx(&mux::control(mux::FCON, &[]));
    assert!(rx.try_iter().all(|f| f.payload != b"b"));
}

/// Given a channel with an exchange in flight and a write waiting on flow control,
/// when the modem closes the channel, and on another port the device is unplugged,
/// then the exchange and the write fail at once (as disconnected for the unplug), the
/// watches get their remainder and one Disconnect, the port its state, and the channel
/// opens again with a new watch.
#[test]
fn exchanges_and_writes_end_with_their_channel() {
    for unplug in [false, true] {
        let modem = Modem::new().silent("AT+COPS=?");
        let (m, device, log) = muxed("gone", modem, MuxOptions::default());
        let (states_sink, states) = sink();
        m.subscribe_core(states_sink);
        let v = m.open_mux_channel("gone".into(), 1).unwrap();
        let held = m.open_mux_channel("gone".into(), 2).unwrap();
        let (watch, rx) = sink::<SerialEvent>();
        m.watch_framed(held.clone(), watch_opts(0, false), readline(), watch)
            .unwrap();
        device.send(&mux::data(2, b"one\ntail"));
        assert_eq!(next_data(&rx), b"one");
        // Waiting on flow control may take twice the wire time: at 1 baud, minutes.
        m.set_baud_rate("gone".into(), 1).unwrap();
        device.send(&mux::control(mux::MSC, &[0x0B, 0x8F]));
        log.wait_frame("MSC answer", |f| {
            f.payload.starts_with(&[mux::MSC & !2]) && f.payload[2] == 0x0B
        });
        let write = background(&m, {
            let held = held.clone();
            move |m| m.write(held, "never".into())
        });
        let exchange = background(&m, {
            let v = v.clone();
            move |m| m.exchange(v, "AT+COPS=?\r".into(), no_prepare(LONG_MS))
        });
        log.wait_command("AT+COPS=?");
        if unplug {
            device.fail_reads(io::ErrorKind::BrokenPipe);
        } else {
            device.send(&mux::frame(1, false, mux::DISC | mux::PF, &[]));
            device.send(&mux::frame(2, false, mux::DISC | mux::PF, &[]));
        }
        let e = exchange.join().unwrap().unwrap_err();
        if unplug {
            assert_eq!(kind(e), ErrorKind::Disconnected);
        } else {
            assert_eq!(e.to_string(), "CMUX DLCI 1 closed by the modem");
        }
        assert!(write.join().unwrap().is_err());
        assert_eq!(next_data(&rx), b"tail");
        assert_eq!(next_disconnect(&rx).0, held);
        let state = pick(&states, "channel state", |e| match e {
            CoreEvent::PortState {
                path,
                state: PortStatus::Disconnected { kind, .. },
            } if *path == held => Some(*kind),
            _ => None,
        });
        assert_eq!(state, ErrorKind::Disconnected);
        assert!(m.write(held.clone(), "x".into()).is_err());
        if !unplug {
            assert_eq!(m.open_mux_channel("gone".into(), 2).unwrap(), held);
            let (watch, rx) = sink::<SerialEvent>();
            m.watch(held.clone(), watch_opts(0, false), watch).unwrap();
            device.send(&mux::data(2, b"hello"));
            assert_eq!(collect(&rx, 5), b"hello");
        }
        let _ = m.close_all();
    }
}

/// Given a muxed port whose modem is slow to answer CLD (or never does),
/// when the port closes,
/// then other ports work meanwhile, the close ends with the answer, or after T2 without one.
#[test]
fn closing_waits_for_the_modem_not_for_the_manager() {
    let options = MuxOptions::default().answer_cld(false);
    let (m, device, log) = muxed("slow", Modem::new(), options.clone());
    let other = FakeDevice::new();
    m.insert_port("other".into(), Box::new(other.port()))
        .unwrap();
    let cld = |f: &mux::Frame| f.dlci == 0 && f.payload == [mux::CLD, 1];
    let closing = background(&m, |m| m.close("slow".into()));
    log.wait_frame("CLD", cld);
    m.write("other".into(), "x".into()).unwrap();
    assert_eq!(m.managed_ports().unwrap().len(), 2);
    device.send(&mux::control(mux::CLD & !2, &[]));
    closing.join().unwrap().unwrap();

    // T2 20 ms, never answered.
    let modem = Modem::new().mux(options);
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    m.insert_port("mute".into(), Box::new(device.port()))
        .unwrap();
    m.enable_mux("mute".into(), "AT+CMUX=0,0,5,31,255,0,2".into(), LONG_MS)
        .unwrap();
    m.close("mute".into()).unwrap();
    assert_eq!(log.frames().iter().filter(|f| cld(f)).count(), 1);
    m.close_all().unwrap();
}

/// Given a modem with an N1 of 2048 and channels with different watches,
/// when big frames and unfinished lines arrive and streams end one by one,
/// then nothing is dropped as too big, a batch waits for its window and goes out when its
/// stream ends (never to the next one), so does the parser's remainder, a stream whose sink said no gets nothing more, and a remainder sink may call the manager.
#[test]
fn channel_watches_keep_their_own_batches() {
    let modem = Modem::new().mux(MuxOptions::default());
    let device = FakeDevice::with_modem(modem);
    let m = manager_with("big", device.port());
    m.enable_mux("big".into(), "AT+CMUX=0,0,5,2048,255,0,255".into(), LONG_MS)
        .unwrap();
    let v = m.open_mux_channel("big".into(), 1).unwrap();
    let (batch, batch_rx) = sink::<CoreEvent>();
    let batch = m
        .watch_core(v.clone(), watch_opts(NEVER, false), None, batch)
        .unwrap();
    let (other, other_rx) = sink::<CoreEvent>();
    let other = m
        .watch_core(v.clone(), watch_opts(NEVER, false), None, other)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let no = calls.clone();
    m.watch_core(
        v.clone(),
        watch_opts(0, false),
        None,
        Arc::new(move |e| {
            no.lock().unwrap().push(data(&e));
            false
        }),
    )
    .unwrap();
    let (live, live_rx) = sink::<CoreEvent>();
    m.watch_core(v.clone(), watch_opts(0, false), None, live)
        .unwrap();
    let (tx, old_rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let manager = m.clone();
    let old = m
        .watch_framed(
            v.clone(),
            watch_opts(0, false),
            readline(),
            Arc::new(move |e| {
                let ports = manager.managed_ports().unwrap().len();
                tx.lock().unwrap().send((data(&e), ports)).is_ok()
            }),
        )
        .unwrap();

    device.send(&mux::data(1, &[b'A'; 2000]));
    assert_eq!(collect(&live_rx, 2000), [b'A'; 2000]);
    eventually("the refusing sink ran", || calls.lock().unwrap().len() == 1);
    device.send(&mux::data(1, b"\ntail"));
    assert_eq!(collect(&live_rx, 5), b"\ntail");
    assert_eq!(next(&old_rx, "line"), (Some(vec![b'A'; 2000]), 2));
    assert!(batch_rx.try_recv().is_err(), "batch before its window");
    let unwatch = background(&m, move |m| m.unwatch(old));
    assert_eq!(next(&old_rx, "remainder"), (Some(b"tail".to_vec()), 2));
    unwatch.join().unwrap().unwrap();
    m.unwatch_core(batch).unwrap();
    assert_eq!(collect(&batch_rx, 2005).len(), 2005);
    assert!(rest(batch_rx).is_empty());
    assert!(
        other_rx.try_recv().is_err(),
        "the other stream still batches"
    );
    assert_eq!(
        m.unwatch_core(batch).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    m.unwatch_core(other).unwrap();
    assert_eq!(collect(&other_rx, 2005).len(), 2005);
    assert_eq!(calls.lock().unwrap().len(), 1, "called after it said no");

    // The old watch's plain batch goes out before `unwatch` returns, and only to it.
    let (plain, plain_rx) = sink::<SerialEvent>();
    let plain = m.watch(v.clone(), watch_opts(NEVER, false), plain).unwrap();
    device.send(&mux::data(1, b"batched"));
    assert_eq!(collect(&live_rx, 7), b"batched");
    m.unwatch(plain).unwrap();
    assert_eq!(data(&plain_rx.try_recv().unwrap()).unwrap(), b"batched");
    assert!(rest(plain_rx).is_empty());
    let (again, again_rx) = sink::<SerialEvent>();
    m.watch(v.clone(), watch_opts(0, false), again).unwrap();
    device.send(&mux::data(1, b"new"));
    assert_eq!(collect(&again_rx, 3), b"new");
    assert_eq!(collect(&live_rx, 3), b"new");

    // The port takes its channels and their streams with it, and only those.
    let (elsewhere, _rx) = sink();
    let device = FakeDevice::new();
    m.insert_port("elsewhere".into(), Box::new(device.port()))
        .unwrap();
    let elsewhere = m
        .watch_core("elsewhere".into(), WatchOptions::default(), None, elsewhere)
        .unwrap();
    m.force_close("big".into()).unwrap();
    assert_eq!(m.managed_ports().unwrap(), ["elsewhere"]);
    assert!(rest(live_rx).is_empty());
    m.unwatch_core(elsewhere).unwrap();
    m.close_all().unwrap();
}
