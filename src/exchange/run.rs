//! Run exchange on a physical port or CMUX virtual channel.

use crate::at::parse::ExchangeResponse;
use crate::cmux::CmuxSession;
use crate::error::Error;
use crate::events::{ExchangeOptions, RxPrepareMode};
use crate::exchange::io::ExchangeIo;
use crate::exchange::options::ReadUntilOutcome;
use crate::hub::handle::RxHubHandle;
use crate::hub::ExchangeWaiter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Run exchange on a physical port through the RX hub.
pub fn run_physical_exchange<H: RxHubHandle>(
    hub: &H,
    io: &dyn ExchangeIo,
    command: &str,
    user_solicited: &[String],
    payload: Vec<u8>,
    options: ExchangeOptions,
    exchange_cancel: Arc<AtomicBool>,
) -> Result<ExchangeResponse, Error> {
    let mut opts = options;
    if opts.command.is_none() {
        opts.command = Some(command.to_string());
    }
    let resolved = opts.resolve();
    exchange_cancel.store(false, Ordering::SeqCst);
    let cancel = exchange_cancel.clone();
    let hub_shared = hub.shared();

    match resolved.rx_prepare {
        RxPrepareMode::Purge => {
            hub_shared.purge_buffers();
            io.purge_rx()?;
        }
        RxPrepareMode::Drain => {
            hub_shared
                .drain(
                    resolved.drain_idle_ms,
                    resolved.drain_max_ms,
                    cancel.clone(),
                    resolved.solicited_prefixes.clone(),
                )
                .map_err(Error::String)?;
        }
        RxPrepareMode::None => {}
    }

    let waiter = ExchangeWaiter::new(resolved.clone(), cancel.clone());
    hub.set_exchange_waiter(waiter.clone());

    if cancel.load(Ordering::SeqCst) {
        hub.clear_exchange_waiter();
        return Err(Error::String("exchange cancelled".into()));
    }

    if let Err(e) = io.write_payload(&payload) {
        hub.clear_exchange_waiter();
        return Err(e);
    }

    let stale = hub_shared.take_idle_bytes();
    if !stale.is_empty() {
        waiter.push_bytes(&stale);
    }

    let wait_result = waiter.wait(resolved.timeout_ms);
    hub.clear_exchange_waiter();
    let (raw, matched) = wait_result.map_err(Error::String)?;
    let outcome = ReadUntilOutcome { raw, matched };
    Ok(ExchangeResponse::from_raw(
        outcome.raw,
        outcome.matched,
        command,
        user_solicited,
        resolved.result_format,
    ))
}

/// CMUX virtual channel exchange (shared desktop + mobile).
pub fn run_mux_exchange(
    session: &CmuxSession,
    dlci: u8,
    command: &str,
    user_solicited: &[String],
    payload: Vec<u8>,
    options: ExchangeOptions,
    exchange_cancel: Arc<AtomicBool>,
) -> Result<ExchangeResponse, Error> {
    let mut opts = options;
    if opts.command.is_none() {
        opts.command = Some(command.to_string());
    }
    let resolved = opts.resolve();
    exchange_cancel.store(false, Ordering::SeqCst);
    let cancel = exchange_cancel.clone();
    let waiter = ExchangeWaiter::new(resolved.clone(), cancel);
    session.set_exchange_waiter(dlci, waiter.clone());
    session.send_uih(dlci, &payload).map_err(Error::String)?;
    let wait_result = waiter.wait(resolved.timeout_ms);
    session.clear_exchange_waiter(dlci);
    let (raw, matched) = wait_result.map_err(Error::String)?;
    Ok(ExchangeResponse::from_raw(
        raw,
        matched,
        command,
        user_solicited,
        resolved.result_format,
    ))
}

#[cfg(test)]
mod parity_tests {
    use super::*;
    use crate::at::parse::AtParseStatus;
    use crate::events::{AtResultFormat, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode};
    use std::sync::atomic::AtomicBool;
    use std::time::Duration;

    #[test]
    fn cmux_virtual_exchange_on_mock() {
        use crate::cmux::encode_uih;
        use crate::cmux::CmuxPhysicalIo;
        use crate::cmux::CmuxSession;

        struct NoopIo;
        impl CmuxPhysicalIo for NoopIo {
            fn write_all(&self, _data: &[u8]) -> Result<(), String> {
                Ok(())
            }
        }

        let session = CmuxSession::new("mock".into(), Arc::new(NoopIo));
        session.register_dlci(2, "mock#dlci=2".into());
        let cancel = Arc::new(AtomicBool::new(false));
        let options = ExchangeOptions {
            timeout_ms: Some(1000),
            rx_prepare: Some(RxPrepareMode::None),
            completion_mode: Some(ExchangeCompletionMode::AtFinalLine),
            result_format: Some(AtResultFormat::Verbose),
            ..Default::default()
        };

        let waiter_session = session.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(20));
            let payload = b"AT\r\r\nOK\r\n";
            let frame = encode_uih(2, payload);
            waiter_session.feed_physical_rx(&frame);
        });

        let response = run_mux_exchange(&session, 2, "AT", &[], b"AT\r".to_vec(), options, cancel)
            .expect("mux exchange");
        assert_eq!(response.status, AtParseStatus::Ok);
    }

    #[cfg(mobile)]
    mod mobile_hub {
        use super::*;
        use crate::hub::handle::RxHubHandle;
        use crate::hub::mobile::MobileRxHub;
        use crate::hub::HubRoutingState;
        use std::time::Instant;

        struct MockExchangeIo {
            fail_write: bool,
        }

        impl ExchangeIo for MockExchangeIo {
            fn purge_rx(&self) -> Result<(), Error> {
                Ok(())
            }
            fn write_payload(&self, _payload: &[u8]) -> Result<(), Error> {
                if self.fail_write {
                    Err(Error::String("disconnect".into()))
                } else {
                    Ok(())
                }
            }
        }

        fn mobile_hub(path: &str) -> Arc<MobileRxHub> {
            Arc::new(MobileRxHub::new(path.into()))
        }

        #[test]
        fn mobile_take_idle_bytes_replayed_after_write() {
            let hub = mobile_hub("dev-idle");
            let path = "dev-idle".to_string();
            hub.shared()
                .feed_bytes(b"\r\nOK\r\n", &mut HubRoutingState::new(path));

            let cancel = Arc::new(AtomicBool::new(false));
            let options = ExchangeOptions {
                timeout_ms: Some(1000),
                rx_prepare: Some(RxPrepareMode::None),
                ..Default::default()
            };
            let io = MockExchangeIo { fail_write: false };

            let response = run_physical_exchange(
                hub.as_ref(),
                &io,
                "AT",
                &[],
                b"AT\r".to_vec(),
                options,
                cancel,
            )
            .expect("exchange completes from idle buffer");
            assert_eq!(response.status, AtParseStatus::Ok);
            let text = String::from_utf8_lossy(&response.raw);
            assert!(text.contains("OK"), "expected OK in {:?}", text);
        }

        #[test]
        fn mobile_exchange_fails_fast_on_disconnect() {
            let hub = mobile_hub("dev-disc");
            let cancel = Arc::new(AtomicBool::new(false));
            let options = ExchangeOptions {
                timeout_ms: Some(5000),
                rx_prepare: Some(RxPrepareMode::None),
                ..Default::default()
            };
            let io = MockExchangeIo { fail_write: true };
            let start = Instant::now();
            let err = run_physical_exchange(
                hub.as_ref(),
                &io,
                "AT",
                &[],
                b"AT\r".to_vec(),
                options,
                cancel,
            )
            .unwrap_err();
            assert!(start.elapsed() < Duration::from_millis(200));
            assert!(
                err.to_string().contains("disconnect"),
                "unexpected err: {err}"
            );
        }
    }
}
