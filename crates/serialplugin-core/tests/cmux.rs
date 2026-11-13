//! Stories: CMUX on the manager (fake modems, M26 quirks, channel lifecycle).

use crate::common::*;

/// Given a port in CMUX mode with channel 2 open,
/// when text and binary writes go to the channel path,
/// then the modem gets them as UIH frames on DLCI 2.
#[test]
fn write_on_cmux_virtual_path_goes_out_as_uih() {
    let (fake, uih) = mux_fake();
    let m = manager_with("muxed", fake.port(true));
    m.enable_mux("muxed".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let vpath = m.open_mux_channel("muxed".into(), 2).unwrap();
    assert_eq!(m.write(vpath.clone(), "hello".into()).unwrap(), 5);
    assert_eq!(m.write_binary(vpath, vec![1, 2, 3]).unwrap(), 3);
    wait_until("UIH frames", Duration::from_secs(2), || {
        uih.lock().unwrap().len() >= 2
    });
    let got = uih.lock().unwrap().clone();
    assert_eq!(got, vec![(2, b"hello".to_vec()), (2, vec![1, 2, 3])]);
    m.close("muxed".into()).unwrap();
}

/// Given a modem deaf for a moment after the AT+CMUX OK (Quectel M26),
/// when enable_mux runs right after open,
/// then it succeeds.
#[test]
fn enable_mux_right_after_open_waits_for_the_modem_to_switch() {
    let fake = Fake::new();
    m26_like_modem(&fake, 30);
    let m = manager_with("m26-open", fake.port(true));
    m.enable_mux(
        "m26-open".into(),
        "AT+CMUX=0,0,5,127,10,3,30,10,2".into(),
        1000,
    )
    .expect("enable_mux with no traffic before it");
    m.disable_mux("m26-open".into()).unwrap();
    m.close("m26-open".into()).unwrap();
}

/// Given a modem that holds CLD while a DISC is pending (Quectel M26),
/// when disable_mux closes an open channel,
/// then the CLD goes out only after the DISC's UA was read, so the quirk never triggers.
#[test]
fn disable_mux_with_a_channel_sends_cld_after_the_disc_is_answered() {
    let fake = Fake::new();
    let cld_before_ua = m26_like_modem(&fake, 0);
    let m = manager_with("m26-cld", fake.port(true));
    m.enable_mux(
        "m26-cld".into(),
        "AT+CMUX=0,0,5,127,10,3,30,10,2".into(),
        1000,
    )
    .unwrap();
    m.open_mux_channel("m26-cld".into(), 1).unwrap();
    m.disable_mux("m26-cld".into()).unwrap();
    assert!(
        !cld_before_ua.load(Ordering::SeqCst),
        "CLD while DISC unanswered"
    );
    m.close("m26-cld".into()).unwrap();
}

/// Given a muxed port whose modem leaves CLD unanswered,
/// when it is closing (blocked on the CLD response),
/// then calls on another port complete while that close is still blocked.
#[test]
fn closing_a_muxed_port_does_not_hold_the_manager_lock() {
    use serialplugin_core::cmux::frame::{encode_frame, UIH};
    let (fake, _uih) = mux_fake();
    let m = manager_with("slow-cld", fake.port(true));
    m.insert_port("other".into(), Fake::new().port(true))
        .unwrap();
    // T2 = 255 (2.55 s, the 27.007 maximum): the close waits on CLD until the test answers.
    m.enable_mux("slow-cld".into(), "AT+CMUX=0,0,5,31,10,3,255".into(), 1000)
        .unwrap();
    let cld = encode_frame(0, true, UIH, &[0xC3, 0x01]);
    let bg = m.clone();
    let closing = thread::spawn(move || bg.close("slow-cld".into()));
    wait_until("CLD sent", Duration::from_secs(2), || {
        fake.tx().windows(cld.len()).any(|w| w == cld.as_slice())
    });
    m.write("other".into(), "x".into()).unwrap();
    m.managed_ports().unwrap();
    assert!(!closing.is_finished(), "close stopped waiting for CLD");
    fake.push(&encode_frame(0, true, UIH, &[0xC1, 0x01]));
    closing.join().unwrap().unwrap();
    m.close("other".into()).unwrap();
}

/// Given a CMUX channel with a watch,
/// when the modem closes the channel,
/// then the watch gets Disconnect, the path a Disconnected state, writes fail, and a reopened channel works with a new watch.
#[test]
fn channel_closed_by_the_modem_reports_state_and_reopens_cleanly() {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF, UIH};
    let (fake, _uih) = mux_fake();
    let m = manager_with("dlc", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    m.enable_mux("dlc".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("dlc".into(), 2).unwrap();
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s).unwrap();

    fake.push(&encode_frame(2, true, DISC | PF, &[]));
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).expect("Disconnect") {
            SerialEvent::Disconnect { path, .. } => break assert_eq!(path, v),
            _ => continue,
        }
    }
    loop {
        match states.recv_timeout(Duration::from_secs(2)).expect("state") {
            CoreEvent::PortState {
                path,
                state: PortStatus::Disconnected { kind, .. },
            } if path == v => break assert_eq!(kind, ErrorKind::Disconnected),
            _ => continue,
        }
    }
    assert!(m.write(v.clone(), "x".into()).is_err());

    assert_eq!(m.open_mux_channel("dlc".into(), 2).unwrap(), v);
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s)
        .expect("watch on the re-opened channel");
    fake.push(&encode_frame(2, true, UIH, b"hello"));
    assert_eq!(
        collect_data(&rx, Duration::from_secs(2), |d| d.len() >= 5),
        b"hello"
    );
    m.close("dlc".into()).unwrap();
}

/// Given a CMUX channel with a watch,
/// when the modem sends DISC on DLCI 0,
/// then the watch gets Disconnect, channel writes fail and CMUX can start again.
#[test]
fn modem_leaving_mux_tears_the_session_down() {
    use serialplugin_core::cmux::frame::{encode_frame, DISC, PF};
    let (fake, _uih) = mux_fake();
    let m = manager_with("left", fake.port(true));
    m.enable_mux("left".into(), "AT+CMUX=0".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("left".into(), 1).unwrap();
    let (s, rx) = old_sink();
    m.watch(v.clone(), watch_opts(5, false), s).unwrap();

    fake.push(&encode_frame(0, true, DISC | PF, &[]));
    loop {
        match rx.recv_timeout(Duration::from_secs(2)).expect("Disconnect") {
            SerialEvent::Disconnect { path, .. } => break assert_eq!(path, v),
            _ => continue,
        }
    }
    wait_until("channel write fails", Duration::from_secs(2), || {
        m.write(v.clone(), "x".into()).is_err()
    });
    wait_until("CMUX can start again", Duration::from_secs(3), || {
        m.enable_mux("left".into(), "AT+CMUX=0".into(), 1000)
            .is_ok()
    });
    m.close("left".into()).unwrap();
}

/// Given a modem that never answers,
/// when enable_mux runs with 100 ms,
/// then it fails as a timeout (a hang is caught by the test runner's limit).
#[test]
fn enable_mux_uses_the_callers_timeout() {
    let m = manager_with("mute", Fake::new().port(true));
    let err = m
        .enable_mux("mute".into(), "AT+CMUX=0".into(), 100)
        .unwrap_err();
    assert_eq!(
        ErrorKind::from_message(&err.to_string()),
        ErrorKind::Timeout
    );
    m.close("mute".into()).unwrap();
}

/// Given an OK already waiting in the hub,
/// when enable_mux runs,
/// then the waiting OK counts as the AT+CMUX reply.
#[test]
fn enable_mux_does_not_drain_before_at_cmux() {
    let fake = Fake::new();
    let m = manager_with("nodrain", fake.port(true));
    fake.push(b"\r\nOK\r\n");
    wait_until("hub buffered the reply", Duration::from_secs(2), || {
        m.bytes_to_read("nodrain".into()).unwrap() == 6
    });
    // T1 100 ms, N2 0, T2 20 ms: the failing DLCI 0 open and CLD stay short.
    let err = m
        .enable_mux("nodrain".into(), "AT+CMUX=0,0,5,31,10,0,2".into(), 100)
        .unwrap_err();
    // The OK was taken as the AT+CMUX reply, so the failure is the DLCI 0 open.
    assert_eq!(ErrorKind::from_message(&err.to_string()), ErrorKind::Other);
    assert!(fake.tx().starts_with(b"AT+CMUX="), "AT+CMUX went out first");
    m.close("nodrain".into()).unwrap();
}

/// Given a muxed port with channel watch_core streams and another port's stream,
/// when the channel and the port are closed,
/// then the channel's batch is delivered, the channel path is gone, the other stream stays.
#[test]
fn muxed_port_close_takes_its_channels_and_streams() {
    use serialplugin_core::cmux::frame::{encode_frame, UIH};
    let (fake, _uih) = mux_fake();
    let m = manager_with("mx", fake.port(true));
    m.insert_port("mx-other".into(), Fake::new().port(true))
        .unwrap();
    let other = m
        .watch_core(
            "mx-other".into(),
            WatchOptions::default(),
            None,
            core_sink().0,
        )
        .unwrap();
    m.enable_mux("mx".into(), "AT+CMUX=0,0,5,31,10,0,2".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("mx".into(), 2).unwrap();
    let (batch, batch_rx) = core_sink();
    m.watch_core(v.clone(), watch_opts(10_000, false), None, batch)
        .expect("watch_core on a channel");
    let (live, live_rx) = core_sink();
    m.watch_core(v.clone(), watch_opts(0, false), None, live)
        .unwrap();
    fake.push(&encode_frame(2, true, UIH, b"tail"));
    assert_eq!(next_frame(&live_rx), b"tail");
    m.close(v.clone()).unwrap();
    assert_eq!(next_frame(&batch_rx), b"tail", "batch delivered on close");

    let v = m.open_mux_channel("mx".into(), 3).unwrap();
    m.force_close("mx".into()).unwrap();
    assert!(!m.managed_ports().unwrap().contains(&v));
    m.unwatch_core(other)
        .expect("other port's stream still registered");
    m.close("mx-other".into()).unwrap();
}

/// Given a muxed port with an open channel,
/// when the device disconnects,
/// then the channel path gets a Disconnected state too.
#[test]
fn disconnect_of_a_muxed_port_reports_its_channels() {
    let (fake, _uih) = mux_fake();
    let m = manager_with("mxd", fake.port(true));
    m.enable_mux("mxd".into(), "AT+CMUX=0,0,5,31,10,0,2".into(), 1000)
        .unwrap();
    let v = m.open_mux_channel("mxd".into(), 2).unwrap();
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    fake.fail_reads(io::ErrorKind::BrokenPipe);
    let disconnected = |p: String| move |ev: &CoreEvent| matches!(ev, CoreEvent::PortState { path, state: PortStatus::Disconnected { .. } } if *path == p);
    wait_event(&states, "channel Disconnected", disconnected(v.clone()));
    let _ = m.close(v);
    let _ = m.close("mxd".into());
}
