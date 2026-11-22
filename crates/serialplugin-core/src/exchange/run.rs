//! Run exchange on a physical port or CMUX virtual channel.

use crate::at::parse::ExchangeResponse;
use crate::cmux::CmuxSession;
use crate::error::Error;
use crate::events::{ExchangeOptions, RxPrepareMode};
use crate::exchange::io::ExchangeIo;
use crate::exchange::options::ReadUntilOutcome;
use crate::hub::handle::RxHubHandle;
use crate::hub::shared::PORT_CLOSED;
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
    let cancel = exchange_cancel.clone();
    let hub_shared = hub.shared();

    match resolved.rx_prepare {
        RxPrepareMode::Purge => {
            hub_shared.purge_buffers();
            io.purge_rx()?;
        }
        RxPrepareMode::Drain => {
            // The drain also drops idle bytes (a late reply to a timed-out exchange),
            // atomically with installing its slot.
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
    // Nothing is sent on a port that is being closed (the waiter has failed).
    if hub_shared.is_closing() {
        hub.clear_exchange_waiter();
        return Err(Error::String(PORT_CLOSED.into()));
    }

    if let Err(e) = io.write_payload(&payload) {
        hub.clear_exchange_waiter();
        return Err(e);
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
    let cancel = exchange_cancel.clone();
    let waiter = ExchangeWaiter::new(resolved.clone(), cancel);
    session.set_exchange_waiter(dlci, waiter.clone());
    if let Err(e) = session.send_uih(dlci, &payload) {
        session.clear_exchange_waiter(dlci);
        return Err(Error::String(e));
    }
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
