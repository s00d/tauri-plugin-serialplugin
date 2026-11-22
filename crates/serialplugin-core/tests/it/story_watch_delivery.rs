//! Stories: how events reach the watches: threads, batches, timers, sinks that block, say
//! no or call back, and what the last events are.

use crate::common::*;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{
    CoreEvent, ExchangeOptions, PortStatus, SerialEvent, WatchOptions,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

/// Given an old watch and a parser `watch_core`, both with a batch that never ends,
/// when the port is closed with a command in flight,
/// then the command is cancelled, both get the tail in order (the parser its remainder)
/// before close returns and nothing after it, the old
/// sink on no delivery thread, and the state is Closed, never Disconnected.
#[test]
fn a_close_delivers_the_tail() {
    let device = FakeDevice::with_modem(Modem::new().silent("AT+HANG"));
    let m = manager_with("bye", device.port());
    let (states, states_rx) = sink::<CoreEvent>();
    let states = m.subscribe_core(states);
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    m.watch(
        "bye".into(),
        watch_opts(NEVER, false),
        Arc::new(move |e| {
            let name = std::thread::current().name().map(String::from);
            tx.lock().unwrap().send((data(&e), name)).is_ok()
        }),
    )
    .unwrap();
    let (core, core_rx) = sink::<CoreEvent>();
    m.watch_core("bye".into(), watch_opts(NEVER, false), readline(), core)
        .unwrap();
    device.send(b"head\ntail-data");
    device.wait_handled();
    // A command in flight holds the hub too; the close ends it.
    let command = background(&m, |m| {
        m.exchange("bye".into(), "AT+HANG\r".into(), no_prepare(LONG_MS))
    });
    device.wait_written("the command", |w| w.ends_with(b"\r"));
    m.close("bye".into()).unwrap();
    let old: Vec<_> = rx.try_iter().collect();
    assert_eq!(
        kind(command.join().unwrap().unwrap_err()),
        ErrorKind::Cancelled
    );
    assert!(rest(rx).is_empty(), "events after close returned");
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].0.as_deref(), Some(&b"head\ntail-data"[..]));
    assert_ne!(old[0].1.as_deref(), Some("serial-core-delivery"));
    let frames: Vec<_> = rest(core_rx).iter().map(data).collect();
    assert_eq!(
        frames,
        [Some(b"head".to_vec()), Some(b"tail-data".to_vec())]
    );
    m.unsubscribe_core(states).unwrap();
    let states: Vec<_> = rest(states_rx)
        .into_iter()
        .map(|e| match e {
            CoreEvent::PortState { state, .. } => state,
            other => panic!("unexpected {other:?}"),
        })
        .collect();
    assert_eq!(states, [PortStatus::Closed]);
}

/// Given a `watch_core` sink blocked in its first event and a fast one beside it,
/// when a flood arrives, a command runs and the port closes,
/// then the hub, the command, the fast stream and the close go on without the blocked sink,
/// which later gets every byte in order; with a small queue it overflows and is gone.
#[test]
fn a_blocked_sink_holds_up_no_one() {
    for small in [false, true] {
        let device = FakeDevice::with_modem(Modem::new());
        let m = manager_with("slow", device.port());
        if small {
            m.set_core_queue_capacity(4096);
        }
        let (entered, entries) = sink::<()>();
        let (release, gate) = mpsc::channel::<()>();
        let gate = Mutex::new(gate);
        let (forward, rx) = sink::<CoreEvent>();
        let slow: Sink<CoreEvent> = Arc::new(move |e| {
            entered(());
            let _ = gate.lock().unwrap().recv();
            forward(e)
        });
        let options = || WatchOptions {
            size: Some(256),
            ..watch_opts(0, false)
        };
        let id = m.watch_core("slow".into(), options(), None, slow).unwrap();
        let (fast, fast_rx) = sink::<CoreEvent>();
        m.watch_core("slow".into(), options(), None, fast).unwrap();
        device.send(b"one ");
        next(&entries, "blocked sink");
        assert_eq!(collect(&fast_rx, 4), b"one ");
        let flood = [b'z'; 64 * 256];
        for chunk in flood.chunks(256) {
            // Taken before the next: the fast stream never queues much.
            device.send(chunk);
            assert_eq!(collect(&fast_rx, 256), chunk);
        }
        m.at("slow".into(), "AT".into(), None)
            .expect("a command beside a blocked sink");
        if small {
            let e = m.unwatch_core(id).unwrap_err();
            assert_eq!(e.kind(), ErrorKind::NotFound, "overflowed and gone");
        }
        m.close("slow".into()).unwrap();
        assert!(entries.try_recv().is_err(), "still in its first event");
        drop(release);
        let events = rest(rx);
        let got: usize = events.iter().filter_map(data).map(|d| d.len()).sum();
        match events.last() {
            Some(CoreEvent::Overflow { dropped_from }) if small => {
                assert_eq!(*dropped_from, got as u64);
                assert!(got < flood.len());
            }
            _ => {
                assert!(!small);
                let all: Vec<u8> = events.iter().filter_map(data).flatten().collect();
                assert_eq!(all, [&b"one "[..], &flood].concat());
            }
        }
    }
}

/// Given a `watch_core` sink that says no to its first event,
/// when more bytes arrive,
/// then it is not called again, later bytes are idle RX and its id is gone.
#[test]
fn a_sink_that_says_no_is_removed() {
    /// Dropped with the sink, once its delivery thread has ended.
    struct Dropped(Arc<AtomicBool>);
    impl Drop for Dropped {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let device = FakeDevice::new();
    let m = manager_with("gone", device.port());
    let calls = Arc::new(AtomicUsize::new(0));
    let ended = Arc::new(AtomicBool::new(false));
    let (count, guard) = (calls.clone(), Dropped(ended.clone()));
    let no: Sink<CoreEvent> = Arc::new(move |_| {
        let _ = &guard;
        count.fetch_add(1, Ordering::SeqCst);
        false
    });
    let id = m
        .watch_core("gone".into(), watch_opts(0, false), None, no)
        .unwrap();
    device.send(b"a");
    eventually("its delivery ended", || ended.load(Ordering::SeqCst));
    // The dead stream takes nothing more.
    device.send(b"b");
    wait_idle(&m, &device, "gone", 1);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(m.unwatch_core(id).unwrap_err().kind(), ErrorKind::NotFound);
    m.close("gone".into()).unwrap();
}

/// Given a parser watch holding an unfinished line,
/// when `unwatch` delivers it to a sink that calls back into the manager,
/// then the call works: the sink runs under no manager lock.
#[test]
fn a_remainder_sink_may_call_back() {
    let device = FakeDevice::new();
    let m = manager_with("remainder", device.port());
    let manager = m.clone();
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let sink: Sink<SerialEvent> = Arc::new(move |event| {
        let ports = manager.managed_ports().unwrap().len();
        tx.lock()
            .unwrap()
            .send((data(&event).unwrap(), ports))
            .is_ok()
    });
    let lf = parser(serde_json::json!({"type": "delimiter", "delimiter": [10]}));
    let id = m
        .watch_framed("remainder".into(), WatchOptions::default(), lf, sink)
        .unwrap();
    device.send(b"first\npartial");
    assert_eq!(next(&rx, "frame"), (b"first".to_vec(), 1));
    let unwatch = background(&m, move |m| m.unwatch(id));
    assert_eq!(next(&rx, "remainder"), (b"partial".to_vec(), 1));
    unwatch.join().unwrap().unwrap();
    m.close("remainder".into()).unwrap();
}

/// Given watches with every kind of batch beside a live one,
/// when bytes arrive that end no line,
/// then a batch waits for its window or for its size, not for more; with `route_urc` the
/// unfinished line follows on the window, also into a parser; a parser's own timer fires
/// with no more data; a size of 0 still reads.
#[test]
fn batches_lines_and_timers() {
    let device = FakeDevice::new();
    let m = manager_with("batch", device.port());
    let core = |options, parser| {
        let (events, rx) = sink::<CoreEvent>();
        m.watch_core("batch".into(), options, parser, events)
            .unwrap();
        rx
    };
    let sized = |size| WatchOptions {
        size: Some(size),
        ..watch_opts(NEVER, false)
    };
    let three = || parser(serde_json::json!({"type": "byteLength", "length": 3}));
    let waits = core(watch_opts(NEVER, false), None);
    let waits_for_size = core(sized(6), None);
    let line_tail = core(watch_opts(1, true), None);
    let parsed_tail = core(watch_opts(1, true), three());
    let gap = parser(serde_json::json!({"type": "interByteTimeout", "interval": 20}));
    let idle_gap = core(watch_opts(NEVER, false), gap);
    let live = core(watch_opts(0, false), None);
    let (old, old_rx) = sink::<SerialEvent>();
    let old_id = m.watch("batch".into(), sized(6), old).unwrap();

    device.send(b"abc");
    for rx in [&live, &line_tail, &parsed_tail, &idle_gap] {
        assert_eq!(next_data(rx), b"abc");
    }
    assert!(waits.try_recv().is_err() && waits_for_size.try_recv().is_err());
    assert!(old_rx.try_recv().is_err());
    device.send(b"def");
    assert_eq!(collect(&live, 3), b"def");
    assert_eq!(next_data(&waits_for_size), b"abcdef");
    assert_eq!(next_data(&old_rx), b"abcdef");
    assert!(waits.try_recv().is_err());
    m.unwatch(old_id).unwrap();

    // The old watch: the line tail on its window, with and without a parser.
    for (parser, whole) in [(None, &b"ghi"[..]), (three(), b"jkl")] {
        let (old, rx) = sink::<SerialEvent>();
        let id = m
            .watch_framed("batch".into(), watch_opts(1, true), parser, old)
            .unwrap();
        device.send(whole);
        assert_eq!(collect(&rx, 3), whole);
        m.unwatch(id).unwrap();
    }
    // Frames go out at once; bytes that look like URCs stay data without `route_urc`, as
    // does anything binary (a MAVLink frame with LF and its start byte inside).
    let (old, rx) = sink::<SerialEvent>();
    let id = m.watch("batch".into(), watch_opts(0, false), old).unwrap();
    let raw = b"$GPGGA,123519,4807.038,N*47\r\n+CREG: 1\r\n\xFD\x09\x00\x0A\x2B\x00\xFD\x72";
    device.send(raw);
    assert_eq!(collect(&rx, raw.len()), raw);
    m.unwatch(id).unwrap();
    m.close("batch".into()).unwrap();
    let all = rest(waits);
    assert_eq!(all.len(), 1, "one batch at the end");
    assert!(data(&all[0]).unwrap().starts_with(b"abcdefghijkl"));

    let device = FakeDevice::new();
    let m = manager_with("zero", device.port());
    let (events, rx) = sink::<SerialEvent>();
    let options = WatchOptions {
        size: Some(0),
        ..watch_opts(0, false)
    };
    let id = m.watch("zero".into(), options, events).unwrap();
    device.send(b"read");
    assert_eq!(collect(&rx, 4), b"read");
    m.unwatch(id).unwrap();
    device.send(b"idle");
    wait_idle(&m, &device, "zero", 4);
    m.close("zero".into()).unwrap();
}

/// Given `watch_core` streams of several sizes, then an old watch beside them,
/// when more bytes wait than any of them asks for,
/// then the hub reads as much as the biggest stream takes at once, and with the old watch as
/// much as that one takes.
#[test]
fn reads_are_as_big_as_the_biggest_stream_or_the_old_watch() {
    let device = FakeDevice::new();
    let m = manager_with("size", device.port());
    let sized = |size| WatchOptions {
        size: Some(size),
        ..watch_opts(0, false)
    };
    let streams = [16, 4096].map(|size| {
        let (events, rx) = sink::<CoreEvent>();
        m.watch_core("size".into(), sized(size), None, events)
            .unwrap();
        rx
    });
    // The bytes of the first read once `n` wait, then the rest.
    let first_read = |n: usize| {
        // The read in progress may have begun with the size before.
        device.send(b"x");
        streams
            .iter()
            .for_each(|rx| assert_eq!(next_data(rx), b"x"));
        device.hold_reads(true);
        device.wait_held(1);
        device.send(&vec![b'y'; n]);
        device.hold_reads(false);
        let first = streams.each_ref().map(|rx| next_data(rx).len());
        for (rx, first) in streams.iter().zip(first) {
            collect(rx, n - first);
        }
        first
    };
    assert_eq!(first_read(5000), [4096; 2]);
    let (events, _rx) = sink::<SerialEvent>();
    let old = m.watch("size".into(), sized(8), events).unwrap();
    assert_eq!(first_read(20), [8; 2]);
    m.unwatch(old).unwrap();
    assert_eq!(first_read(5000), [4096; 2]);
    m.close("size".into()).unwrap();
}

/// Given a command draining the port, with a `route_urc` stream and a plain one,
/// when a two-line URC arrives before the command, then in its drain, and the command is
/// cancelled,
/// then the `route_urc` stream gets the first line as a URC, then both lines, the plain
/// stream gets no URC.
#[test]
fn urcs_of_a_drain_reach_a_stream() {
    let device = FakeDevice::new();
    let m = manager_with("drain", device.port());
    // Small reads: a drain shows by its own, bigger ones.
    let small = |batch, route_urc| WatchOptions {
        size: Some(64),
        ..watch_opts(batch, route_urc)
    };
    let (urcs, urcs_rx) = sink::<CoreEvent>();
    m.watch_core("drain".into(), small(NEVER, true), None, urcs)
        .unwrap();
    let (plain, plain_rx) = sink::<CoreEvent>();
    m.watch_core("drain".into(), small(0, false), None, plain)
        .unwrap();
    let pdu = "0791447758100650";
    // Outside a drain only the first line is a URC.
    let urc = format!("\r\n+CMT: ,23\r\n{pdu}\r\n");
    device.send(urc.as_bytes());
    device.send(b"RING\r\n");
    assert_eq!(
        [next_urc(&urcs_rx), next_urc(&urcs_rx)],
        ["+CMT: ,23", "RING"]
    );

    let command = background(&m, |m| {
        let options = ExchangeOptions {
            drain_idle_ms: Some(LONG_MS),
            drain_max_ms: Some(LONG_MS),
            ..Default::default()
        };
        m.exchange("drain".into(), "AT\r".into(), options)
    });
    device.wait_asked(1024);
    device.send(urc.as_bytes());
    device.wait_handled();
    eventually("the command was cancelled", || {
        m.cancel_exchange("drain".into()).unwrap();
        command.is_finished()
    });
    assert!(command.join().unwrap().is_err());
    assert_eq!([next_urc(&urcs_rx), next_urc(&urcs_rx)], ["+CMT: ,23", pdu]);
    m.close("drain".into()).unwrap();
    let any_urc = rest(plain_rx)
        .iter()
        .any(|e| matches!(e.serial(), Some(SerialEvent::Urc { .. })));
    assert!(!any_urc, "URC on a stream without route_urc");
}
