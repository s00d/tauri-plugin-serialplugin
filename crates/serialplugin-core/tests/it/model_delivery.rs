//! Model test of one `watch_core` queue with a sink that blocks: chunks and URC lines of
//! random size arrive while the first event is still being handled. The model counts the
//! queue (payload plus 64 bytes an event) and predicts which events get through, whether
//! and at which data byte the stream overflows, and that nothing follows.

use crate::common::*;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use serialplugin_core::events::{CoreEvent, SerialEvent};
use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
enum Item {
    Data(usize),
    /// A `+CREG: …` line of this many digits.
    Urc(usize),
    Disconnect,
}

#[derive(Debug, PartialEq)]
enum Got {
    Data(usize),
    Urc(String),
    Disconnect,
    Overflow(u64),
}

const REASON: &str = "Serial port disconnected: fake device: read error";

/// What reaches the sink: the first event is taken off the queue, the others wait in it.
fn expected(items: &[Item], capacity: usize) -> Vec<Got> {
    let (mut out, mut queued, mut offset) = (Vec::new(), 0, 0u64);
    for (i, item) in items.iter().enumerate() {
        let (payload, got) = match item {
            Item::Data(n) => (*n, Got::Data(*n)),
            Item::Urc(n) => {
                let line = urc(*n);
                (line.len(), Got::Urc(line))
            }
            Item::Disconnect => (REASON.len(), Got::Disconnect),
        };
        let size = 64 + payload;
        if queued > 0 && queued + size > capacity {
            out.push(Got::Overflow(offset));
            return out;
        }
        if let Got::Data(n) = got {
            offset += n as u64;
        }
        if i > 0 {
            queued += size;
        }
        out.push(got);
        if matches!(item, Item::Disconnect) {
            return out;
        }
    }
    out
}

fn urc(digits: usize) -> String {
    format!("+CREG: {}", "1".repeat(digits))
}

fn item() -> impl Strategy<Value = Item> {
    prop_oneof![
        8 => (1usize..200).prop_map(Item::Data),
        3 => (1usize..40).prop_map(Item::Urc),
        1 => Just(Item::Disconnect),
    ]
}

fn run_case(items: &[Item], capacity: usize) {
    let device = FakeDevice::new();
    let m = manager_with("queue", device.port());
    m.set_core_queue_capacity(capacity);
    let (entered_tx, entered) = mpsc::channel();
    let (release, gate) = mpsc::channel::<()>();
    let (tx, rx) = mpsc::channel();
    let state = Mutex::new((tx, entered_tx, gate));
    let sink: Sink<CoreEvent> = Arc::new(move |event| {
        let state = state.lock().unwrap();
        let _ = state.1.send(());
        let _ = state.2.recv();
        let got = match event {
            CoreEvent::Event(SerialEvent::Data { data, .. }) => Got::Data(data.len()),
            CoreEvent::Event(SerialEvent::Urc { line, .. }) => Got::Urc(line),
            CoreEvent::Event(SerialEvent::Disconnect { reason, .. }) => {
                assert_eq!(reason, REASON);
                Got::Disconnect
            }
            CoreEvent::Overflow { dropped_from } => Got::Overflow(dropped_from),
            other => panic!("unexpected {other:?}"),
        };
        state.0.send(got).is_ok()
    });
    let (states, port_states) = crate::common::sink();
    m.subscribe_core(states);
    // Every read is one event: the window is 0 and the size above any chunk.
    m.watch_core("queue".into(), watch_opts(0, true), None, sink)
        .unwrap();
    let mut gone = false;
    for (i, item) in items.iter().enumerate() {
        match item {
            Item::Data(n) => device.send(&[vec![b'z'; n - 1], b"\n".to_vec()].concat()),
            Item::Urc(n) => device.send(format!("{}\r\n", urc(*n)).as_bytes()),
            Item::Disconnect => device.fail_reads(io::ErrorKind::BrokenPipe),
        }
        if i == 0 {
            next(&entered, "first event in the sink");
        }
        if matches!(item, Item::Disconnect) {
            gone = true;
            break;
        }
        device.wait_read();
    }
    if gone {
        // The state follows the stream's own Disconnect.
        next_state(&port_states, "queue");
    }
    // The last event is queued: the hub ends only after it.
    m.close("queue".into()).unwrap();
    drop(release);
    assert_eq!(rest(rx), expected(items, capacity));
}

#[test]
fn a_blocked_queue_matches_the_model() {
    let mut runner = TestRunner::new(Config {
        cases: 32,
        failure_persistence: None,
        ..Config::default()
    });
    let result = runner.run(&(vec(item(), 1..12), 1usize..1000), |(items, capacity)| {
        run_case(&items, capacity);
        Ok(())
    });
    if let Err(e) = result {
        panic!("{e}");
    }
}
