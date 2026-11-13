//! Stories: devices that die (EOF, persistent errors, at once after open) and hub restarts.

use crate::common::*;

/// Given a port subscriber and a device whose reads fail with PermissionDenied (a Windows unplug),
/// when the errors persist,
/// then the port state is Disconnected with kind Disconnected.
#[test]
fn port_state_kind_of_an_unplug_is_disconnected() {
    let fake = Fake::new();
    let m = manager_with("win-unplug", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    fake.fail_reads(io::ErrorKind::PermissionDenied);
    let kind = loop {
        match states.recv_timeout(Duration::from_secs(5)).expect("state") {
            CoreEvent::PortState {
                state: PortStatus::Disconnected { kind, .. },
                ..
            } => break kind,
            _ => continue,
        }
    };
    assert_eq!(kind, ErrorKind::Disconnected);
    m.close("win-unplug".into()).unwrap();
}

/// Given a device that fails its first read,
/// when it is inserted (5 rounds),
/// then subscribers get Opened, then Disconnected, and the path stays managed until close.
#[test]
fn insert_port_of_a_dead_device_reports_opened_then_disconnected() {
    for round in 0..5 {
        let fake = Fake::new();
        fake.fail_reads(io::ErrorKind::BrokenPipe);
        let m = SerialManager::new();
        let (sub, states) = core_sink();
        m.subscribe_core(sub);
        m.insert_port("dead".into(), fake.port(true)).unwrap();
        let first = states.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(
            matches!(
                &first,
                CoreEvent::PortState {
                    state: PortStatus::Opened,
                    ..
                }
            ),
            "round {round}: {first:?}"
        );
        let second = states
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| panic!("round {round}: no Disconnected"));
        assert!(
            matches!(
                &second,
                CoreEvent::PortState {
                    state: PortStatus::Disconnected { .. },
                    ..
                }
            ),
            "round {round}: {second:?}"
        );
        assert_eq!(m.managed_ports().unwrap(), vec!["dead".to_string()]);
        m.close("dead".into()).unwrap();
    }
}

/// Given a watched port whose reads return Ok(0) (a tty after hang-up),
/// when the EOF reads start,
/// then three of them, a poll apart, end in one Disconnect with the EOF reason; nothing follows it.
#[test]
fn eof_reads_end_in_one_disconnect_without_spinning() {
    let fake = Fake::new();
    let m = manager_with("eof", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    let (cs, crx) = core_sink();
    m.watch_core("eof".into(), WatchOptions::default(), None, cs)
        .unwrap();
    fake.eof();
    let reason = "Serial port disconnected: EOF";
    loop {
        match crx
            .recv_timeout(Duration::from_secs(2))
            .expect("Disconnect")
        {
            CoreEvent::Event(SerialEvent::Disconnect { reason: r, .. }) => {
                assert_eq!(r, reason);
                break;
            }
            _ => continue,
        }
    }
    let kind = loop {
        match states.recv_timeout(Duration::from_secs(2)).expect("state") {
            CoreEvent::PortState {
                state: PortStatus::Disconnected { kind, reason: r },
                ..
            } => {
                assert_eq!(r, reason);
                break kind;
            }
            _ => continue,
        }
    };
    assert_eq!(kind, ErrorKind::Disconnected);
    wait_until("handles released", Duration::from_secs(2), || {
        fake.live() == 0
    });
    let reads = fake.eof_reads();
    assert_eq!(
        reads.len(),
        3,
        "EOF reads: no spinning, no read after Disconnect"
    );
    let spread = reads[2] - reads[0];
    assert!(
        spread >= Duration::from_millis(18),
        "EOF reads {spread:?} apart: hub spun"
    );
    assert!(
        crx.recv_timeout(Duration::from_millis(200)).is_err(),
        "a second event after Disconnect"
    );
    assert_eq!(m.managed_ports().unwrap(), vec!["eof".to_string()]);
    let e = m.write_core("eof".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Disconnected);
    assert_eq!(e.to_string(), reason);
    m.close("eof".into()).unwrap();
}

/// Given a watched port that returns two Ok(0) reads before each byte,
/// when this repeats five times,
/// then every byte arrives, no Disconnect, writes still work.
#[test]
fn data_between_eof_reads_keeps_the_port_open() {
    let fake = Fake::new();
    let m = manager_with("eof-reset", fake.port(true));
    let (s, rx) = old_sink();
    m.watch("eof-reset".into(), watch_opts(5, false), s)
        .unwrap();
    for i in 0..5u8 {
        {
            let mut q = fake.0.rx.lock().unwrap();
            fake.0.eof_left.store(2, Ordering::SeqCst);
            q.push_back(b'a' + i);
            fake.0.cv.notify_all();
        }
        let got = collect_data(&rx, Duration::from_secs(2), |d| !d.is_empty());
        assert_eq!(got, vec![b'a' + i]);
    }
    assert_eq!(fake.eof_reads().len(), 10);
    assert!(rx.recv_timeout(Duration::from_millis(100)).is_err());
    m.write("eof-reset".into(), "x".into())
        .expect("port still open");
    m.close("eof-reset".into()).unwrap();
}

/// Given a device with RDY already pending,
/// when insert_port_watch_core inserts it (5 rounds),
/// then the stream gets every byte of RDY.
#[test]
fn insert_port_watch_core_gets_the_first_read() {
    for round in 0..5 {
        let fake = Fake::new();
        fake.push(b"RDY\r\n");
        let m = SerialManager::new();
        let (cs, crx) = core_sink();
        let id = m
            .insert_port_watch_core(
                "rdy".into(),
                fake.port(true),
                watch_opts(5, false),
                None,
                cs,
            )
            .unwrap();
        let mut got: Vec<u8> = Vec::new();
        while got.len() < 5 {
            let ev = crx
                .recv_timeout(Duration::from_secs(2))
                .unwrap_or_else(|_| panic!("round {round}: got {got:?}"));
            got.extend(core_data(&ev).unwrap_or_default());
        }
        assert_eq!(got, b"RDY\r\n");
        m.unwatch_core(id).unwrap();
        m.close("rdy".into()).unwrap();
    }
}

/// Given a device that fails at once,
/// when insert_port_watch_core inserts it (5 rounds),
/// then the call succeeds and the stream gets the Disconnect with the read error.
#[test]
fn insert_port_watch_core_of_a_dead_device_delivers_disconnect() {
    for round in 0..5 {
        let fake = Fake::new();
        fake.fail_reads(io::ErrorKind::BrokenPipe);
        let m = SerialManager::new();
        let (cs, crx) = core_sink();
        m.insert_port_watch_core(
            "dead".into(),
            fake.port(true),
            WatchOptions::default(),
            None,
            cs,
        )
        .expect("insert succeeds");
        match crx
            .recv_timeout(Duration::from_secs(2))
            .unwrap_or_else(|_| panic!("round {round}: no Disconnect"))
        {
            CoreEvent::Event(SerialEvent::Disconnect { path, reason }) => {
                assert_eq!(path, "dead");
                assert_eq!(reason, "Serial port disconnected: fake read error");
            }
            other => panic!("round {round}: {other:?}"),
        }
        m.close("dead".into()).unwrap();
    }
}

/// Given a tcp bridge that greets as soon as it accepts,
/// when open_watch_core opens it with a readline parser,
/// then the stream gets both greeting lines as frames.
#[test]
fn open_watch_core_gets_bytes_sent_right_after_open() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    let (done_tx, done_rx) = mpsc::channel::<()>();
    let server = thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        sock.write_all(b"RDY\r\n+QIND: 1\r\n").unwrap();
        let _ = done_rx.recv_timeout(Duration::from_secs(5));
    });
    let m = SerialManager::new();
    let (cs, crx) = core_sink();
    let (session, id) = open_watch(&m, &path, cs).expect("open_watch_core");
    assert_eq!(session, path);
    assert_eq!(next_frame(&crx), b"RDY\r");
    assert_eq!(next_frame(&crx), b"+QIND: 1\r");
    m.unwatch_core(id).unwrap();
    m.close(path).unwrap();
    let _ = done_tx.send(());
    server.join().unwrap();
}

/// Given a tcp bridge that closes the connection at once,
/// when open_watch_core opens it,
/// then the open succeeds and the stream gets Disconnect (connection closed).
#[test]
fn open_watch_core_of_a_bridge_that_hangs_up_delivers_disconnect() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    let server = thread::spawn(move || drop(listener.accept().unwrap()));
    let m = SerialManager::new();
    let (cs, crx) = core_sink();
    open_watch(&m, &path, cs).expect("open succeeds");
    server.join().unwrap();
    // One deadline for the whole wait: other events must not extend it.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match crx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("Disconnect")
        {
            CoreEvent::Event(SerialEvent::Disconnect { path: p, reason }) => {
                assert_eq!(p, path);
                assert!(reason.contains("closed the connection"), "{reason}");
                break;
            }
            _ => continue,
        }
    }
    m.close(path).unwrap();
}

/// Given an open tcp port and a port subscriber,
/// when the same path is opened again,
/// then the subscriber gets Closed, then Opened.
#[test]
fn reopen_closes_the_old_session_first() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let path = format!("tcp://{}", listener.local_addr().unwrap());
    let m = SerialManager::new();
    m.open(path.clone(), 115_200, None, None, None, None, Some(100))
        .unwrap();
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    m.open(path.clone(), 115_200, None, None, None, None, Some(100))
        .unwrap();
    let mut seen = Vec::new();
    while seen.len() < 2 {
        if let CoreEvent::PortState { state, .. } = states
            .recv_timeout(Duration::from_secs(3))
            .expect("port state")
        {
            seen.push(state);
        }
    }
    assert_eq!(seen, vec![PortStatus::Closed, PortStatus::Opened]);
    m.close(path).unwrap();
}

/// Given a watch sink that panics and kills the hub thread,
/// when a read comes next,
/// then a new hub starts and later bytes are read.
#[test]
fn a_dead_hub_is_restarted_by_the_next_read() {
    let fake = Fake::new();
    let m = manager_with("phoenix", fake.port(true));
    let hit = Arc::new(AtomicBool::new(false));
    let flag = hit.clone();
    let sink: EventSink = Arc::new(move |ev| {
        if matches!(ev, SerialEvent::Data { .. }) {
            flag.store(true, Ordering::SeqCst);
            panic!("sink panics (expected by this test)");
        }
        true
    });
    m.watch("phoenix".into(), watch_opts(0, false), sink)
        .unwrap();
    fake.push(b"x");
    wait_until("sink ran", Duration::from_secs(2), || {
        hit.load(Ordering::SeqCst)
    });
    fake.push(b"hello");
    // Whether the dead hub thread has ended is not observable from the public API: a
    // read fails until it has (its watch is still attached), then starts a new hub.
    let mut got = Vec::new();
    wait_until("a new hub read the bytes", Duration::from_secs(3), || {
        if let Ok(d) = m.read_binary("phoenix".into(), Some(50), Some(5 - got.len())) {
            got.extend(d);
        }
        got.len() >= 5
    });
    assert_eq!(got, b"hello");
    let _ = m.close("phoenix".into());
}
