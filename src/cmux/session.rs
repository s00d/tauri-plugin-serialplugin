//! CMUX session: deframe physical RX, route per DLCI, encode TX.

use crate::cmux::frame::{encode_uih, DecodedFrame, Deframer};
use crate::cmux::io::CmuxPhysicalIo;
use crate::events::SerialEvent;
use crate::port_rx_hub::ExchangeWaiter;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use std::time::{Duration, Instant};
use tauri::ipc::Channel;

/// Per-DLCI routing state (virtual channel).
pub struct DlciChannel {
    pub path: String,
    pub exchange_waiter: Mutex<Option<Arc<ExchangeWaiter>>>,
    watch: Mutex<Option<DlciWatchSlot>>,
    stream_buffer: Mutex<Vec<u8>>,
}

struct DlciWatchSlot {
    channel: Channel<SerialEvent>,
    batch_timeout_ms: u64,
    flush_at: Instant,
}

impl DlciChannel {
    fn new(path: String) -> Self {
        Self {
            path,
            exchange_waiter: Mutex::new(None),
            watch: Mutex::new(None),
            stream_buffer: Mutex::new(Vec::new()),
        }
    }

    pub fn attach_watch(&self, channel: Channel<SerialEvent>, batch_timeout_ms: u64) {
        *crate::sync_util::lock_or_recover(&self.watch) = Some(DlciWatchSlot {
            channel,
            batch_timeout_ms,
            flush_at: Instant::now(),
        });
    }

    pub fn detach_watch(&self) {
        *crate::sync_util::lock_or_recover(&self.watch) = None;
        crate::sync_util::lock_or_recover(&self.stream_buffer).clear();
    }

    fn push_stream(&self, payload: &[u8]) {
        let mut guard = crate::sync_util::lock_or_recover(&self.watch);
        let Some(slot) = guard.as_mut() else {
            return;
        };
        self.stream_buffer
            .lock()
            .unwrap()
            .extend_from_slice(payload);
        if slot.flush_at.elapsed() >= Duration::from_millis(slot.batch_timeout_ms) {
            slot.flush_at = Instant::now();
            self.flush_stream_buffer();
        }
    }

    fn flush_stream_buffer(&self) {
        let mut buf = crate::sync_util::lock_or_recover(&self.stream_buffer);
        if buf.is_empty() {
            return;
        }
        if let Some(slot) = crate::sync_util::lock_or_recover(&self.watch).as_ref() {
            let size = buf.len();
            let data = std::mem::take(&mut *buf);
            let path = self.path.clone();
            crate::watch_registry::send_event(
                &slot.channel,
                SerialEvent::Data { path, data, size },
            );
        } else {
            buf.clear();
        }
    }
}

/// Active GSM 07.10 multiplexer on a physical serial port.
pub struct CmuxSession {
    physical_path: String,
    io: Arc<dyn CmuxPhysicalIo>,
    deframer: Mutex<Deframer>,
    channels: Mutex<HashMap<u8, Arc<DlciChannel>>>,
}

impl CmuxSession {
    pub fn new(physical_path: String, io: Arc<dyn CmuxPhysicalIo>) -> Arc<Self> {
        Arc::new(Self {
            physical_path,
            io,
            deframer: Mutex::new(Deframer::default()),
            channels: Mutex::new(HashMap::new()),
        })
    }

    pub fn physical_path(&self) -> &str {
        &self.physical_path
    }

    pub fn register_dlci(&self, dlci: u8, virtual_path: String) -> Arc<DlciChannel> {
        let ch = Arc::new(DlciChannel::new(virtual_path));
        crate::sync_util::lock_or_recover(&self.channels).insert(dlci, ch.clone());
        ch
    }

    pub fn unregister_dlci(&self, dlci: u8) {
        crate::sync_util::lock_or_recover(&self.channels).remove(&dlci);
    }

    pub fn set_watch(&self, dlci: u8, channel: Channel<SerialEvent>, batch_timeout_ms: u64) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            ch.attach_watch(channel, batch_timeout_ms);
        }
    }

    pub fn clear_watch(&self, dlci: u8) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            ch.detach_watch();
        }
    }

    pub fn set_exchange_waiter(&self, dlci: u8, waiter: Arc<ExchangeWaiter>) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            *crate::sync_util::lock_or_recover(&ch.exchange_waiter) = Some(waiter);
        }
    }

    pub fn clear_exchange_waiter(&self, dlci: u8) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            *crate::sync_util::lock_or_recover(&ch.exchange_waiter) = None;
        }
    }

    pub fn send_uih(&self, dlci: u8, payload: &[u8]) -> Result<usize, String> {
        let frame = encode_uih(dlci, payload);
        self.io.write_all(&frame)?;
        Ok(payload.len())
    }

    /// Feed raw bytes from the physical RX hub thread.
    pub fn feed_physical_rx(&self, chunk: &[u8]) {
        let frames = crate::sync_util::lock_or_recover(&self.deframer).feed(chunk);
        for frame in frames {
            self.dispatch_frame(frame);
        }
    }

    fn dispatch_frame(&self, frame: DecodedFrame) {
        let ch = crate::sync_util::lock_or_recover(&self.channels)
            .get(&frame.dlci)
            .cloned();
        let Some(ch) = ch else {
            return;
        };
        let waiter = crate::sync_util::lock_or_recover(&ch.exchange_waiter).clone();
        if let Some(waiter) = waiter {
            waiter.push_bytes(&frame.payload);
            return;
        }
        ch.push_stream(&frame.payload);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
    use crate::exchange_read::ResolvedExchangeOptions;
    use crate::port_rx_hub::ExchangeWaiter;
    use std::sync::atomic::AtomicBool;

    fn test_options() -> ResolvedExchangeOptions {
        ResolvedExchangeOptions {
            timeout_ms: 3000,
            max_bytes: 4096,
            terminators: vec![],
            idle_ms: None,
            rx_prepare: RxPrepareMode::None,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::AtFinalLine,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        }
    }

    #[cfg(unix)]
    #[test]
    fn feed_physical_rx_routes_to_exchange_waiter() {
        use crate::cmux::SerialPortIo;
        use serialport::SerialPort;
        use serialport::TTYPort;
        let (port, _) = TTYPort::pair().expect("pty");
        let session = CmuxSession::new(
            "pty".into(),
            Arc::new(SerialPortIo(Arc::new(Mutex::new(
                Box::new(port) as Box<dyn SerialPort>
            )))),
        );
        session.register_dlci(2, "pty#dlci=2".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let waiter = ExchangeWaiter::new(test_options(), cancel);
        session.set_exchange_waiter(2, waiter.clone());
        let payload = b"AT\r\r\nOK\r\n";
        let frame = encode_uih(2, payload);
        session.feed_physical_rx(&frame);
        let (raw, matched) = waiter.wait(1000).expect("complete");
        assert!(String::from_utf8_lossy(&raw).contains("OK"));
        assert!(matches!(matched, crate::at_parse::ExchangeMatch::Ok));
    }
}
