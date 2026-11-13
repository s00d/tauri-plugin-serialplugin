//! Pieces shared by the btleplug BLE port (desktop/iOS) and the Android GATT port:
//! the `ble://` scheme and the notification → `Read` queue.

use std::io;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::time::Duration;

pub const BLE_SCHEME: &str = "ble://";

/// Notification bytes a [`rx_channel`] port keeps queued but unread. Past this the
/// newest chunks are dropped (with one warning per overflow run): a port that is open
/// but not read must not grow without bound. The hub normally reads all the time.
pub(crate) const RX_QUEUE_CAP: usize = 1 << 20;

/// Largest attribute value (Core spec Vol 3 Part F §3.2.9): a write longer than this
/// fails or is cut, whatever the MTU.
const MAX_ATTRIBUTE_LEN: usize = 512;

/// Bytes per ATT write: MTU minus the 3-byte ATT header (default MTU 23), at most 512.
#[cfg_attr(target_os = "android", allow(dead_code))] // Android chunks in Kotlin
pub(crate) fn chunk_size(mtu: u16) -> usize {
    (usize::from(mtu.max(23)) - 3).min(MAX_ATTRIBUTE_LEN)
}

/// Every Nth chunk is written with response when RX allows both write types, so the
/// peripheral can push back before its buffer overflows (as Adafruit's UART apps do).
pub(crate) const WRITE_PACE_EVERY: usize = 8;

/// Chooses the write type per chunk (see [`WRITE_PACE_EVERY`]).
#[derive(Debug, Default)]
#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) struct WritePacer {
    sent: usize,
}

#[cfg_attr(target_os = "android", allow(dead_code))]
impl WritePacer {
    /// Whether the next chunk is written with response.
    pub(crate) fn with_response(&mut self, can_with: bool, can_without: bool) -> bool {
        self.sent += 1;
        match (can_with, can_without) {
            (true, true) => self.sent.is_multiple_of(WRITE_PACE_EVERY),
            (_, false) => true,
            (false, true) => false,
        }
    }
}

/// `ble://<target>` names this device: its platform id (case-insensitive), its MAC
/// (case-insensitive; Apple hides it as all zeros) or its advertised name (exact).
#[cfg_attr(target_os = "android", allow(dead_code))]
pub(crate) fn target_matches(id: &str, address: &str, name: Option<&str>, target: &str) -> bool {
    id.eq_ignore_ascii_case(target)
        || (address != "00:00:00:00:00:00" && address.eq_ignore_ascii_case(target))
        || name == Some(target)
}

/// `ble://<id>` → `<id>`, or `None` for other paths.
pub fn ble_target(path: &str) -> Option<&str> {
    path.strip_prefix(BLE_SCHEME)
}

/// Sender half of [`rx_channel`]: never blocks (it runs on a Bluetooth binder thread).
pub(crate) struct RxFeed {
    tx: Sender<Vec<u8>>,
    queued: Arc<AtomicUsize>,
    cap: usize,
    dropping: AtomicBool,
}

impl RxFeed {
    /// Queue `chunk`; `false` when it was dropped because the queue is at its cap.
    pub(crate) fn push(&self, chunk: Vec<u8>) -> bool {
        let n = chunk.len();
        if self.queued.load(Ordering::Acquire) + n > self.cap {
            // Warn once per overflow run.
            if self.dropping.swap(true, Ordering::Relaxed) {
                return false;
            }
            crate::log_warn!(
                "[BLE] RX queue full ({} bytes unread): dropping notifications until it is read",
                self.cap
            );
            return false;
        }
        self.dropping.store(false, Ordering::Relaxed);
        self.queued.fetch_add(n, Ordering::AcqRel);
        if self.tx.send(chunk).is_err() {
            self.queued.fetch_sub(n, Ordering::AcqRel);
        }
        true
    }
}

/// A byte-capped notification queue; [`RxQueue::buffered`] counts queued chunks too.
pub(crate) fn rx_channel(cap: usize) -> (RxFeed, RxQueue) {
    let (tx, rx) = mpsc::channel();
    let queued = Arc::new(AtomicUsize::new(0));
    let feed = RxFeed {
        tx,
        queued: queued.clone(),
        cap,
        dropping: AtomicBool::new(false),
    };
    let mut q = RxQueue::new(rx);
    q.queued = Some(queued);
    (feed, q)
}

/// Notification chunks → `Read` with the serial timeout semantics the hub expects.
/// Dropping the sender (device gone) turns the next read into `UnexpectedEof`,
/// which the hub reports as `Disconnect`.
pub(crate) struct RxQueue {
    rx: Receiver<Vec<u8>>,
    pending: Vec<u8>,
    /// Bytes sent but not yet received (only for [`rx_channel`] queues).
    queued: Option<Arc<AtomicUsize>>,
}

impl RxQueue {
    pub(crate) fn new(rx: Receiver<Vec<u8>>) -> Self {
        Self {
            rx,
            pending: Vec::new(),
            queued: None,
        }
    }

    fn took(&self, chunk: &[u8]) {
        if let Some(q) = &self.queued {
            q.fetch_sub(chunk.len(), Ordering::AcqRel);
        }
    }

    pub(crate) fn read(&mut self, buf: &mut [u8], timeout: Duration) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.rx.recv_timeout(timeout.max(Duration::from_millis(1))) {
                Ok(chunk) => {
                    self.took(&chunk);
                    if chunk.is_empty() {
                        // An empty notification: no data, never `Ok(0)` (that is EOF).
                        return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE read timeout"));
                    }
                    self.pending = chunk;
                }
                Err(RecvTimeoutError::Timeout) => {
                    return Err(io::Error::new(io::ErrorKind::TimedOut, "BLE read timeout"))
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "BLE device disconnected",
                    ))
                }
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }

    pub(crate) fn clear(&mut self) {
        self.pending.clear();
        while let Ok(chunk) = self.rx.try_recv() {
            self.took(&chunk);
        }
    }

    /// Unread bytes: the split chunk plus, for [`rx_channel`] queues, the queued ones.
    pub(crate) fn buffered(&self) -> usize {
        self.pending.len()
            + self
                .queued
                .as_ref()
                .map_or(0, |q| q.load(Ordering::Acquire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn rx_queue_splits_chunks_and_reports_timeout_and_disconnect() {
        let (tx, rx) = mpsc::channel();
        let mut q = RxQueue::new(rx);
        tx.send(b"hello".to_vec()).unwrap();
        let mut buf = [0u8; 3];
        assert_eq!(q.read(&mut buf, Duration::from_millis(10)).unwrap(), 3);
        assert_eq!(&buf, b"hel");
        assert_eq!(q.buffered(), 2);
        assert_eq!(q.read(&mut buf, Duration::from_millis(10)).unwrap(), 2);
        assert_eq!(&buf[..2], b"lo");
        let e = q.read(&mut buf, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        drop(tx);
        let e = q.read(&mut buf, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
        q.clear();
    }

    /// An empty notification is no data, not `Ok(0)` (the hub reads that as EOF).
    #[test]
    fn empty_notification_is_a_timeout_not_eof() {
        let (tx, rx) = mpsc::channel();
        let mut q = RxQueue::new(rx);
        tx.send(Vec::new()).unwrap();
        let mut buf = [0u8; 4];
        let e = q.read(&mut buf, Duration::from_millis(10)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
    }

    #[test]
    fn rx_channel_counts_queued_bytes_and_drops_over_cap() {
        let (feed, mut q) = rx_channel(8);
        assert!(feed.push(b"hello".to_vec()));
        assert_eq!(q.buffered(), 5, "queued chunk counts before any read");
        assert!(!feed.push(b"world".to_vec()), "5 + 5 > cap 8: dropped");
        assert!(feed.push(b"abc".to_vec()));
        assert_eq!(q.buffered(), 8);
        let mut buf = [0u8; 2];
        assert_eq!(q.read(&mut buf, Duration::from_millis(10)).unwrap(), 2);
        assert_eq!(q.buffered(), 6, "3 pending + 3 queued");
        assert!(feed.push(b"xy".to_vec()), "room again after a read");
        q.clear();
        assert_eq!(q.buffered(), 0);
        let mut all = [0u8; 8];
        let e = q.read(&mut all, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::TimedOut);
        drop(feed);
        let e = q.read(&mut all, Duration::from_millis(5)).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn chunks_follow_mtu_capped_at_the_attribute_size() {
        assert_eq!(chunk_size(23), 20);
        assert_eq!(chunk_size(0), 20);
        assert_eq!(chunk_size(247), 244);
        // Android 14+ always negotiates 517; Linux / Windows report up to 527.
        assert_eq!(chunk_size(517), 512);
        assert_eq!(chunk_size(527), 512);
    }

    #[test]
    fn every_eighth_chunk_waits_for_a_response_when_rx_allows_both() {
        let mut p = WritePacer::default();
        let got: Vec<bool> = (0..16).map(|_| p.with_response(true, true)).collect();
        let want: Vec<bool> = (1..=16_usize)
            .map(|i| i.is_multiple_of(WRITE_PACE_EVERY))
            .collect();
        assert_eq!(got, want);
        let mut p = WritePacer::default();
        assert!(
            (0..9).all(|_| p.with_response(true, false)),
            "no write-without-response"
        );
        let mut p = WritePacer::default();
        assert!(
            (0..9).all(|_| !p.with_response(false, true)),
            "no write-with-response"
        );
    }

    #[test]
    fn target_matches_id_mac_in_any_case_or_name() {
        let m = |t| target_matches("hci0/dev_AA_BB", "AA:BB:CC:DD:EE:FF", Some("Nus Board"), t);
        assert!(m("hci0/dev_AA_BB"));
        assert!(m("aa:bb:cc:dd:ee:ff"));
        assert!(m("AA:BB:CC:DD:EE:FF"));
        assert!(m("Nus Board"));
        assert!(!m("nus board"), "names are exact");
        assert!(!m("AA:BB"));
        // Apple hides the MAC (all zero): never matches by address.
        assert!(!target_matches(
            "uuid",
            "00:00:00:00:00:00",
            None,
            "00:00:00:00:00:00"
        ));
        assert!(target_matches(
            "5D2A-UUID",
            "00:00:00:00:00:00",
            None,
            "5d2a-uuid"
        ));
    }

    #[test]
    fn ble_target_strips_scheme() {
        assert_eq!(ble_target("ble://AA:BB"), Some("AA:BB"));
        assert_eq!(ble_target("/dev/ttyUSB0"), None);
    }
}
