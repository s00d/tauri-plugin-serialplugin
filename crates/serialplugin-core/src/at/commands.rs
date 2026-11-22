//! Shared AT command runtime (desktop + mobile).

use crate::at::session::{
    check_expect_ok, normalize_at_command, AtCommandOptions, AtPhase, AtPhaseWrite,
    AtSessionConfig, SendSmsPduOptions,
};
use crate::error::Error;
use crate::events::{ExchangeCompletionMode, ExchangeOptions, RxPrepareMode};
use crate::port::tx_queue::PortTxQueue;

/// Platform exchange entry point used by `at` / `at_phases` (physical or CMUX virtual).
pub trait ExchangeRunner {
    fn run_exchange_unqueued(
        &self,
        path: String,
        payload: Vec<u8>,
        options: ExchangeOptions,
    ) -> Result<crate::at::parse::ExchangeResponse, Error>;
}

fn phase_gap_hook(_phase_index: usize) {
    #[cfg(target_os = "android")]
    if _phase_index > 0 {
        // CH340 resets if the next command lands while a large RX is still draining.
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// Run one AT command with session defaults (caller holds TX queue turn).
pub fn run_at<R: ExchangeRunner>(
    runner: &R,
    session: &AtSessionConfig,
    path: String,
    command: String,
    options: Option<AtCommandOptions>,
) -> Result<crate::at::parse::AtCommandResult, Error> {
    let append_cr = options
        .as_ref()
        .and_then(|o| o.append_cr)
        .unwrap_or_else(|| session.append_cr());
    let payload = normalize_at_command(&command, append_cr);
    let exchange_opts = session.merge_exchange(&command, options.as_ref());
    let response = runner.run_exchange_unqueued(path, payload.into_bytes(), exchange_opts)?;
    check_expect_ok(
        session,
        response.status,
        &String::from_utf8_lossy(&response.raw),
    )?;
    Ok(crate::at::parse::AtCommandResult::from_exchange(
        command, response,
    ))
}

/// Multi-phase AT flow (caller holds TX queue turn).
pub fn run_at_phases<R: ExchangeRunner>(
    runner: &R,
    session: &AtSessionConfig,
    path: String,
    phases: Vec<AtPhase>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    let stop_on_error = session.stop_on_error();
    let mut results = Vec::with_capacity(phases.len());
    // A payload phase (PDU + Ctrl-Z after `AT+CMGS=n` / `AT+CMGW=n`) has no AT command
    // of its own; its reply (`+CMGS: <mr>`) answers the command that opened the prompt.
    let mut command_prefixes: Vec<String> = Vec::new();
    for (i, phase) in phases.iter().enumerate() {
        phase_gap_hook(i);
        let label = phase.command.clone().unwrap_or_else(|| match &phase.write {
            AtPhaseWrite::Text(s) => s.clone(),
            AtPhaseWrite::Binary(b) => format!("<binary {} bytes>", b.len()),
        });
        let rx_prepare = if i == 0 {
            None
        } else {
            Some(RxPrepareMode::None)
        };
        let mut exchange_opts = session.merge_phase(phase, &label);
        let own_prefixes = crate::at::parse::derive_solicited_prefixes(&label);
        if own_prefixes.is_empty() {
            let prefixes = exchange_opts
                .solicited_prefixes
                .get_or_insert_with(Vec::new);
            for p in &command_prefixes {
                if !prefixes.contains(p) {
                    prefixes.push(p.clone());
                }
            }
        } else {
            command_prefixes = own_prefixes;
        }
        if let Some(rp) = rx_prepare {
            exchange_opts.rx_prepare = Some(rp);
        }
        let payload = match &phase.write {
            AtPhaseWrite::Text(s) => normalize_at_command(s, session.append_cr()).into_bytes(),
            AtPhaseWrite::Binary(b) => b.clone(),
        };
        // `timed_out` comes from the exchange error only. `check_expect_ok`'s error quotes
        // the modem's reply (`+CME ERROR: network timeout` is an answer, not a timeout).
        // The runner returns an untyped `Error`; its exchange messages carry no device text.
        let phase_result = runner
            .run_exchange_unqueued(path.clone(), payload, exchange_opts)
            .map_err(|e| {
                let timed_out = crate::error::ErrorKind::from_message(&e.to_string())
                    == crate::error::ErrorKind::Timeout;
                (e, timed_out)
            })
            .and_then(|response| {
                check_expect_ok(
                    session,
                    response.status,
                    &String::from_utf8_lossy(&response.raw),
                )
                .map_err(|e| (e, false))?;
                Ok(crate::at::parse::AtCommandResult::from_exchange(
                    label.clone(),
                    response,
                ))
            });
        match phase_result {
            Ok(r) => results.push(r),
            Err((e, timed_out)) => {
                if stop_on_error {
                    return Err(e);
                }
                let mut failed = crate::at::parse::AtCommandResult::failed(label, e.to_string());
                failed.timed_out = timed_out;
                results.push(failed);
            }
        }
    }
    Ok(results)
}

/// Built-in CMGS phase list.
pub fn cmgs_phases(
    session: &AtSessionConfig,
    length: u32,
    pdu: Vec<u8>,
    options: Option<&SendSmsPduOptions>,
) -> Vec<AtPhase> {
    let timeout_ms = options
        .and_then(|o| o.timeout_ms)
        .or(session.default_timeout_ms);
    let result_format = options
        .and_then(|o| o.result_format)
        .or(session.result_format);
    let cmd = format!("AT+CMGS={length}");
    let mut payload = pdu;
    payload.push(0x1a);
    vec![
        AtPhase {
            write: AtPhaseWrite::Text(cmd.clone()),
            completion_mode: Some(ExchangeCompletionMode::AtIntermediate),
            result_format,
            timeout_ms,
            command: Some(cmd),
            rx_prepare: None,
        },
        AtPhase {
            write: AtPhaseWrite::Binary(payload),
            completion_mode: Some(ExchangeCompletionMode::AtFinalLine),
            result_format,
            timeout_ms,
            command: Some(String::new()),
            rx_prepare: Some(RxPrepareMode::None),
        },
    ]
}

/// Run CMGS recipe through `run_at_phases` (caller holds TX queue turn).
pub fn run_send_sms_pdu<R: ExchangeRunner>(
    runner: &R,
    session: &AtSessionConfig,
    path: String,
    length: u32,
    pdu: Vec<u8>,
    options: Option<SendSmsPduOptions>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    let phases = cmgs_phases(session, length, pdu, options.as_ref());
    run_at_phases(runner, session, path, phases)
}

/// Helper: run AT inside the port TX queue.
pub fn queue_at<R: ExchangeRunner>(
    runner: &R,
    tx_queue: &PortTxQueue,
    path: String,
    command: String,
    options: Option<AtCommandOptions>,
) -> Result<crate::at::parse::AtCommandResult, Error> {
    tx_queue.run_serial(|| {
        let session = tx_queue.at_session();
        run_at(runner, &session, path, command, options)
    })
}

pub fn queue_at_phases<R: ExchangeRunner>(
    runner: &R,
    tx_queue: &PortTxQueue,
    path: String,
    phases: Vec<AtPhase>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    tx_queue.run_serial(|| {
        let session = tx_queue.at_session();
        run_at_phases(runner, &session, path, phases)
    })
}

pub fn queue_send_sms_pdu<R: ExchangeRunner>(
    runner: &R,
    tx_queue: &PortTxQueue,
    path: String,
    length: u32,
    pdu: Vec<u8>,
    options: Option<SendSmsPduOptions>,
) -> Result<Vec<crate::at::parse::AtCommandResult>, Error> {
    tx_queue.run_serial(|| {
        let session = tx_queue.at_session();
        run_send_sms_pdu(runner, &session, path, length, pdu, options)
    })
}
