//! CMUX session: deframe physical RX, route per DLCI, encode TX.

use crate::cmux::frame::{
    encode_frame, encode_uih, DecodedFrame, Deframer, DISC, DM, PF, SABM, UA, UI, UIH,
};
use crate::cmux::io::CmuxPhysicalIo;
use crate::events::SerialEvent;
use crate::hub::ExchangeWaiter;
use std::collections::HashMap;
use std::sync::mpsc;
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
        let due = {
            let mut guard = crate::sync_util::lock_or_recover(&self.watch);
            let Some(slot) = guard.as_mut() else {
                return;
            };
            crate::sync_util::lock_or_recover(&self.stream_buffer).extend_from_slice(payload);
            slot.take_due()
        };
        // Flush outside the `watch` lock: flush_stream_buffer takes it again.
        if due {
            self.flush_stream_buffer();
        }
    }

    /// Flush a batch whose timeout elapsed with no new frame to trigger it.
    fn tick(&self) {
        let due = crate::sync_util::lock_or_recover(&self.watch)
            .as_mut()
            .is_some_and(DlciWatchSlot::take_due);
        if due {
            self.flush_stream_buffer();
        }
    }

    fn flush_stream_buffer(&self) {
        let data = std::mem::take(&mut *crate::sync_util::lock_or_recover(&self.stream_buffer));
        if data.is_empty() {
            return;
        }
        let channel = crate::sync_util::lock_or_recover(&self.watch)
            .as_ref()
            .map(|slot| slot.channel.clone());
        if let Some(channel) = channel {
            let size = data.len();
            crate::port::watch_registry::send_event(
                &channel,
                SerialEvent::Data {
                    path: self.path.clone(),
                    data,
                    size,
                },
            );
        }
    }
}

impl DlciWatchSlot {
    fn take_due(&mut self) -> bool {
        if self.flush_at.elapsed() >= Duration::from_millis(self.batch_timeout_ms) {
            self.flush_at = Instant::now();
            true
        } else {
            false
        }
    }
}

/// Active GSM 07.10 multiplexer on a physical serial port.
pub struct CmuxSession {
    physical_path: String,
    io: Arc<dyn CmuxPhysicalIo>,
    deframer: Mutex<Deframer>,
    channels: Mutex<HashMap<u8, Arc<DlciChannel>>>,
    /// SABM sent, waiting for UA (true) or DM (false).
    pending_open: Mutex<HashMap<u8, mpsc::Sender<bool>>>,
}

impl CmuxSession {
    pub fn new(physical_path: String, io: Arc<dyn CmuxPhysicalIo>) -> Arc<Self> {
        Arc::new(Self {
            physical_path,
            io,
            deframer: Mutex::new(Deframer::default()),
            channels: Mutex::new(HashMap::new()),
            pending_open: Mutex::new(HashMap::new()),
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
        if crate::sync_util::lock_or_recover(&self.channels)
            .remove(&dlci)
            .is_some()
        {
            let _ = self.io.write_all(&encode_frame(dlci, true, DISC | PF, &[]));
        }
    }

    pub fn has_dlci(&self, dlci: u8) -> bool {
        crate::sync_util::lock_or_recover(&self.channels).contains_key(&dlci)
    }

    /// Establish a DLC: send SABM and wait for the modem's UA (DM = refused).
    /// DLCI 0 (the control channel) must be opened first.
    pub fn open_dlci(&self, dlci: u8, timeout: Duration) -> Result<(), String> {
        let (tx, rx) = mpsc::channel();
        crate::sync_util::lock_or_recover(&self.pending_open).insert(dlci, tx);
        let sent = self.io.write_all(&encode_frame(dlci, true, SABM | PF, &[]));
        let result = sent.and_then(|()| match rx.recv_timeout(timeout) {
            Ok(true) => Ok(()),
            Ok(false) => Err(format!("modem refused CMUX DLCI {dlci} (DM)")),
            Err(_) => Err(format!(
                "no UA for CMUX DLCI {dlci} within {} ms",
                timeout.as_millis()
            )),
        });
        crate::sync_util::lock_or_recover(&self.pending_open).remove(&dlci);
        result
    }

    /// Ask the modem to leave multiplexer mode (CLD on the control channel).
    pub fn close_down(&self) {
        // CLD command: type octet 0xC3 (EA=1, C/R=1, type 0x30), length 0.
        let _ = self
            .io
            .write_all(&encode_frame(0, true, UIH, &[0xC3, 0x01]));
    }

    /// Batch-timeout flush for virtual-channel watches (called from the hub tick).
    pub fn tick(&self) {
        let channels: Vec<_> = crate::sync_util::lock_or_recover(&self.channels)
            .values()
            .cloned()
            .collect();
        for ch in channels {
            ch.tick();
        }
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

    /// Fail the in-flight exchange waiter on `dlci` (used by `cancel_exchange`).
    pub fn cancel_active_exchange(&self, dlci: u8) {
        if let Some(ch) = crate::sync_util::lock_or_recover(&self.channels).get(&dlci) {
            if let Some(waiter) = crate::sync_util::lock_or_recover(&ch.exchange_waiter).as_ref() {
                waiter.fail_with_reason("exchange cancelled".into());
            }
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
        match frame.kind {
            UA | DM => {
                if let Some(tx) =
                    crate::sync_util::lock_or_recover(&self.pending_open).get(&frame.dlci)
                {
                    let _ = tx.send(frame.kind == UA);
                }
                return;
            }
            UIH | UI => {}
            // SABM/DISC from the modem: we are the initiator, nothing to accept.
            _ => return,
        }
        if frame.dlci == 0 {
            self.handle_control(&frame.payload);
            return;
        }
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

    /// Control-channel message from the modem. Commands (MSC, TEST, FCon/FCoff …) are
    /// acknowledged by echoing them back with the type octet's C/R bit cleared, as
    /// 27.010 §5.4.6 requires; responses to our own commands need no action.
    fn handle_control(&self, payload: &[u8]) {
        let Some(&type_octet) = payload.first() else {
            return;
        };
        if type_octet & 0x02 == 0 {
            return;
        }
        let mut reply = payload.to_vec();
        reply[0] &= !0x02;
        let _ = self.io.write_all(&encode_frame(0, true, UIH, &reply));
    }
}

#[cfg(all(test, not(target_os = "android")))]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, ExchangeCompletionMode, RxPrepareMode};
    use crate::exchange::options::ResolvedExchangeOptions;
    use crate::hub::ExchangeWaiter;
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
        use crate::sync_util::pty_pair_locked;
        use serialport::SerialPort;
        let (_pty, port, _master) = pty_pair_locked();
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
        assert!(matches!(matched, crate::at::parse::ExchangeMatch::Ok));
    }

    #[test]
    fn cancel_active_exchange_fails_waiter_quickly() {
        use crate::cmux::io::CmuxPhysicalIo;
        struct NoopIo;
        impl CmuxPhysicalIo for NoopIo {
            fn write_all(&self, _data: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }
        let session = CmuxSession::new("mock".into(), Arc::new(NoopIo));
        session.register_dlci(2, "mock#dlci=2".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let waiter = ExchangeWaiter::new(test_options(), cancel);
        session.set_exchange_waiter(2, waiter.clone());
        let waiter_bg = waiter.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let done_bg = done.clone();
        std::thread::spawn(move || {
            let _ = waiter_bg.wait(5000);
            done_bg.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        std::thread::sleep(std::time::Duration::from_millis(20));
        let start = std::time::Instant::now();
        session.cancel_active_exchange(2);
        while !done.load(std::sync::atomic::Ordering::SeqCst)
            && start.elapsed() < std::time::Duration::from_millis(500)
        {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(done.load(std::sync::atomic::Ordering::SeqCst));
        assert!(start.elapsed() < std::time::Duration::from_millis(500));
    }
}
