//! Background bulk-IN reader (2–4 in-flight transfers + RX filter chain).

use crate::error::{ReadOutcome, Result, TransferError, UsbSerialError};
use crate::rx_filter::{apply_filters, RxFilter};
use crate::transport::BulkIn;
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const STOP_JOIN_TIMEOUT: Duration = Duration::from_secs(1);

pub struct SerialReader {
    rx: Receiver<Vec<u8>>,
    stop_tx: Option<Sender<()>>,
    join: Option<JoinHandle<()>>,
    error: Arc<Mutex<Option<UsbSerialError>>>,
}

impl SerialReader {
    pub fn start(
        bulk_in: Box<dyn BulkIn>,
        max_packet_size: u16,
        read_timeout_ms: u32,
        filters: Vec<Box<dyn RxFilter>>,
    ) -> Self {
        let bufsize = (max_packet_size as usize).saturating_mul(4).max(64);
        let (data_tx, data_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let error = Arc::new(Mutex::new(None));
        let err_clone = error.clone();
        let join = thread::spawn(move || {
            reader_loop(
                bulk_in,
                bufsize,
                read_timeout_ms,
                filters,
                &data_tx,
                &stop_rx,
                &err_clone,
            );
        });
        Self {
            rx: data_rx,
            stop_tx: Some(stop_tx),
            join: Some(join),
            error,
        }
    }

    pub fn try_read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if let Some(err) = self.error.lock().unwrap().take() {
            return Err(err);
        }
        match self.rx.try_recv() {
            Ok(chunk) => {
                let n = chunk.len().min(buf.len());
                buf[..n].copy_from_slice(&chunk[..n]);
                Ok(n)
            }
            Err(TryRecvError::Empty) => Ok(0),
            Err(TryRecvError::Disconnected) => {
                if let Some(err) = self.error.lock().unwrap().take() {
                    return Err(err);
                }
                Err(UsbSerialError::Disconnected)
            }
        }
    }

    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(j) = self.join.take() {
            let (done_tx, done_rx) = mpsc::channel();
            thread::spawn(move || {
                let _ = j.join();
                let _ = done_tx.send(());
            });
            let _ = done_rx.recv_timeout(STOP_JOIN_TIMEOUT);
        }
    }
}

impl Drop for SerialReader {
    fn drop(&mut self) {
        self.stop();
    }
}

fn is_stall(err: &UsbSerialError) -> bool {
    matches!(err, UsbSerialError::Io(msg) if msg == "stall")
}

fn reader_loop(
    mut bulk_in: Box<dyn BulkIn>,
    bufsize: usize,
    timeout_ms: u32,
    mut filters: Vec<Box<dyn RxFilter>>,
    data_tx: &Sender<Vec<u8>>,
    stop_rx: &Receiver<()>,
    error: &Arc<Mutex<Option<UsbSerialError>>>,
) {
    let mut buf = vec![0u8; bufsize];
    let mut consecutive_stalls = 0u32;
    loop {
        if stop_rx.try_recv().is_ok() {
            bulk_in.cancel_all();
            break;
        }
        match bulk_in.read(&mut buf, timeout_ms) {
            Ok(ReadOutcome::Data(data)) if !data.is_empty() => {
                consecutive_stalls = 0;
                let filtered = if filters.is_empty() {
                    data
                } else {
                    apply_filters(&mut filters, &data)
                };
                if !filtered.is_empty() && data_tx.send(filtered).is_err() {
                    break;
                }
            }
            Ok(ReadOutcome::TimedOut) | Ok(ReadOutcome::Data(_)) => {}
            Ok(ReadOutcome::Cancelled) => break,
            Err(e) if is_stall(&e) => {
                if consecutive_stalls == 0 {
                    consecutive_stalls += 1;
                    if bulk_in.clear_halt().is_err() {
                        *error.lock().unwrap() = Some(e);
                        break;
                    }
                    continue;
                }
                *error.lock().unwrap() = Some(UsbSerialError::from(TransferError::Stall));
                break;
            }
            Err(e) => {
                *error.lock().unwrap() = Some(e);
                break;
            }
        }
    }
}

#[cfg(all(test, feature = "fake-transport"))]
mod tests {
    use super::*;
    use crate::fake::FakeTransport;
    use crate::transport::Transport;
    use std::time::Instant;

    #[test]
    fn reader_delivers_injected_rx() {
        let fake = FakeTransport::cdc_single_iface();
        fake.push_rx(b"hello");
        let bulk = fake.open_bulk_in(0x81, 64).unwrap();
        let mut reader = SerialReader::start(bulk, 64, 100, vec![]);
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut out = [0u8; 8];
        let mut n = 0;
        while n == 0 && Instant::now() < deadline {
            n = reader.try_read(&mut out).unwrap();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(n, 5);
        assert_eq!(&out[..5], b"hello");
        reader.stop();
    }
}
