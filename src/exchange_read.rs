//! Write + read-until helpers for AT-style request/response exchange.

use crate::at_parse::ExchangeMatch;
#[cfg(desktop)]
use crate::at_parse::{at_final_line_complete, at_intermediate_line_complete};
use crate::events::{AtResultFormat, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode};
#[cfg(desktop)]
use crate::error::Error;
#[cfg(desktop)]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(desktop)]
use std::time::{Duration, Instant};

const DEFAULT_EXCHANGE_TIMEOUT_MS: u64 = 5000;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 4096;
const DEFAULT_DRAIN_IDLE_MS: u64 = 50;
const DEFAULT_DRAIN_MAX_MS: u64 = 200;
#[cfg(desktop)]
const POLL_READ_TIMEOUT_MS: u64 = 100;

/// Resolved exchange parameters with defaults applied.
#[derive(Debug, Clone)]
pub struct ResolvedExchangeOptions {
    pub timeout_ms: u64,
    pub max_bytes: usize,
    pub terminators: Vec<Vec<u8>>,
    pub idle_ms: Option<u64>,
    pub rx_prepare: RxPrepareMode,
    pub drain_idle_ms: u64,
    pub drain_max_ms: u64,
    pub completion_mode: ExchangeCompletionMode,
    pub result_format: AtResultFormat,
    pub command: Option<String>,
    pub solicited_prefixes: Vec<String>,
}

impl ExchangeOptions {
    pub fn resolve(&self) -> ResolvedExchangeOptions {
        let rx_prepare = self.effective_rx_prepare();
        let command = self.command.clone();
        let solicited_prefixes = self.solicited_prefixes.clone().unwrap_or_default();
        let merged = command
            .as_deref()
            .map(|c| crate::at_parse::merge_solicited_prefixes(c, &solicited_prefixes))
            .unwrap_or(solicited_prefixes);
        ResolvedExchangeOptions {
            timeout_ms: self.timeout_ms.unwrap_or(DEFAULT_EXCHANGE_TIMEOUT_MS),
            max_bytes: self.max_bytes.unwrap_or(DEFAULT_MAX_RESPONSE_BYTES),
            terminators: self
                .terminators
                .as_ref()
                .map(|list| list.iter().map(|s| s.as_bytes().to_vec()).collect())
                .unwrap_or_else(default_terminators),
            idle_ms: self.idle_ms,
            rx_prepare,
            drain_idle_ms: self.drain_idle_ms.unwrap_or(DEFAULT_DRAIN_IDLE_MS),
            drain_max_ms: self.drain_max_ms.unwrap_or(DEFAULT_DRAIN_MAX_MS),
            completion_mode: self.completion_mode.clone().unwrap_or_default(),
            result_format: self.result_format.unwrap_or_default(),
            command,
            solicited_prefixes: merged,
        }
    }

    fn effective_rx_prepare(&self) -> RxPrepareMode {
        if let Some(mode) = &self.rx_prepare {
            return mode.clone();
        }
        RxPrepareMode::Drain
    }
}

pub fn default_terminators() -> Vec<Vec<u8>> {
    vec![
        b"\r\nOK\r\n".to_vec(),
        b"\r\nERROR\r\n".to_vec(),
        b"\nOK\n".to_vec(),
        b"\nERROR\n".to_vec(),
    ]
}

pub fn matches_terminators(buf: &[u8], terminators: &[Vec<u8>]) -> bool {
    terminators
        .iter()
        .any(|term| !term.is_empty() && buf.windows(term.len()).any(|w| w == term.as_slice()))
}

pub struct ReadUntilOutcome {
    pub raw: Vec<u8>,
    pub matched: ExchangeMatch,
}

#[cfg(desktop)]
fn substring_match(buf: &[u8], terminators: &[Vec<u8>]) -> Option<ExchangeMatch> {
    for term in terminators {
        if term.is_empty() {
            continue;
        }
        if buf.windows(term.len()).any(|w| w == term.as_slice()) {
            let term_str = String::from_utf8_lossy(term).into_owned();
            return Some(ExchangeMatch::Substring { term: term_str });
        }
    }
    None
}

#[cfg(desktop)]
fn check_complete(buf: &[u8], options: &ResolvedExchangeOptions) -> Option<ExchangeMatch> {
    match options.completion_mode {
        ExchangeCompletionMode::AtFinalLine => at_final_line_complete(buf, options.result_format),
        ExchangeCompletionMode::AtIntermediate => {
            at_intermediate_line_complete(buf, options.result_format)
        }
        ExchangeCompletionMode::Substring => substring_match(buf, &options.terminators),
    }
}

/// Soft-drain RX until idle or max time (returns drained bytes).
#[cfg(desktop)]
pub fn drain_rx_port(
    port: &mut dyn serialport::SerialPort,
    idle_ms: u64,
    max_ms: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, Error> {
    port.set_timeout(Duration::from_millis(POLL_READ_TIMEOUT_MS))
        .map_err(|e| Error::String(format!("Failed to set drain poll timeout: {}", e)))?;

    let deadline = Instant::now() + Duration::from_millis(max_ms);
    let mut drained = Vec::new();
    let mut last_byte_at: Option<Instant> = None;
    let mut chunk = vec![0u8; 1024];

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::String("exchange cancelled".to_string()));
        }
        if Instant::now() >= deadline {
            break;
        }
        match port.read(&mut chunk) {
            Ok(0) => {
                if let Some(last) = last_byte_at {
                    if last.elapsed() >= Duration::from_millis(idle_ms) {
                        break;
                    }
                } else {
                    break;
                }
            }
            Ok(n) => {
                drained.extend_from_slice(&chunk[..n]);
                last_byte_at = Some(Instant::now());
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                if let Some(last) = last_byte_at {
                    if last.elapsed() >= Duration::from_millis(idle_ms) {
                        break;
                    }
                } else {
                    break;
                }
            }
            Err(e) => {
                return Err(Error::String(format!("Failed to drain RX: {}", e)));
            }
        }
    }
    Ok(drained)
}

/// Read from `port` until completion, idle silence, timeout, cancel, or max bytes.
#[cfg(desktop)]
pub fn read_until_port(
    port: &mut dyn serialport::SerialPort,
    options: &ResolvedExchangeOptions,
    cancel: &AtomicBool,
) -> Result<ReadUntilOutcome, Error> {
    port.set_timeout(Duration::from_millis(POLL_READ_TIMEOUT_MS))
        .map_err(|e| Error::String(format!("Failed to set exchange poll timeout: {}", e)))?;

    let deadline = Instant::now() + Duration::from_millis(options.timeout_ms);
    let mut response = Vec::new();
    let mut last_byte_at: Option<Instant> = None;
    let mut chunk = vec![0u8; 1024];

    loop {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::String("exchange cancelled".to_string()));
        }
        if Instant::now() >= deadline {
            let partial = String::from_utf8_lossy(&response);
            return Err(Error::String(format!(
                "exchange timed out after {} ms (partial: {:?})",
                options.timeout_ms, partial
            )));
        }
        if response.len() >= options.max_bytes {
            return Err(Error::String(format!(
                "exchange response exceeded {} bytes",
                options.max_bytes
            )));
        }

        match port.read(&mut chunk) {
            Ok(0) => {}
            Ok(n) => {
                response.extend_from_slice(&chunk[..n]);
                last_byte_at = Some(Instant::now());
                if let Some(matched) = check_complete(&response, options) {
                    return Ok(ReadUntilOutcome {
                        raw: response,
                        matched,
                    });
                }
                if response.len() >= options.max_bytes {
                    return Err(Error::String(format!(
                        "exchange response exceeded {} bytes",
                        options.max_bytes
                    )));
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                if let (Some(idle_ms), Some(last)) = (options.idle_ms, last_byte_at) {
                    if !response.is_empty() && last.elapsed() >= Duration::from_millis(idle_ms) {
                        return Ok(ReadUntilOutcome {
                            raw: response,
                            matched: ExchangeMatch::Idle,
                        });
                    }
                }
            }
            Err(e) => {
                return Err(Error::String(format!(
                    "Failed to read during exchange: {}",
                    e
                )));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, ExchangeCompletionMode};

    #[test]
    fn matches_ok_terminator_substring_mode() {
        let buf = b"AT\r\r\nOK\r\n";
        let opts = ResolvedExchangeOptions {
            timeout_ms: 5000,
            max_bytes: 4096,
            terminators: default_terminators(),
            idle_ms: None,
            rx_prepare: RxPrepareMode::Drain,
            drain_idle_ms: 50,
            drain_max_ms: 200,
            completion_mode: ExchangeCompletionMode::Substring,
            result_format: AtResultFormat::Verbose,
            command: Some("AT".into()),
            solicited_prefixes: vec![],
        };
        assert!(check_complete(buf, &opts).is_some());
    }

    #[test]
    fn at_final_line_does_not_match_embedded_ok() {
        let buf = b"data \r\nOK\r\n still\r\nOK\r\n";
        assert!(matches!(
            check_complete(
                buf,
                &ResolvedExchangeOptions {
                    timeout_ms: 5000,
                    max_bytes: 4096,
                    terminators: vec![],
                    idle_ms: None,
                    rx_prepare: RxPrepareMode::Drain,
                    drain_idle_ms: 50,
                    drain_max_ms: 200,
                    completion_mode: ExchangeCompletionMode::AtFinalLine,
                    result_format: AtResultFormat::Verbose,
                    command: None,
                    solicited_prefixes: vec![],
                }
            ),
            Some(ExchangeMatch::Ok)
        ));
    }

    #[test]
    fn resolve_defaults_to_drain_and_at_final_line() {
        let opts = ExchangeOptions::default().resolve();
        assert_eq!(opts.rx_prepare, RxPrepareMode::Drain);
        assert_eq!(opts.completion_mode, ExchangeCompletionMode::AtFinalLine);
    }
}
