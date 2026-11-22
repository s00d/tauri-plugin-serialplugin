//! Delivery of the old watches (the plugin's Tauri `Channel`s, [`EventSink`] closures).
//!
//! Each watch is an [`EventStream`]: its sink and the events it has not got yet. Events are
//! queued under the lock of the watch that produced them and delivered with no hub, manager
//! or session lock held, one thread at a time per stream: the hub thread after it routed a
//! chunk, or the caller of `unwatch` / `close` for the tail. A thread that finds a delivery
//! in progress leaves its events to it; [`EventStream::settle`] then waits for them.
//!
//! [`watch_core`](crate::SerialManager::watch_core) streams do not come through here: their
//! sink only pushes into a queue ([`crate::hub::delivery`]).

// False positive on Android targets, whose `thread_local!` hides the `const` block.
#![allow(clippy::missing_const_for_thread_local)]

use crate::events::{EventSink, SerialEvent};
use crate::sync_util::lock_or_recover;
use std::cell::Cell;
use std::collections::VecDeque;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

thread_local! {
    /// Sink calls on this thread's stack (any port, hub or CMUX session).
    static SINK_DEPTH: Cell<usize> = const { Cell::new(0) };
}

/// This thread is inside a sink. It must not wait for another thread's delivery: that one
/// may be waiting for this one (two sinks that unwatch or close each other).
pub(crate) fn in_sink() -> bool {
    SINK_DEPTH.with(Cell::get) > 0
}

/// Call `sink`; `None` when it panicked (caught: the thread it runs on is not its own).
pub(crate) fn call_sink(sink: &EventSink, event: SerialEvent) -> Option<bool> {
    SINK_DEPTH.with(|d| d.set(d.get() + 1));
    let result = catch_unwind(AssertUnwindSafe(|| sink(event)));
    SINK_DEPTH.with(|d| d.set(d.get() - 1));
    if result.is_err() {
        crate::log_warn!("serial event sink panicked; its stream ends");
    }
    result.ok()
}

/// One old watch: its sink and the events it has not got yet.
pub(crate) struct EventStream {
    sink: EventSink,
    state: Mutex<StreamState>,
    /// An event was delivered, or a delivery ended.
    progress: Condvar,
    /// The sink panicked.
    gone: AtomicBool,
}

#[derive(Default)]
struct StreamState {
    queue: VecDeque<SerialEvent>,
    delivering: bool,
    queued: u64,
    delivered: u64,
    /// Nothing more is queued.
    ended: bool,
}

impl StreamState {
    fn push(&mut self, events: &mut Vec<SerialEvent>) {
        if !self.ended {
            self.queued += events.len() as u64;
            self.queue.extend(events.drain(..));
        }
        events.clear();
    }
}

impl EventStream {
    pub(crate) fn gone(&self) -> bool {
        self.gone.load(Ordering::SeqCst)
    }

    fn push(&self, events: &mut Vec<SerialEvent>) {
        if !events.is_empty() {
            lock_or_recover(&self.state).push(events);
        }
    }

    /// Queue the last events.
    fn end(&self, events: &mut Vec<SerialEvent>) {
        let mut s = lock_or_recover(&self.state);
        s.push(events);
        s.ended = true;
    }

    /// Nothing left to deliver, now or later.
    fn is_done(&self) -> bool {
        let s = lock_or_recover(&self.state);
        s.ended && s.queue.is_empty() && !s.delivering
    }

    /// Deliver what is queued now; all of it once the stream ended (a live stream's hub
    /// may queue faster than the sink takes: its next pass delivers the rest).
    fn deliver(&self) {
        let mut s = lock_or_recover(&self.state);
        if s.delivering {
            return;
        }
        s.delivering = true;
        let target = s.queued;
        while s.ended || s.delivered < target {
            let Some(event) = s.queue.pop_front() else {
                break;
            };
            drop(s);
            let result = call_sink(&self.sink, event);
            if result == Some(false) {
                crate::log_warn!("Failed to send serial event: receiver is gone");
            }
            s = lock_or_recover(&self.state);
            s.delivered += 1;
            if result.is_none() {
                self.gone.store(true, Ordering::SeqCst);
                s.ended = true;
                s.delivered = s.queued;
                s.queue.clear();
            }
            self.progress.notify_all();
        }
        s.delivering = false;
        self.progress.notify_all();
    }

    /// What was queued before this call is delivered when it returns, by this thread or by
    /// the one delivering now. Inside a sink it does not wait for another delivery.
    pub(crate) fn settle(&self) {
        let target = lock_or_recover(&self.state).queued;
        loop {
            self.deliver();
            let s = lock_or_recover(&self.state);
            if s.delivered >= target {
                return;
            }
            if s.delivering {
                if in_sink() {
                    return;
                }
                drop(self.progress.wait(s));
            }
        }
    }
}

/// The old watches of one hub (or one CMUX session) that may still have events to deliver.
#[derive(Default)]
pub struct Outbox {
    streams: Mutex<Vec<Arc<EventStream>>>,
}

impl Outbox {
    /// A stream for `sink`, delivered from here until it ended.
    pub(crate) fn stream(&self, sink: EventSink) -> Arc<EventStream> {
        let stream = Arc::new(EventStream {
            sink,
            state: Mutex::default(),
            progress: Condvar::new(),
            gone: AtomicBool::new(false),
        });
        lock_or_recover(&self.streams).push(stream.clone());
        stream
    }

    /// Deliver queued events. Call with no lock held.
    pub(crate) fn deliver(&self) {
        let streams = lock_or_recover(&self.streams).clone();
        for stream in &streams {
            stream.deliver();
        }
        self.forget_done(&streams);
    }

    /// [`EventStream::settle`] of one stream. Call with no lock held.
    pub(crate) fn settle(&self, stream: &Arc<EventStream>) {
        stream.settle();
        self.forget_done(std::slice::from_ref(stream));
    }

    /// [`EventStream::settle`] of every stream. Call with no lock held.
    pub(crate) fn settle_all(&self) {
        let streams = lock_or_recover(&self.streams).clone();
        streams.iter().for_each(|s| s.settle());
        self.forget_done(&streams);
    }

    fn forget_done(&self, streams: &[Arc<EventStream>]) {
        for done in streams.iter().filter(|s| s.is_done()) {
            lock_or_recover(&self.streams).retain(|s| !Arc::ptr_eq(s, done));
        }
    }
}

/// What owns an attached old watch: a hub, or a CMUX channel.
pub(crate) trait WatchOwner: Send + Sync {
    /// Detach `stream` (if still attached) with its tail queued, and settle it.
    fn stop(&self, stream: &Arc<EventStream>);
}

/// Where the events of a watch go.
pub(crate) enum Target {
    Old(Arc<EventStream>),
    /// `watch_core`: the sink only queues, so it is called at once.
    Core {
        sink: EventSink,
        /// Cleared once the subscriber is gone.
        alive: Arc<AtomicBool>,
    },
}

impl Target {
    pub(crate) fn is(&self, stream: &Arc<EventStream>) -> bool {
        matches!(self, Target::Old(s) if Arc::ptr_eq(s, stream))
    }

    pub(crate) fn gone(&self) -> bool {
        match self {
            Target::Old(stream) => stream.gone(),
            Target::Core { alive, .. } => !alive.load(Ordering::SeqCst),
        }
    }

    /// Send `events` (emptied). Call under the watch's lock, so they go in production
    /// order and none after its last.
    pub(crate) fn send(&self, events: &mut Vec<SerialEvent>) {
        match self {
            Target::Old(stream) => stream.push(events),
            Target::Core { sink, .. } => events.drain(..).for_each(|e| {
                sink(e);
            }),
        }
    }

    /// [`Self::send`] the last events.
    pub(crate) fn end(&self, events: &mut Vec<SerialEvent>) {
        match self {
            Target::Old(stream) => stream.end(events),
            Target::Core { .. } => self.send(events),
        }
    }
}
