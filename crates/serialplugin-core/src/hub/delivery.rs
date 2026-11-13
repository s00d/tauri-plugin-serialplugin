//! Delivery threads of the core receivers ([`CoreSink`]: `watch_core`,
//! `subscribe_core`): the hub only queues, so a slow sink never stalls RX, `at()` or
//! `close()`. Each queue is bounded by bytes; on overflow the sink gets
//! [`CoreEvent::Overflow`] and the queue closes (the subscription is gone). The worker
//! drains what is queued and ends once every sender is dropped.
//!
//! The old `watch` ([`crate::events::EventSink`], the plugin's Tauri `Channel`) does
//! not come through here: as in 4.x its sink runs on the RX side.

use crate::events::{CoreEvent, CoreSink, SerialEvent};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Approximate queue cost of an event (payload bytes plus a fixed overhead).
fn cost(event: &CoreEvent) -> usize {
    const OVERHEAD: usize = 64;
    OVERHEAD
        + match event {
            CoreEvent::Event(SerialEvent::Data { data, .. }) => data.len(),
            CoreEvent::Event(SerialEvent::Urc { line, .. }) => line.len(),
            CoreEvent::Event(SerialEvent::Error { message, .. }) => message.len(),
            CoreEvent::Event(SerialEvent::Disconnect { reason, .. }) => reason.len(),
            _ => 0,
        }
}

/// Bounded queue + delivery thread of one core subscription.
pub(crate) struct CoreQueue {
    tx: Mutex<Option<Sender<(CoreEvent, usize)>>>,
    queued: Arc<AtomicUsize>,
    capacity: usize,
    /// `Data` bytes accepted so far (the `Overflow` offset).
    offset: AtomicU64,
    alive: Arc<AtomicBool>,
}

impl CoreQueue {
    pub(crate) fn spawn(sink: CoreSink, capacity: usize) -> Arc<Self> {
        let (tx, rx) = mpsc::channel::<(CoreEvent, usize)>();
        let queued = Arc::new(AtomicUsize::new(0));
        let alive = Arc::new(AtomicBool::new(true));
        let (q, a) = (queued.clone(), alive.clone());
        let spawned = thread::Builder::new()
            .name("serial-core-delivery".into())
            .spawn(move || {
                for (event, size) in rx {
                    q.fetch_sub(size, Ordering::SeqCst);
                    if !sink(event) {
                        a.store(false, Ordering::SeqCst);
                        break;
                    }
                }
            });
        if spawned.is_err() {
            alive.store(false, Ordering::SeqCst);
        }
        Arc::new(Self {
            tx: Mutex::new(Some(tx)),
            queued,
            capacity: capacity.max(1),
            offset: AtomicU64::new(0),
            alive,
        })
    }

    pub(crate) fn alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub(crate) fn alive_flag(&self) -> Arc<AtomicBool> {
        self.alive.clone()
    }

    /// Queue `event`; `false` once the subscription is gone (overflow or sink said no).
    pub(crate) fn push(&self, event: CoreEvent) -> bool {
        let mut guard = crate::sync_util::lock_or_recover(&self.tx);
        let Some(tx) = guard.as_ref().filter(|_| self.alive()) else {
            return false;
        };
        let size = cost(&event);
        let queued = self.queued.load(Ordering::SeqCst);
        if queued > 0 && queued + size > self.capacity {
            let dropped_from = self.offset.load(Ordering::SeqCst);
            let _ = tx.send((CoreEvent::Overflow { dropped_from }, 0));
            *guard = None;
            self.alive.store(false, Ordering::SeqCst);
            crate::log_warn!("core subscriber queue overflow at byte {dropped_from}; unsubscribed");
            return false;
        }
        if let CoreEvent::Event(SerialEvent::Data { data, .. }) = &event {
            self.offset.fetch_add(data.len() as u64, Ordering::SeqCst);
        }
        self.queued.fetch_add(size, Ordering::SeqCst);
        if tx.send((event, size)).is_err() {
            self.alive.store(false, Ordering::SeqCst);
            return false;
        }
        true
    }

    /// Stop accepting events; the worker delivers what is queued and ends.
    pub(crate) fn close(&self) {
        crate::sync_util::lock_or_recover(&self.tx).take();
    }

    /// An [`crate::events::EventSink`] that queues watch events here as
    /// [`CoreEvent::Event`].
    pub(crate) fn event_sink(self: &Arc<Self>) -> crate::events::EventSink {
        let queue = self.clone();
        Arc::new(move |event| queue.push(CoreEvent::Event(event)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::Receiver;
    use std::time::Duration;

    fn ev(e: SerialEvent) -> CoreEvent {
        CoreEvent::Event(e)
    }

    fn data(n: usize) -> CoreEvent {
        ev(SerialEvent::Data {
            path: "p".into(),
            data: vec![0; n],
            size: n,
        })
    }

    #[test]
    fn cost_counts_the_payload_of_every_event() {
        let p = || "p".to_string();
        assert_eq!(cost(&data(10)), 74);
        assert_eq!(
            cost(&ev(SerialEvent::Urc {
                path: p(),
                line: "abcd".into()
            })),
            68
        );
        assert_eq!(
            cost(&ev(SerialEvent::Error {
                path: p(),
                message: "abcde".into()
            })),
            69
        );
        assert_eq!(
            cost(&ev(SerialEvent::Disconnect {
                path: p(),
                reason: "abcdef".into()
            })),
            70
        );
        assert_eq!(cost(&CoreEvent::Overflow { dropped_from: 9 }), 64);
    }

    /// A sink that blocks until released; `entered` reports each call.
    fn gated() -> (CoreSink, Receiver<()>, mpsc::Sender<()>) {
        let (entered_tx, entered) = mpsc::channel();
        let (release, release_rx) = mpsc::channel::<()>();
        let entered_tx = Mutex::new(entered_tx);
        let release_rx = Mutex::new(release_rx);
        let sink: CoreSink = Arc::new(move |_| {
            let _ = entered_tx.lock().unwrap().send(());
            let _ = release_rx.lock().unwrap().recv();
            true
        });
        (sink, entered, release)
    }

    /// Capacity is checked as `queued + size > capacity`, and an event bigger than
    /// the whole queue still goes through when nothing is queued.
    #[test]
    fn capacity_is_counted_in_bytes_of_queued_events() {
        let (sink, entered, release) = gated();
        let q = CoreQueue::spawn(sink, 130);
        assert!(
            q.push(data(500)),
            "nothing queued: accepted whatever its size"
        );
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        // The worker holds the first event (no longer queued).
        assert!(q.push(data(1))); // 65 queued
        assert!(q.push(data(1)), "65 + 65 = 130 fits"); // 130 queued
        assert!(!q.push(data(1)), "195 > 130 overflows");
        assert!(!q.alive());
        drop(release);
    }

    #[test]
    fn sink_saying_no_ends_the_queue_and_close_stops_pushes() {
        let q = CoreQueue::spawn(Arc::new(|_| false), 1 << 20);
        assert!(q.alive());
        q.push(data(1));
        let t = std::time::Instant::now();
        while q.alive() {
            assert!(t.elapsed() < Duration::from_secs(2), "still alive");
            thread::sleep(Duration::from_millis(1));
        }
        let q = CoreQueue::spawn(Arc::new(|_| true), 1 << 20);
        q.close();
        assert!(!q.push(data(1)), "push after close");
    }
}
