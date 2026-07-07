//! Resolved exchange parameters and read-until completion helpers.

use crate::at::parse::ExchangeMatch;
use crate::events::{AtResultFormat, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode};

const DEFAULT_EXCHANGE_TIMEOUT_MS: u64 = 5000;
const DEFAULT_MAX_RESPONSE_BYTES: usize = 4096;
const DEFAULT_DRAIN_IDLE_MS: u64 = 50;
const DEFAULT_DRAIN_MAX_MS: u64 = 200;

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
            .map(|c| crate::at::parse::merge_solicited_prefixes(c, &solicited_prefixes))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{AtResultFormat, ExchangeCompletionMode};
    use crate::exchange::completion::check_exchange_complete;

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
        assert!(check_exchange_complete(buf, &opts).is_some());
    }

    #[test]
    fn at_final_line_does_not_match_embedded_ok() {
        let buf = b"data \r\nOK\r\n still\r\nOK\r\n";
        assert!(matches!(
            check_exchange_complete(
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
