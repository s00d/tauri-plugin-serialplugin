//! Stories: event delivery to watches and watch_core streams (threads, queues, parsers, URCs).

use crate::common::*;

/// Given a watch and a parser watch_core with 5 s batches and bytes still batched,
/// when the port is closed,
/// then both get the tail in order, the parser its remainder, a Closed state and no Disconnect.
#[test]
fn intentional_close_delivers_tail_in_order_without_disconnect() {
    let fake = Fake::new();
    let m = manager_with("bye", fake.port(true));
    let (sub, states) = core_sink();
    m.subscribe_core(sub);
    let (sink, rx) = old_sink();
    m.watch("bye".into(), watch_opts(5_000, false), sink)
        .unwrap();
    let (csink, crx) = core_sink();
    m.watch_core("bye".into(), watch_opts(5_000, false), readline(), csink)
        .unwrap();
    fake.push(b"head\ntail-data");
    // The hub took them (a hub read routes in the same loop pass, before a stop).
    wait_until_taken(&fake);
    m.close("bye".into()).unwrap();

    let data = collect_data(&rx, Duration::from_secs(2), |d| d.len() >= 14);
    assert_eq!(data, b"head\ntail-data");
    assert!(quiet_for(&rx, |e| matches!(
        e,
        SerialEvent::Disconnect { .. }
    )));
    assert_eq!(next_frame(&crx), b"head");
    assert_eq!(next_frame(&crx), b"tail-data");
    // Opened, then Closed; never Disconnected.
    let disconnected = |e: &CoreEvent| {
        matches!(
            e,
            CoreEvent::PortState {
                state: PortStatus::Disconnected { .. },
                ..
            }
        )
    };
    wait_event(&states, "Closed", |e| {
        assert!(!disconnected(e), "{e:?}");
        matches!(
            e,
            CoreEvent::PortState {
                state: PortStatus::Closed,
                ..
            }
        )
    });
    assert!(quiet_for(&states, disconnected));
}

/// Given a watch_core sink that blocks on its first event,
/// when more bytes arrive, at() runs and the port closes,
/// then at() and close return while the sink is still blocked, and the sink later gets
/// every byte in order.
#[test]
fn slow_core_sink_never_stalls_hub_at_or_close() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("slow", fake.port(true));
    let first = AtomicBool::new(true);
    let blocked = Arc::new(AtomicBool::new(false));
    let in_sink = blocked.clone();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    // The first data event blocks the sink until the checks below are done
    // (10 s cap in case the test fails before releasing it).
    let (release, gate) = mpsc::channel::<()>();
    let gate = Mutex::new(gate);
    let sink: CoreSink = Arc::new(move |e| {
        if core_data(&e).is_some() && first.swap(false, Ordering::SeqCst) {
            in_sink.store(true, Ordering::SeqCst);
            let _ = gate.lock().unwrap().recv_timeout(Duration::from_secs(10));
            in_sink.store(false, Ordering::SeqCst);
        }
        if let CoreEvent::Event(e) = e {
            return tx.lock().unwrap().send(e).is_ok();
        }
        true
    });
    m.watch_core("slow".into(), watch_opts(5, false), None, sink)
        .unwrap();
    fake.push(b"one ");
    wait_until("sink blocked", Duration::from_secs(2), || {
        blocked.load(Ordering::SeqCst)
    });
    fake.push(b"two ");
    wait_until_taken(&fake);

    let reply = m.at("slow".into(), "AT+CSQ".into(), None).unwrap();
    assert!(blocked.load(Ordering::SeqCst), "at() waited for the sink");
    assert!(reply.lines.iter().any(|l| l == "+CSQ: 20,99"), "{reply:?}");

    fake.push(b"three");
    wait_until_taken(&fake);
    m.close("slow".into()).unwrap();
    assert!(blocked.load(Ordering::SeqCst), "close waited for the sink");
    release.send(()).unwrap();

    let data = collect_data(&rx, Duration::from_secs(15), |d| d.len() >= 13);
    assert_eq!(data, b"one two three");
}

/// Given two watch_core streams with a small queue, one sink stalled,
/// when a flood arrives,
/// then the fast one gets all of it, the slow one an Overflow at the right byte and nothing after, and the port keeps working.
#[test]
fn rust_receiver_overflow_unsubscribes_and_port_keeps_working() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("flood", fake.port(true));
    m.set_core_queue_capacity(4096);
    let (gate_tx, gate_rx) = mpsc::channel::<()>();
    let gate = Mutex::new(Some(gate_rx));
    let (tx, slow_rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let slow: CoreSink = Arc::new(move |e| {
        if let Some(g) = gate.lock().unwrap().take() {
            let _ = g.recv();
        }
        tx.lock().unwrap().send(e).is_ok()
    });
    let opts = WatchOptions {
        serial_data_flush_interval_ms: Some(1),
        size: Some(256),
        ..Default::default()
    };
    m.watch_core("flood".into(), opts.clone(), None, slow)
        .unwrap();
    let (fast, fast_rx) = core_sink();
    m.watch_core("flood".into(), opts, None, fast).unwrap();

    for _ in 0..64 {
        fake.push(&[b'z'; 256]);
        wait_until_taken(&fake); // a stream of reads, not one chunk
    }
    let mut fast_bytes = 0;
    while fast_bytes < 64 * 256 {
        let ev = fast_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("fast sink");
        fast_bytes += core_data(&ev).map_or(0, <[u8]>::len);
    }
    assert!(m.managed_ports().unwrap().contains(&"flood".to_string()));
    m.at("flood".into(), "AT".into(), None)
        .expect("at after overflow");

    gate_tx.send(()).unwrap();
    let mut got = 0u64;
    let dropped_from = loop {
        match slow_rx
            .recv_timeout(Duration::from_secs(3))
            .expect("overflow")
        {
            CoreEvent::Overflow { dropped_from } => break dropped_from,
            ev => got += core_data(&ev).map_or(0, |d| d.len() as u64),
        }
    };
    assert_eq!(dropped_from, got);
    assert!(dropped_from < 64 * 256);
    fake.push(b"after");
    assert!(quiet_for(&slow_rx, |_| true), "unsubscribed after Overflow");
    m.close("flood".into()).unwrap();
}

/// Given a watch_core sink that calls at() on its own port,
/// when data arrives,
/// then the at() call completes (no deadlock).
#[test]
fn core_sinks_may_call_at_on_the_same_port() {
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("reentrant", fake.port(true));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let mgr = m.clone();
    let core: CoreSink = Arc::new(move |e| {
        if core_data(&e).is_some() {
            let r = mgr.at("reentrant".into(), "AT".into(), None);
            let _ = tx.lock().unwrap().send(r.is_ok());
        }
        true
    });
    m.watch_core("reentrant".into(), watch_opts(5, false), None, core)
        .unwrap();
    fake.push(b"ping");
    assert!(rx
        .recv_timeout(Duration::from_secs(5))
        .expect("sink at() hung"));
    m.close("reentrant".into()).unwrap();
}

/// Given an old watch with a slow sink and a batch still in the hub,
/// when the port is closed,
/// then close delivers the batch on the RX side, nothing reaches the sink after close, no delivery queue is used.
#[test]
fn old_watch_sink_runs_on_the_rx_side_as_in_4x() {
    let fake = Fake::new();
    let m = manager_with("timing", fake.port(true));
    let closed = Arc::new(AtomicBool::new(false));
    let late = Arc::new(AtomicUsize::new(0));
    let queued = Arc::new(AtomicUsize::new(0));
    let got = Arc::new(Mutex::new(Vec::new()));
    let sink: EventSink = {
        let (closed, late, queued, got) =
            (closed.clone(), late.clone(), queued.clone(), got.clone());
        Arc::new(move |e| {
            if closed.load(Ordering::SeqCst) {
                late.fetch_add(1, Ordering::SeqCst);
            }
            if thread::current().name() == Some("serial-watch-delivery") {
                queued.fetch_add(1, Ordering::SeqCst);
            }
            if let SerialEvent::Data { data, .. } = e {
                got.lock().unwrap().extend(data);
            }
            thread::sleep(Duration::from_millis(30));
            true
        })
    };
    // A 5 s batch: "head" is still in the hub at `close()`, which delivers it.
    m.watch("timing".into(), watch_opts(5_000, false), sink)
        .unwrap();
    fake.push(b"head");
    wait_until_taken(&fake);
    m.close("timing".into()).unwrap();
    closed.store(true, Ordering::SeqCst);
    // A window in which nothing may reach the sink (no event to wait for).
    thread::sleep(Duration::from_millis(200));
    assert_eq!(late.load(Ordering::SeqCst), 0, "sink called after close()");
    assert_eq!(
        queued.load(Ordering::SeqCst),
        0,
        "sink behind a delivery queue"
    );
    assert_eq!(got.lock().unwrap().as_slice(), b"head");
}

/// Given a framed old watch, a readline watch_core and a raw watch_core,
/// when bytes arrive and one stream is unwatched mid-line,
/// then each gets its own frames, the unwatched one its remainder, nothing after.
#[test]
fn watch_core_subscribers_have_their_own_parser_and_copy() {
    let fake = Fake::new();
    let m = manager_with("fanout", fake.port(true));
    let (old, old_rx) = old_sink();
    m.watch_framed("fanout".into(), WatchOptions::default(), readline(), old)
        .unwrap();
    let (a, a_rx) = core_sink();
    let a_id = m
        .watch_core("fanout".into(), WatchOptions::default(), readline(), a)
        .unwrap();
    let (b, b_rx) = core_sink();
    m.watch_core("fanout".into(), watch_opts(5, false), None, b)
        .unwrap();

    fake.push(b"x1\npar");
    assert_eq!(next_frame(&a_rx), b"x1");
    m.unwatch_core(a_id).unwrap();
    assert_eq!(next_frame(&a_rx), b"par", "remainder on unwatch_core");
    fake.push(b"tial\n");
    let lines = collect_data(&old_rx, Duration::from_secs(3), |d| d.len() >= 9);
    assert_eq!(lines, b"x1partial");
    let mut raw = Vec::new();
    while raw.len() < 11 {
        raw.extend(next_frame(&b_rx));
    }
    assert_eq!(raw, b"x1\npartial\n");
    assert!(quiet_for(&a_rx, |e| core_data(e).is_some()));
    assert_eq!(
        m.unwatch_core(a_id).unwrap_err().kind(),
        ErrorKind::NotFound
    );
    m.close("fanout".into()).unwrap();
}

/// Given two route_urc watch_core streams and a raw one,
/// when a URC arrives during at(),
/// then both route_urc streams get it, the raw one does not, at() gets its reply.
#[test]
fn urcs_during_at_go_to_every_route_urc_subscriber() {
    let fake = Fake::new();
    fake.modem("\r\n+CREG: 1,1\r\n");
    let m = manager_with("urc", fake.port(true));
    let (u1, u1_rx) = core_sink();
    let (u2, u2_rx) = core_sink();
    let (raw, raw_rx) = core_sink();
    m.watch_core("urc".into(), watch_opts(5, true), None, u1)
        .unwrap();
    m.watch_core("urc".into(), watch_opts(5, true), None, u2)
        .unwrap();
    m.watch_core("urc".into(), watch_opts(5, false), None, raw)
        .unwrap();
    let reply = m.at("urc".into(), "AT+CSQ".into(), None).unwrap();
    assert!(reply.lines.iter().any(|l| l == "+CSQ: 20,99"), "{reply:?}");
    for rx in [&u1_rx, &u2_rx] {
        let ev = rx.recv_timeout(Duration::from_secs(2)).expect("urc copy");
        assert!(
            matches!(&ev, CoreEvent::Event(SerialEvent::Urc { line, .. }) if line == "+CREG: 1,1"),
            "{ev:?}"
        );
    }
    assert!(quiet_for(&raw_rx, |e| matches!(
        e,
        CoreEvent::Event(SerialEvent::Urc { .. })
    )));
    m.close("urc".into()).unwrap();
}

/// Given a watch_core sink that returns false,
/// when three chunks arrive,
/// then it is called once, its stream is dropped (later bytes are idle RX) and its id is gone.
#[test]
fn core_sink_returning_false_is_removed() {
    /// Dropped with the sink, i.e. once its delivery thread has ended.
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let fake = Fake::new();
    let m = manager_with("gone", fake.port(true));
    let calls = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicBool::new(false));
    let (c, guard) = (calls.clone(), Dropped(ended.clone()));
    let sink: CoreSink = Arc::new(move |_| {
        let _ = &guard;
        c.fetch_add(1, Ordering::SeqCst);
        false
    });
    let id = m
        .watch_core("gone".into(), watch_opts(5, false), None, sink)
        .unwrap();
    fake.push(b"a");
    wait_until("the sink's delivery ended", Duration::from_secs(3), || {
        ended.load(Ordering::SeqCst)
    });
    // `b` reaches the dead stream, which the hub then drops; `c` finds no watch.
    fake.push(b"b");
    wait_until_taken(&fake);
    fake.push(b"c");
    wait_until_idle(&m, &fake, "gone", 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    // The stream is gone, so its id is too.
    assert_eq!(m.unwatch_core(id).unwrap_err().kind(), ErrorKind::NotFound);
    m.close("gone".into()).unwrap();
}

/// Given a read waiting for data (30 s timeout, nothing arrives),
/// when cancel_read is called,
/// then the read fails as cancelled.
#[test]
fn cancel_read_ends_a_pending_read() {
    let m = manager_with("cr", Fake::new().port(true));
    let bg = m.clone();
    let reader = thread::spawn(move || bg.read_binary("cr".into(), Some(30_000), None));
    // The read slot is not observable from the public API: cancel until the read ends
    // (a cancel before the slot exists is a no-op).
    wait_until("read cancelled", Duration::from_secs(3), || {
        m.cancel_read("cr".into()).unwrap();
        reader.is_finished()
    });
    let err = reader.join().unwrap().unwrap_err();
    assert_eq!(
        ErrorKind::from_message(&err.to_string()),
        ErrorKind::Cancelled
    );
    m.close("cr".into()).unwrap();
}
