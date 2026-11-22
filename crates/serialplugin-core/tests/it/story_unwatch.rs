//! Stories: what `unwatch` and `close` promise while sinks run: nothing arrives after they
//! returned, the tail comes first, they wait for no other stream, and sinks that call back,
//! block or panic hold up no one else.

use crate::common::*;
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{CoreEvent, ExchangeOptions, PortStatus, SerialEvent};
use serialplugin_core::SerialManager;
use std::io;
use std::sync::{Arc, Barrier, Mutex, OnceLock};
use std::thread::JoinHandle;

/// T1 100 ms, N2 3, T2 300 ms: a channel closed from a sink waits out T1 (the hub is in the
/// sink, the modem's answer stays unread).
const COMMAND: &str = "AT+CMUX=0,0,5,64,10,3,30";

/// A watched path and how its device sends to it.
struct Rig {
    m: SerialManager,
    device: FakeDevice,
    /// The port, or a channel of it.
    path: String,
    port: &'static str,
    channel: bool,
    log: Log,
}

impl Rig {
    /// `channel`: the path is a CMUX channel.
    fn new(port: &'static str, channel: bool) -> Self {
        let modem = Modem::new()
            .silent("AT+HANG")
            .mux(MuxOptions::default().answer_cld(true));
        let device = FakeDevice::with_modem(modem);
        let m = manager_with(port, device.port());
        let mut path = port.to_string();
        if channel {
            m.enable_mux(port.into(), COMMAND.into(), LONG_MS).unwrap();
            path = m.open_mux_channel(port.into(), 1).unwrap();
        }
        Self {
            m,
            device,
            path,
            port,
            channel,
            log: Log::default(),
        }
    }

    fn frame(&self, bytes: &[u8]) -> Vec<u8> {
        if self.channel {
            mux::data(1, bytes)
        } else {
            bytes.to_vec()
        }
    }

    fn send(&self, bytes: &[u8]) {
        self.device.send(&self.frame(bytes));
    }

    /// A `readline` watch whose sink blocks in the line `one`, with `tail` unfinished.
    fn blocked(&self) -> (u32, Gate) {
        let (sink, gate) = gated(&self.log);
        let id = self
            .m
            .watch_framed(
                self.path.clone(),
                watch_opts(NEVER, false),
                readline(),
                sink,
            )
            .unwrap();
        self.send(b"one\ntail");
        gate.wait_in("one");
        (id, gate)
    }

    /// `call` in the background; its return is logged as `name`.
    fn call<T: Send + 'static>(
        &self,
        name: &'static str,
        call: impl FnOnce(SerialManager) -> T + Send + 'static,
    ) -> JoinHandle<T> {
        let log = self.log.clone();
        background(&self.m, move |m| {
            let result = call(m);
            note(&log, name);
            result
        })
    }

    /// The watch is detached: what comes now is not its.
    fn wait_detached(&self) {
        let path = self.port.to_string();
        eventually("the watch is detached", || {
            let read = self.m.read(path.clone(), Some(0), Some(1));
            !read.unwrap_err().to_string().contains("watch is active")
        });
    }

    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
}

/// Given a port (and a channel) whose watch sink is busy with a line on the hub thread,
/// when another thread unwatches,
/// then `unwatch` returns after that line and the unfinished one behind it, the sink gets
/// nothing more, and the next bytes are the port's again.
#[test]
fn unwatch_waits_for_its_sink_and_the_tail() {
    for channel in [false, true] {
        let rig = Rig::new("busy", channel);
        let (id, gate) = rig.blocked();
        let unwatch = rig.call("unwatched", move |m| m.unwatch(id));
        if !channel {
            rig.wait_detached();
        }
        meanwhile();
        drop(gate);
        unwatch.join().unwrap().unwrap();
        let want = ["in one", "out one", "in tail", "out tail", "unwatched"];
        assert_eq!(rig.log(), want);
        assert_eq!(
            kind(rig.m.unwatch(id).unwrap_err()),
            ErrorKind::NotFound,
            "a second unwatch"
        );
        rig.send(b"later\n");
        if channel {
            rig.device.wait_handled();
        } else {
            wait_idle(&rig.m, &rig.device, rig.port, 6);
        }
        assert_eq!(rig.log(), want);
        rig.m.close(rig.port.into()).unwrap();
    }
}

/// Given the same busy sink,
/// when two threads close the port and a third unwatches meanwhile,
/// then all of them return after the tail, one close succeeds, and no Disconnect is sent.
#[test]
fn closers_and_an_unwatch_wait_for_the_tail() {
    for channel in [false, true] {
        let rig = Rig::new("closers", channel);
        let (states, states_rx) = sink::<CoreEvent>();
        rig.m.subscribe_core(states);
        let (id, gate) = rig.blocked();
        let port = || rig.port.to_string();
        let first = rig.call("closed", {
            let port = port();
            move |m| m.close(port)
        });
        if !channel {
            eventually("the close took the record", || {
                rig.m.managed_ports().unwrap().is_empty()
            });
        }
        let second = rig.call("closed", {
            let port = port();
            move |m| m.close(port)
        });
        let unwatch = rig.call("unwatched", move |m| m.unwatch(id));
        meanwhile();
        drop(gate);
        let closes = [first.join().unwrap(), second.join().unwrap()];
        assert!(closes.iter().any(|c| c.is_ok()), "{closes:?}");
        let _ = unwatch.join().unwrap();
        let log = rig.log();
        assert_eq!(log[..4], ["in one", "out one", "in tail", "out tail"]);
        assert_eq!(log.len(), 7, "{log:?}");
        rig.device.wait_closed();
        assert_eq!(next_state(&states_rx, &rig.path), PortStatus::Closed);
    }
}

/// Given a channel whose sink is busy while its modem leaves the multiplexer,
/// when the app unwatches the channel,
/// then `unwatch` returns after the remainder and the Disconnect.
/// Given a busy sink another thread is unwatching,
/// when the app closes the physical port,
/// then `close` waits for that sink too.
#[test]
fn a_channel_that_is_gone_still_ends_in_order() {
    let rig = Rig::new("left", true);
    let (sink, gate) = gated(&rig.log);
    let id = rig
        .m
        .watch_framed(rig.path.clone(), watch_opts(NEVER, false), readline(), sink)
        .unwrap();
    // In one read: the modem is gone before the sink runs.
    let end = [rig.frame(b"one\ntail"), mux::control(mux::CLD, &[])].concat();
    rig.device.send(&end);
    gate.wait_in("one");
    let unwatch = rig.call("unwatched", move |m| m.unwatch(id));
    meanwhile();
    drop(gate);
    unwatch.join().unwrap().unwrap();
    assert_eq!(
        rig.log(),
        [
            "in one",
            "out one",
            "in tail",
            "out tail",
            "in Disconnect",
            "out Disconnect",
            "unwatched"
        ]
    );
    rig.m.close(rig.port.into()).unwrap();

    let rig = Rig::new("twice", true);
    let (sink, gate) = gated(&rig.log);
    let id = rig
        .m
        .watch(rig.path.clone(), watch_opts(NEVER, false), sink)
        .unwrap();
    rig.send(b"batch");
    rig.device.wait_handled();
    // The caller of `unwatch` delivers the batch itself: the hub is free.
    let unwatch = rig.call("unwatched", move |m| m.unwatch(id));
    gate.wait_in("batch");
    let close = rig.call("closed", |m| m.close("twice".into()));
    meanwhile();
    drop(gate);
    unwatch.join().unwrap().unwrap();
    close.join().unwrap().unwrap();
    let log = rig.log();
    assert_eq!(log[..2], ["in batch", "out batch"], "{log:?}");
    assert_eq!(log.len(), 4);
}

/// Given two ports (and a port and a channel) whose sinks run at the same moment,
/// when each unwatches, or closes, the other's path,
/// then neither waits for the other: both calls return, and every device is released.
#[test]
fn sinks_that_end_each_other_do_not_deadlock() {
    for (close, channel) in [(false, false), (true, false), (false, true), (true, true)] {
        let rigs = [Rig::new("left", false), Rig::new("right", channel)];
        let meet = Arc::new(Barrier::new(2));
        let ids: Arc<[OnceLock<u32>; 2]> = Arc::default();
        let (done, returns) = sink::<bool>();
        for i in 0..2 {
            let other = rigs[1 - i].m.clone();
            let path = rigs[1 - i].path.clone();
            let (meet, known, done) = (meet.clone(), ids.clone(), done.clone());
            let first = Mutex::new(true);
            let id = rigs[i]
                .m
                .watch(
                    rigs[i].path.clone(),
                    watch_opts(0, false),
                    Arc::new(move |_| {
                        if std::mem::take(&mut *first.lock().unwrap()) {
                            meet.wait();
                            let result = if close {
                                other.close(path.clone())
                            } else {
                                other.unwatch(*known[1 - i].wait())
                            };
                            done(result.is_ok());
                        }
                        true
                    }),
                )
                .unwrap();
            let _ = ids[i].set(id);
        }
        rigs.iter().for_each(|rig| rig.send(b"go"));
        assert!(next(&returns, "the first sink's call"));
        assert!(next(&returns, "the second sink's call"));
        for rig in &rigs {
            if !close {
                rig.send(b"nobody's");
                rig.device.wait_handled();
            }
            let _ = rig.m.close_all();
            rig.device.wait_closed();
        }
    }
}

/// Given a watch sink that unwatches its own watch, with a tail held, and one that closes
/// its own port,
/// when they run,
/// then the calls return, the tail follows, nothing comes after it, and the port is free.
#[test]
fn a_sink_may_end_itself() {
    for (close, channel) in [(false, false), (true, false), (false, true), (true, true)] {
        let rig = Rig::new("self", channel);
        let id: Arc<OnceLock<u32>> = Arc::default();
        let (m, log, path, own) = (rig.m.clone(), rig.log.clone(), rig.path.clone(), id.clone());
        let sink: Sink<SerialEvent> = Arc::new(move |e| {
            let line = String::from_utf8(data(&e).unwrap()).unwrap();
            if line == "one" {
                let result = if close {
                    m.close(path.clone())
                } else {
                    m.unwatch(*own.wait())
                };
                assert!(result.is_ok());
                note(&log, "returned");
            }
            note(&log, line);
            true
        });
        let watched = rig
            .m
            .watch_framed(rig.path.clone(), watch_opts(NEVER, false), readline(), sink)
            .unwrap();
        id.set(watched).unwrap();
        rig.send(b"one\ntail");
        eventually("the tail came", || rig.log().len() == 3);
        assert_eq!(rig.log(), ["returned", "one", "tail"]);
        assert_eq!(
            kind(rig.m.unwatch(watched).unwrap_err()),
            ErrorKind::NotFound
        );
        if !close {
            let (events, rx) = sink_of();
            rig.m
                .watch(rig.path.clone(), watch_opts(0, false), events)
                .unwrap();
            rig.send(b"next");
            assert_eq!(collect(&rx, 4), b"next");
        }
        let _ = rig.m.close_all();
        rig.device.wait_closed();
        assert_eq!(rig.log().len(), 3);
    }
}

fn sink_of() -> (Sink<SerialEvent>, std::sync::mpsc::Receiver<SerialEvent>) {
    sink()
}

/// Given a port whose old watch sink is busy, beside `watch_core` streams,
/// when bytes came with it, and a stream is unwatched,
/// then the streams have the bytes although the old sink has not returned, and
/// `unwatch_core` does not wait for it.
#[test]
fn core_streams_do_not_stand_behind_the_old_sink() {
    for channel in [false, true] {
        let rig = Rig::new("core", channel);
        let core = |parser| {
            let (events, rx) = sink::<CoreEvent>();
            let path = rig.path.clone();
            let id = rig.m.watch_core(path, watch_opts(0, false), parser, events);
            (id.unwrap(), rx)
        };
        let (_lines, lines_rx) = core(readline());
        let (id, rx) = core(readline());
        let (_, gate) = rig.blocked();
        assert_eq!(next_data(&lines_rx), b"one");
        assert_eq!(next_data(&rx), b"one");
        let unwatch = background(&rig.m, move |m| m.unwatch_core(id));
        ends("unwatch_core beside a busy sink", unwatch).unwrap();
        assert_eq!(next_data(&rx), b"tail");
        assert!(rest(rx).is_empty());
        assert_eq!(rig.log(), ["in one"]);
        drop(gate);
        rig.m.close(rig.port.into()).unwrap();
        assert_eq!(next_data(&lines_rx), b"tail");
    }
}

/// Given sinks that panic: an old watch's, a channel's and a `watch_core` one,
/// when more bytes come and the device goes away,
/// then every other stream gets all of it and its Disconnect, commands work, and only the
/// panicking streams ended.
#[test]
fn a_panicking_sink_is_alone() {
    for channel in [false, true] {
        let rig = Rig::new("panic", channel);
        let (hit, hits) = sink::<()>();
        let panics = move |hit: Sink<()>| {
            hit(());
            panic!("sink panics (expected by this test)")
        };
        let old_hit = hit.clone();
        let old = rig
            .m
            .watch(
                rig.path.clone(),
                watch_opts(0, false),
                Arc::new(move |_| panics(old_hit.clone())),
            )
            .unwrap();
        let core_hit = hit.clone();
        let core = rig
            .m
            .watch_core(
                rig.path.clone(),
                watch_opts(0, false),
                None,
                Arc::new(move |_| panics(core_hit.clone())),
            )
            .unwrap();
        let (events, rx) = sink::<CoreEvent>();
        rig.m
            .watch_core(rig.path.clone(), watch_opts(0, false), None, events)
            .unwrap();
        let (states, states_rx) = sink::<CoreEvent>();
        rig.m.subscribe_core(states);
        let other = channel.then(|| {
            let (events, rx) = sink_of();
            let other = rig.m.open_mux_channel(rig.port.into(), 2).unwrap();
            rig.m.watch(other, watch_opts(0, false), events).unwrap();
            rx
        });
        rig.send(b"one");
        assert_eq!(collect(&rx, 3), b"one");
        next(&hits, "a panicking sink");
        next(&hits, "the other panicking sink");
        rig.send(b"two");
        assert_eq!(collect(&rx, 3), b"two");
        if let Some(rx) = &other {
            rig.device.send(&mux::data(2, b"beside"));
            assert_eq!(collect(rx, 6), b"beside");
        }
        let at = rig.m.at(rig.path.clone(), "AT".into(), None).unwrap();
        assert_eq!(at.status, AtParseStatus::Ok);
        rig.device.fail_reads(io::ErrorKind::BrokenPipe);
        assert_eq!(next_disconnect(&rx).0, rig.path);
        if let Some(rx) = &other {
            next_disconnect(rx);
        }
        pick(&states_rx, "the port's end", |e| {
            matches!(e, CoreEvent::PortState { path, state: PortStatus::Disconnected { .. } }
                if path == rig.port)
            .then_some(())
        });
        assert!(hits.try_recv().is_err(), "called again after its panic");
        rig.m.unwatch(old).unwrap();
        assert!(rig.m.unwatch_core(core).is_err());
        rig.m.close_all().unwrap();
    }
}

/// Given a watch sink that sends a command with a long timeout and no answer, then another
/// (on its own port, on its channel, or on another channel of the port), with a caller
/// queued behind it,
/// when another thread closes the port (or the channel),
/// then the command in flight ends at once as cancelled, the others and the close end too,
/// and the commands not sent yet never are.
#[test]
fn a_close_ends_the_exchange_of_a_sink() {
    for (watched, commanded, closed) in [
        ("hang", "hang", "hang"),
        ("hang#dlci=1", "hang#dlci=1", "hang"),
        ("hang#dlci=1", "hang#dlci=1", "hang#dlci=1"),
        ("hang#dlci=1", "hang#dlci=2", "hang"),
        ("hang#dlci=2", "hang#dlci=1", "hang"),
    ] {
        let channel = watched != "hang";
        let modem = Modem::new().silent("AT+HANG").mux(MuxOptions::default());
        let log = modem.log();
        let device = FakeDevice::with_modem(modem);
        let m = manager_with("hang", device.port());
        if channel {
            m.enable_mux("hang".into(), COMMAND.into(), LONG_MS)
                .unwrap();
            for dlci in [1, 2] {
                m.open_mux_channel("hang".into(), dlci).unwrap();
            }
        }
        let (results, results_rx) = sink::<ErrorKind>();
        let manager = m.clone();
        m.watch(
            watched.into(),
            watch_opts(0, false),
            Arc::new(move |_| {
                for options in [no_prepare(LONG_MS), ExchangeOptions::default()] {
                    let e = manager
                        .exchange(commanded.into(), "AT+HANG\r".into(), options)
                        .unwrap_err();
                    results(kind(e));
                }
                true
            }),
        )
        .unwrap();
        device.send(&match watched {
            "hang#dlci=1" => mux::data(1, b"go"),
            "hang#dlci=2" => mux::data(2, b"go"),
            _ => b"go".to_vec(),
        });
        log.wait_command("AT+HANG");
        let queued = background(&m, move |m| {
            m.exchange(commanded.into(), "AT+QUEUED\r".into(), no_prepare(LONG_MS))
        });
        let close = background(&m, move |m| m.close(closed.into()));
        ends("the close", close).unwrap();
        assert_eq!(next(&results_rx, "the first result"), ErrorKind::Cancelled);
        let second = next(&results_rx, "the second result");
        // Begun later: how it fails depends on how far the close was.
        assert_ne!(second, ErrorKind::Timeout);
        let queued = kind(ends("the queued command", queued).unwrap_err());
        assert_ne!(queued, ErrorKind::Timeout);
        let _ = m.close_all();
        device.wait_closed();
        // Queued in time or not: nothing is sent on a path that is being closed.
        let commands = log.commands();
        assert!(commands.iter().all(|c| c.line != "AT+QUEUED"));
        assert_eq!(commands.iter().filter(|c| c.line == "AT+HANG").count(), 1);
    }
}

/// Given a multiplexed port whose own watch sink is busy with the Disconnect (or with the
/// batch before it),
/// when the device was unplugged,
/// then an exchange and a write waiting on flow control have failed on the channels, although
/// no sink has returned.
#[test]
fn an_unplug_fails_channels_before_any_sink_runs() {
    for with_batch in [false, true] {
        let modem = Modem::new().silent("AT+HANG").mux(MuxOptions::default());
        let modem_log = modem.log();
        let device = FakeDevice::with_modem(modem);
        let m = manager_with("unplug", device.port());
        let log = Log::default();
        let (sink, gate) = gated(&log);
        m.watch("unplug".into(), watch_opts(NEVER, false), sink)
            .unwrap();
        if with_batch {
            device.send(b"batch");
            device.wait_handled();
        }
        m.enable_mux("unplug".into(), COMMAND.into(), LONG_MS)
            .unwrap();
        let first = m.open_mux_channel("unplug".into(), 1).unwrap();
        let held = m.open_mux_channel("unplug".into(), 2).unwrap();
        // At 1 baud a write may wait on flow control for minutes.
        m.set_baud_rate("unplug".into(), 1).unwrap();
        device.send(&mux::control(mux::MSC, &[0x0B, 0x8F]));
        device.wait_handled();
        let write = background(&m, move |m| m.write(held, "never".into()));
        let exchange = background(&m, move |m| {
            m.exchange(first, "AT+HANG\r".into(), no_prepare(LONG_MS))
        });
        modem_log.wait_command("AT+HANG");
        device.fail_reads(io::ErrorKind::BrokenPipe);
        gate.wait_in(if with_batch { "batch" } else { "Disconnect" });
        let e = ends("the exchange", exchange).unwrap_err();
        assert_eq!(kind(e), ErrorKind::Disconnected);
        assert!(ends("the held write", write).is_err());
        assert_eq!(log.lock().unwrap().len(), 1, "a sink returned");
        drop(gate);
        m.close("unplug".into()).unwrap();
    }
}

/// Given a multiplexed port whose device dies, or hangs up, while `close` waits for the
/// answer to its CLD,
/// when the hub sees it,
/// then it is a close: no Disconnect, the state is Closed, the batch is delivered.
#[test]
fn a_device_that_dies_during_the_close_is_closed() {
    for eof in [false, true] {
        let device = FakeDevice::new();
        let mut modem = Modem::new().mux(MuxOptions::default().answer_cld(false));
        let cld = mux::frame(0, true, mux::UIH, &[mux::CLD, 1]);
        device.respond(move |device, bytes| {
            if bytes == cld {
                if eof {
                    device.eof();
                } else {
                    device.fail_reads(io::ErrorKind::BrokenPipe);
                }
            }
            modem.feed(bytes)
        });
        let m = manager_with("dies", device.port());
        let (events, rx) = sink_of();
        m.watch("dies".into(), watch_opts(NEVER, false), events)
            .unwrap();
        device.send(b"batch");
        device.wait_handled();
        let (states, states_rx) = sink::<CoreEvent>();
        m.subscribe_core(states);
        // T2 300 ms: the hub sees the end long before `close` stops it.
        m.enable_mux("dies".into(), "AT+CMUX=0,0,5,31,255,0,30".into(), LONG_MS)
            .unwrap();
        m.close("dies".into()).unwrap();
        assert_eq!(next_state(&states_rx, "dies"), PortStatus::Closed);
        let events = rest(rx);
        assert_eq!(events.len(), 1, "{events:?}");
        assert_eq!(data(&events[0]).unwrap(), b"batch");
        assert!(m.managed_ports().unwrap().is_empty());
        device.wait_closed();
    }
}

/// Given a sink that calls the manager when it is dropped,
/// when its watch ends by `unwatch`, by `close`, or by the next `watch`,
/// then that call works: no lock is held.
#[test]
fn a_sink_may_call_the_manager_as_it_is_dropped() {
    struct Last(SerialManager, String, Arc<Mutex<usize>>);
    impl Drop for Last {
        fn drop(&mut self) {
            // Needs every lock a watch is attached under.
            let (sink, _) = sink_of();
            if let Ok(id) = self.0.watch(self.1.clone(), watch_opts(0, false), sink) {
                let _ = self.0.unwatch(id);
            }
            *self.2.lock().unwrap() += 1;
        }
    }
    for channel in [false, true] {
        let rig = Rig::new("drop", channel);
        let dropped = Arc::new(Mutex::new(0));
        let watch = || {
            let last = Last(rig.m.clone(), rig.path.clone(), dropped.clone());
            let sink: Sink<SerialEvent> = Arc::new(move |_| {
                let _ = &last;
                true
            });
            rig.m.watch(rig.path.clone(), watch_opts(0, false), sink)
        };
        let id = watch().unwrap();
        ends("unwatch", background(&rig.m, move |m| m.unwatch(id))).unwrap();
        eventually("the sink is dropped", || *dropped.lock().unwrap() == 1);
        watch().unwrap();
        let close = background(&rig.m, |m| m.close("drop".into()));
        ends("close", close).unwrap();
        eventually("the sink is dropped", || *dropped.lock().unwrap() == 2);
    }
}
