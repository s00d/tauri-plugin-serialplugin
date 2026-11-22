//! Stories: the RX hub used on its own (`PortRxHub`, `RxHubShared`), as the plugin's
//! backends use it: drains, hand-fed routing, and a hub nobody ticks.

use crate::common::*;
use serialplugin_core::at::parse::ExchangeMatch;
use serialplugin_core::cmux::{CmuxSession, SerialPortIo};
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{AtResultFormat, ExchangeOptions, SerialEvent};
use serialplugin_core::hub::{
    emit_urc_to, ExchangeWaiter, HubRoutingState, PortRxHub, RxHubHandle, RxHubShared,
};
use serialport::SerialPort;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

fn hub(device: &FakeDevice) -> PortRxHub {
    let port: Box<dyn SerialPort> = Box::new(device.port());
    PortRxHub::start(Arc::new(Mutex::new(port)), "hub".into())
}

fn waiter(command: &str, format: AtResultFormat) -> Arc<ExchangeWaiter> {
    let options = ExchangeOptions {
        command: Some(command.into()),
        result_format: Some(format),
        ..Default::default()
    };
    ExchangeWaiter::new(options.resolve(), Arc::new(AtomicBool::new(false)))
}

/// Start a drain with nothing to end it but `cancel`; returns once its slot is in.
fn drain(
    shared: &Arc<RxHubShared>,
    cancel: &Arc<AtomicBool>,
    idle_ms: u64,
) -> thread::JoinHandle<Result<Vec<u8>, String>> {
    // A drain throws idle bytes away as its slot goes in: that shows when.
    assert!(!shared.has_watch(), "the marker needs the idle buffer");
    shared.feed_bytes(b"stale", &mut HubRoutingState::new("hub".into()));
    let (bg, cancel) = (shared.clone(), cancel.clone());
    let drain = thread::spawn(move || bg.drain(idle_ms, LONG_MS, cancel, vec![]));
    eventually("the drain started", || shared.buffered_len() == 0);
    drain
}

/// Given a hub on a device,
/// when a watch, a multiplexer, exchanges and a stop go through the hub handle,
/// then each reaches the hub: the watch gets bytes until it is detached, the multiplexer
/// takes frames, a waiter takes the reply or the cancel, and the thread ends.
#[test]
fn a_hub_handle_drives_the_hub() {
    let device = FakeDevice::new();
    let hub = hub(&device);
    let shared = RxHubHandle::shared(&hub);
    let (events, rx) = sink::<SerialEvent>();
    RxHubHandle::attach_watch_with(&hub, events.clone(), 0, 64, false, None);
    device.send(b"abc");
    assert_eq!(collect(&rx, 3), b"abc");
    RxHubHandle::detach_watch(&hub);
    device.send(b"idle");
    eventually("bytes without a watch are idle", || {
        shared.buffered_len() == 4
    });

    let port: Box<dyn SerialPort> = Box::new(device.port());
    let session = CmuxSession::new(
        "hub".into(),
        Arc::new(SerialPortIo(Arc::new(Mutex::new(port)))),
    );
    session.register_dlci(2, "hub#dlci=2".into());
    session.set_watch_with(2, events, 0, None);
    RxHubHandle::attach_cmux(&hub, session);
    device.send(&mux::data(2, b"framed"));
    assert_eq!(collect(&rx, 6), b"framed");
    RxHubHandle::detach_cmux(&hub);
    device.send(b"more");
    eventually("the port is plain again", || shared.buffered_len() == 8);

    let answered = waiter("AT", AtResultFormat::Verbose);
    RxHubHandle::set_exchange_waiter(&hub, answered.clone());
    device.send(b"\r\nOK\r\n");
    let (raw, matched) = answered.wait(LONG_MS).unwrap();
    assert_eq!(raw, b"idlemore\r\nOK\r\n", "idle bytes come first");
    assert_eq!(matched, ExchangeMatch::Ok);
    RxHubHandle::clear_exchange_waiter(&hub);
    let cancelled = waiter("AT", AtResultFormat::Verbose);
    RxHubHandle::set_exchange_waiter(&hub, cancelled.clone());
    RxHubHandle::cancel_active_exchange(&hub);
    assert_eq!(cancelled.wait(LONG_MS).unwrap_err(), "exchange cancelled");
    RxHubHandle::clear_exchange_waiter(&hub);
    device.send(b"x");
    eventually("no waiter takes it", || shared.buffered_len() == 1);

    assert!(!hub.is_finished());
    RxHubHandle::shutdown_hub(&hub);
    eventually("the hub thread ended", || hub.is_finished());
    hub.stop();
    device.wait_closed();
}

/// Given a hub with a drain running,
/// when bytes arrive, time passes for a parser, a second drain starts and the first is
/// cancelled,
/// then the drain returns the bytes, the parser's frame does not wait for it, the second
/// is refused, and the cancel ends it with an error.
#[test]
fn a_drain_takes_the_bytes_and_nothing_else_waits_for_it() {
    let device = FakeDevice::new();
    let hub = hub(&device);
    let shared = hub.shared();
    let cancel = Arc::new(AtomicBool::new(false));
    // Bytes while the hub is held in a read: they reach the slot, not the idle buffer.
    device.hold_reads(true);
    device.wait_held(1);
    let draining = drain(&shared, &cancel, 1);
    device.send(b"late reply");
    device.hold_reads(false);
    assert_eq!(draining.join().unwrap().unwrap(), b"late reply");
    // Nothing at all: an empty drain.
    let empty = hub.drain(1, LONG_MS, cancel.clone(), vec![]);
    assert_eq!(empty, Ok(Vec::new()));

    let draining = drain(&shared, &cancel, LONG_MS);
    assert_eq!(
        hub.drain(1, 1, cancel.clone(), vec![]).unwrap_err(),
        "drain already in progress"
    );
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(draining.join().unwrap().unwrap_err(), "exchange cancelled");

    let (events, rx) = sink::<SerialEvent>();
    let idle_gap = parser(serde_json::json!({"type": "interByteTimeout", "interval": 50}));
    hub.attach_watch_with(
        events,
        NEVER,
        64,
        false,
        Some(idle_gap.unwrap().build().unwrap()),
    );
    device.send(&[1, 2]);
    device.wait_read();
    let cancel = Arc::new(AtomicBool::new(false));
    let draining = thread::spawn({
        let cancel = cancel.clone();
        let shared = shared.clone();
        move || shared.drain(LONG_MS, LONG_MS, cancel, vec![])
    });
    assert_eq!(next_data(&rx), [1, 2]);
    assert!(!draining.is_finished());
    cancel.store(true, Ordering::SeqCst);
    assert!(draining.join().unwrap().is_err());
    hub.stop();
}

/// Given a drain on a watched hub,
/// when a two-line URC arrives in two reads, the drain ends or fails, and the URC sink
/// starts a drain of its own,
/// then both lines go out together either way, and the sink does not hang the hub.
#[test]
fn urcs_in_a_drain_reach_the_watch() {
    for fails in [false, true] {
        let shared = Arc::new(RxHubShared::for_path("hub".into()));
        let mut routing = HubRoutingState::new("hub".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let draining = drain(&shared, &cancel, 0);
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let weak = Arc::downgrade(&shared);
        let reentrant: Sink<SerialEvent> = Arc::new(move |event| {
            let shared = weak.upgrade().unwrap();
            let _ = shared.drain(0, 0, Arc::new(AtomicBool::new(true)), vec![]);
            tx.lock().unwrap().send(event).is_ok()
        });
        // The sink's own drain ends by its caller's clock: the tick that runs it is busy.
        let (recorded, events) = sink::<SerialEvent>();
        let sink = if fails { recorded } else { reentrant };
        shared.attach_watch_with(sink, 0, 64, false, None);
        shared.feed_bytes(b"\r\n+CMT: ,23\r\n", &mut routing);
        shared.feed_bytes(b"0791447758100650\r\nOK\r\n", &mut routing);
        if fails {
            shared.fail_all_waiters("usb error");
            assert_eq!(draining.join().unwrap().unwrap_err(), "usb error");
        } else {
            let hub = shared.clone();
            let done = Arc::new(AtomicBool::new(false));
            let stop = done.clone();
            let ticking = thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    hub.tick("hub", &mut routing);
                    thread::yield_now();
                }
            });
            assert!(draining.join().unwrap().is_ok());
            let lines = [next_urc(&rx), next_urc(&rx)];
            assert_eq!(lines, ["+CMT: ,23", "0791447758100650"]);
            done.store(true, Ordering::SeqCst);
            ticking.join().unwrap();
            continue;
        }
        assert_eq!(next_urc(&events), "+CMT: ,23");
        assert_eq!(next_urc(&events), "0791447758100650");
    }
}

/// Given a hub no thread ticks,
/// when a read and drains time out, with and without bytes in the drain,
/// then the caller's own clock ends them: an empty one is a timeout, bytes are returned.
#[test]
fn callers_end_by_their_own_clock() {
    let shared = Arc::new(RxHubShared::new());
    let e = shared.read_request(64, 5, false).unwrap_err();
    assert_eq!(ErrorKind::from_message(&e), ErrorKind::Other, "{e}");
    assert_eq!(e, "no data received within 5 ms");
    let with_bytes = Arc::new(RxHubShared::new());
    let drains = [&shared, &with_bytes].map(|shared| {
        shared.feed_bytes(b"stale", &mut HubRoutingState::new("hub".into()));
        let bg = shared.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let drain = thread::spawn(move || bg.drain(LONG_MS, 0, cancel, vec![]));
        eventually("the drain started", || shared.buffered_len() == 0);
        drain
    });
    with_bytes.feed_bytes(b"kept", &mut HubRoutingState::new("hub".into()));
    let [empty, kept] = drains.map(|d| d.join().unwrap());
    assert_eq!(empty.unwrap_err(), "drain timed out waiting for hub");
    assert_eq!(kept.unwrap(), b"kept");
}

/// Given a hand-fed hub (the Android push model),
/// when an unwatch comes between a feed and its dispatch, the device is lost twice, and
/// exchanges run with both result formats,
/// then frames keep their order before the remainder, one Disconnect goes out, and a lone
/// `4` after a `+CMT` header is its data line or the numeric ERROR.
#[test]
fn hand_fed_routing_keeps_order() {
    let shared = RxHubShared::for_path("hub".into());
    let mut routing = HubRoutingState::new("hub".into());
    let lf = || Some(readline().unwrap().build().unwrap());
    let (events, rx) = sink::<SerialEvent>();
    shared.attach_watch_with(events, NEVER, 64, false, lf());
    shared.feed_bytes(b"a\nb", &mut routing);
    assert_eq!(
        shared.pending_watch_bytes(&routing),
        0,
        "a parser batches nothing"
    );
    shared.detach_watch();
    shared.dispatch_queued_events(std::mem::take(&mut routing.pending_events));
    assert_eq!(
        (next_data(&rx), next_data(&rx)),
        (b"a".to_vec(), b"b".to_vec())
    );

    for (format, want) in [
        (AtResultFormat::Verbose, vec!["+CMT: ,5", "4"]),
        (AtResultFormat::Numeric, vec!["+CMT: ,5"]),
    ] {
        let (events, rx) = sink::<SerialEvent>();
        shared.attach_watch_with(events, NEVER, 64, true, None);
        let waiter = waiter("AT", format);
        shared.set_exchange_waiter(waiter.clone());
        shared.feed_bytes(b"+CMT: ,5\r\n4\r\n", &mut routing);
        shared.dispatch_queued_events(std::mem::take(&mut routing.pending_events));
        shared.clear_exchange_waiter();
        assert_eq!(waiter.wait(0).is_ok(), format == AtResultFormat::Numeric);
        shared.detach_watch();
        let urcs: Vec<_> = rest(rx)
            .into_iter()
            .map(|e| match e {
                SerialEvent::Urc { line, .. } => line,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(urcs, want, "{format:?}");
    }

    let (events, rx) = sink::<SerialEvent>();
    shared.attach_watch_with(events.clone(), NEVER, 64, false, lf());
    shared.feed_bytes(b"half", &mut routing);
    shared.emit_disconnect("hub", "gone");
    shared.emit_disconnect("hub", "gone again");
    emit_urc_to(&events, "hub", "+CREG: 1");
    assert_eq!(next_data(&rx), b"half");
    assert_eq!(next_disconnect(&rx), ("hub".into(), "gone".into()));
    assert!(matches!(
        next(&rx, "URC"),
        SerialEvent::Urc { path, line } if path == "hub" && line == "+CREG: 1"
    ));
}
