//! `SerialReader` model over a scripted bulk IN: each fed read result is
//! processed to quiescence (the endpoint reports when the reader asks for the
//! next one), so every `try_read` has exactly one correct answer — no sleeping,
//! no polling on deadlines. Covers data, filters, timeouts, stalls (cleared or
//! not), errors, cancel, stop and endpoint release.

use super::common::*;
use android_usb_serial::error::Result as UsbResult;
use android_usb_serial::reader::SerialReader;
use android_usb_serial::rx_filter::{FtdiHeaderFilter, RxFilter, XonXoffRxFilter};
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};

const QUIESCE: Duration = Duration::from_secs(5);

#[derive(Debug, Clone)]
enum Item {
    Data(Vec<u8>),
    Timeout,
    Cancelled,
    Stall { clear_ok: bool },
    Fail(UsbSerialError),
}

#[derive(Debug)]
enum Ack {
    /// `read` called after `n` items were consumed.
    Entered(usize),
    Dropped,
}

struct ScriptedIn {
    feed: Receiver<Item>,
    ack: Sender<Ack>,
    consumed: usize,
    clear_ok: bool,
}

impl BulkIn for ScriptedIn {
    fn read(&mut self, _buf: &mut [u8], timeout_ms: u32) -> UsbResult<ReadOutcome> {
        let _ = self.ack.send(Ack::Entered(self.consumed));
        let Ok(item) = self
            .feed
            .recv_timeout(Duration::from_millis(timeout_ms.into()))
        else {
            return Ok(ReadOutcome::TimedOut);
        };
        self.consumed += 1;
        match item {
            Item::Data(d) => Ok(ReadOutcome::Data(d)),
            Item::Timeout => Ok(ReadOutcome::TimedOut),
            Item::Cancelled => Ok(ReadOutcome::Cancelled),
            Item::Stall { clear_ok } => {
                self.clear_ok = clear_ok;
                Err(UsbSerialError::Io("stall".into()))
            }
            Item::Fail(e) => Err(e),
        }
    }

    fn cancel_all(&mut self) {}

    fn clear_halt(&mut self) -> UsbResult<()> {
        if self.clear_ok {
            Ok(())
        } else {
            Err(UsbSerialError::Io("clear halt".into()))
        }
    }
}

impl Drop for ScriptedIn {
    fn drop(&mut self) {
        let _ = self.ack.send(Ack::Dropped);
    }
}

#[derive(Debug, Clone, Copy)]
enum Filters {
    None,
    Ftdi,
    XonXoff,
    Both,
}

#[derive(Debug, Clone)]
enum ReaderAction {
    Feed(Item),
    Read(usize),
    Stop,
}

/// What the consumer must see.
struct ReaderModel {
    mps: usize,
    filters: Filters,
    chunks: VecDeque<Vec<u8>>,
    leftover: Vec<u8>,
    error: Option<UsbSerialError>,
    ended: bool,
    dropped: bool,
    stalls: u32,
}

impl ReaderModel {
    fn filter(&self, d: &[u8]) -> Vec<u8> {
        let mut out = d.to_vec();
        if matches!(self.filters, Filters::Ftdi | Filters::Both) {
            out = out
                .chunks(self.mps)
                .flat_map(|c| c.get(2..).unwrap_or_default().to_vec())
                .collect();
        }
        if matches!(self.filters, Filters::XonXoff | Filters::Both) {
            out.retain(|&b| b != 17 && b != 19);
        }
        out
    }

    fn end(&mut self, error: Option<UsbSerialError>) {
        self.error = error;
        self.ended = true;
    }

    fn feed(&mut self, item: &Item) {
        match item {
            Item::Data(d) if !d.is_empty() => {
                self.stalls = 0;
                let f = self.filter(d);
                if !f.is_empty() {
                    self.chunks.push_back(f);
                }
            }
            Item::Data(_) | Item::Timeout => {}
            Item::Cancelled => self.end(None),
            Item::Stall { clear_ok } => {
                // One stall is cleared and retried; a second in a row (or a failed
                // clear) is final.
                if self.stalls == 0 && *clear_ok {
                    self.stalls = 1;
                } else {
                    self.end(Some(UsbSerialError::Io("stall".into())));
                }
            }
            Item::Fail(e) => self.end(Some(e.clone())),
        }
    }

    /// Data that arrived before an error is delivered before it.
    fn read(&mut self, n: usize) -> Result<Vec<u8>, UsbSerialError> {
        if self.leftover.is_empty() {
            if let Some(c) = self.chunks.pop_front() {
                self.leftover = c;
            } else if let Some(e) = self.error.take() {
                return Err(e);
            } else if self.ended {
                return Err(UsbSerialError::Disconnected);
            } else {
                return Ok(vec![]);
            }
        }
        let k = n.min(self.leftover.len());
        Ok(self.leftover.drain(..k).collect())
    }
}

fn wait_quiet(ack: &Receiver<Ack>, fed: usize) -> Result<bool, TestCaseError> {
    loop {
        match ack.recv_timeout(QUIESCE) {
            Ok(Ack::Entered(n)) if n == fed => return Ok(false),
            Ok(Ack::Entered(_)) => {}
            Ok(Ack::Dropped) => return Ok(true),
            Err(_) => return Err(TestCaseError::fail("reader never came back for more")),
        }
    }
}

fn run_reader(mps: usize, filters: Filters, actions: &[ReaderAction]) -> Result<(), TestCaseError> {
    let (feed_tx, feed) = mpsc::channel();
    let (ack, ack_rx) = mpsc::channel();
    let bulk = ScriptedIn {
        feed,
        ack,
        consumed: 0,
        clear_ok: true,
    };
    let stages: Vec<Box<dyn RxFilter>> = match filters {
        Filters::None => vec![],
        Filters::Ftdi => vec![Box::new(FtdiHeaderFilter::new(mps as u16))],
        Filters::XonXoff => vec![Box::new(XonXoffRxFilter::new(true))],
        Filters::Both => vec![
            Box::new(FtdiHeaderFilter::new(mps as u16)),
            Box::new(XonXoffRxFilter::new(true)),
        ],
    };
    let mut reader = SerialReader::start(Box::new(bulk), mps as u16, 5, stages);
    let mut m = ReaderModel {
        mps,
        filters,
        chunks: VecDeque::new(),
        leftover: vec![],
        error: None,
        ended: false,
        dropped: false,
        stalls: 0,
    };
    let mut fed = 0usize;
    for a in actions {
        match a {
            ReaderAction::Feed(item) => {
                // The model knows the thread is gone: nothing would read the item.
                if m.ended {
                    continue;
                }
                feed_tx.send(item.clone()).unwrap();
                fed += 1;
                m.feed(item);
                let dropped = wait_quiet(&ack_rx, fed)?;
                prop_assert_eq!(
                    dropped,
                    m.ended,
                    "reader thread ended exactly when the model did"
                );
                m.dropped |= dropped;
            }
            ReaderAction::Read(n) => {
                let want = m.read(*n);
                let mut buf = vec![0u8; *n];
                let deadline = Instant::now() + QUIESCE;
                let want = want.map_err(|e| kind(&e));
                let disconnected = Err(kind(&UsbSerialError::Disconnected));
                let got = loop {
                    let got = reader
                        .try_read(&mut buf)
                        .map(|k| buf[..k].to_vec())
                        .map_err(|e| kind(&e));
                    // The thread drops the endpoint just before the channel; give the
                    // channel that instant to report the disconnect.
                    if want == disconnected && got == Ok(vec![]) && Instant::now() < deadline {
                        std::thread::yield_now();
                        continue;
                    }
                    break got;
                };
                prop_assert_eq!(got, want);
            }
            ReaderAction::Stop => {
                reader.stop();
                if !m.dropped {
                    prop_assert!(
                        wait_quiet(&ack_rx, usize::MAX)?,
                        "stop releases the endpoint"
                    );
                    m.dropped = true;
                }
                m.ended = true;
            }
        }
    }
    drop(reader);
    Ok(())
}

fn item(bufsize: usize) -> impl Strategy<Value = Item> {
    let byte = prop_oneof![4 => any::<u8>(), 1 => Just(17u8), 1 => Just(19u8)];
    prop_oneof![
        8 => prop::collection::vec(byte, 0..=bufsize).prop_map(Item::Data),
        1 => Just(Item::Timeout),
        1 => Just(Item::Cancelled),
        3 => any::<bool>().prop_map(|clear_ok| Item::Stall { clear_ok }),
        1 => prop::sample::select(vec![
            UsbSerialError::Disconnected,
            UsbSerialError::Io("gone".into()),
            UsbSerialError::TimedOut,
        ])
        .prop_map(Item::Fail),
    ]
}

fn reader_case() -> impl Strategy<Value = (usize, Filters, Vec<ReaderAction>)> {
    (
        prop::sample::select(vec![8usize, 16, 64]),
        prop::sample::select(vec![
            Filters::None,
            Filters::Ftdi,
            Filters::XonXoff,
            Filters::Both,
        ]),
    )
        .prop_flat_map(|(mps, filters)| {
            let bufsize = (mps * 4).max(64);
            let action = prop_oneof![
                6 => item(bufsize).prop_map(ReaderAction::Feed),
                6 => (0usize..80).prop_map(ReaderAction::Read),
                1 => Just(ReaderAction::Stop),
            ];
            (
                Just(mps),
                Just(filters),
                prop::collection::vec(action, 1..30),
            )
        })
}

#[test]
fn serial_reader_matches_its_model() {
    run_cases(64, reader_case(), |(mps, filters, actions)| {
        run_reader(mps, filters, &actions)
    });
}
