//! AT session defaults and per-command option merging (native queue).

use crate::events::{AtResultFormat, ExchangeCompletionMode, ExchangeOptions, RxPrepareMode};
use serde::{Deserialize, Serialize};

/// Session-level defaults for native `at` jobs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtSessionConfig {
    pub default_timeout_ms: Option<u64>,
    pub default_terminators: Option<Vec<String>>,
    pub default_idle_ms: Option<u64>,
    pub default_rx_prepare: Option<RxPrepareMode>,
    pub default_solicited_prefixes: Option<Vec<String>>,
    pub stop_on_error: Option<bool>,
    pub expect_ok: Option<bool>,
    pub append_cr: Option<bool>,
    pub result_format: Option<AtResultFormat>,
}

/// Per-command overrides for native `at`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtCommandOptions {
    pub timeout_ms: Option<u64>,
    pub terminators: Option<Vec<String>>,
    pub idle_ms: Option<u64>,
    pub rx_prepare: Option<RxPrepareMode>,
    pub completion_mode: Option<ExchangeCompletionMode>,
    pub result_format: Option<AtResultFormat>,
    pub solicited_prefixes: Option<Vec<String>>,
    pub append_cr: Option<bool>,
}

/// One step in a multi-phase AT flow (e.g. CMGS).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtPhase {
    /// UTF-8 command or raw bytes (array of u8).
    pub write: AtPhaseWrite,
    pub completion_mode: Option<ExchangeCompletionMode>,
    pub result_format: Option<AtResultFormat>,
    pub timeout_ms: Option<u64>,
    pub command: Option<String>,
    pub rx_prepare: Option<RxPrepareMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AtPhaseWrite {
    Text(String),
    Binary(Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendSmsPduOptions {
    pub timeout_ms: Option<u64>,
    pub result_format: Option<AtResultFormat>,
}

impl AtSessionConfig {
    pub fn stop_on_error(&self) -> bool {
        self.stop_on_error.unwrap_or(true)
    }

    pub fn expect_ok(&self) -> bool {
        self.expect_ok.unwrap_or(false)
    }

    pub fn append_cr(&self) -> bool {
        self.append_cr.unwrap_or(true)
    }

    pub fn merge_exchange(&self, command: &str, per: Option<&AtCommandOptions>) -> ExchangeOptions {
        let per = per.cloned().unwrap_or_default();
        ExchangeOptions {
            timeout_ms: per.timeout_ms.or(self.default_timeout_ms),
            terminators: per.terminators.or_else(|| self.default_terminators.clone()),
            idle_ms: per.idle_ms.or(self.default_idle_ms),
            rx_prepare: per
                .rx_prepare
                .or_else(|| self.default_rx_prepare.clone())
                .or(Some(RxPrepareMode::Drain)),
            completion_mode: per
                .completion_mode
                .clone()
                .or(Some(ExchangeCompletionMode::AtFinalLine)),
            result_format: per.result_format.or(self.result_format),
            command: Some(command.to_string()),
            solicited_prefixes: per
                .solicited_prefixes
                .or_else(|| self.default_solicited_prefixes.clone()),
            ..Default::default()
        }
    }

    pub fn merge_phase(&self, phase: &AtPhase, command: &str) -> ExchangeOptions {
        ExchangeOptions {
            timeout_ms: phase.timeout_ms.or(self.default_timeout_ms),
            terminators: self.default_terminators.clone(),
            idle_ms: self.default_idle_ms,
            rx_prepare: phase
                .rx_prepare
                .clone()
                .or_else(|| self.default_rx_prepare.clone())
                .or(Some(RxPrepareMode::Drain)),
            completion_mode: phase
                .completion_mode
                .clone()
                .or(Some(ExchangeCompletionMode::AtFinalLine)),
            result_format: phase.result_format.or(self.result_format),
            command: Some(command.to_string()),
            solicited_prefixes: self.default_solicited_prefixes.clone(),
            ..Default::default()
        }
    }
}

pub fn normalize_at_command(command: &str, append_cr: bool) -> String {
    let trimmed = command.trim();
    if !append_cr || command.contains('\r') || command.contains('\n') {
        trimmed.to_string()
    } else {
        format!("{trimmed}\r")
    }
}

pub fn check_expect_ok(
    session: &AtSessionConfig,
    status: crate::at::parse::AtParseStatus,
    response_preview: &str,
) -> Result<(), crate::error::Error> {
    if session.expect_ok() && status != crate::at::parse::AtParseStatus::Ok {
        return Err(crate::error::Error::String(format!(
            "AT command failed with status {:?}: {}",
            status, response_preview
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_cr_adds_cr() {
        assert_eq!(normalize_at_command("AT", true), "AT\r");
        assert_eq!(normalize_at_command("AT\r\n", true), "AT");
    }
}
