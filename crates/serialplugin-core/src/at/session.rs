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

/// Terminate `command` with CR unless the caller already did (or opted out).
pub fn normalize_at_command(command: &str, append_cr: bool) -> String {
    if !append_cr || command.ends_with(['\r', '\n']) {
        command.to_string()
    } else {
        format!("{}\r", command.trim())
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
        // A terminator the caller already supplied must survive.
        assert_eq!(normalize_at_command("AT\r", true), "AT\r");
        assert_eq!(normalize_at_command("AT\r\n", true), "AT\r\n");
        assert_eq!(normalize_at_command("AT\r", false), "AT\r");
    }

    fn json(o: &ExchangeOptions) -> serde_json::Value {
        serde_json::to_value(o).unwrap()
    }

    #[test]
    fn session_flags_default_and_follow_the_config() {
        let d = AtSessionConfig::default();
        assert!(d.stop_on_error());
        assert!(!d.expect_ok());
        assert!(d.append_cr());
        let s = AtSessionConfig {
            stop_on_error: Some(false),
            expect_ok: Some(true),
            append_cr: Some(false),
            ..Default::default()
        };
        assert!(!s.stop_on_error());
        assert!(s.expect_ok());
        assert!(!s.append_cr());
    }

    fn session() -> AtSessionConfig {
        AtSessionConfig {
            default_timeout_ms: Some(700),
            default_terminators: Some(vec!["READY".into()]),
            default_idle_ms: Some(30),
            default_rx_prepare: Some(RxPrepareMode::Purge),
            default_solicited_prefixes: Some(vec!["+X:".into()]),
            result_format: Some(AtResultFormat::Numeric),
            ..Default::default()
        }
    }

    #[test]
    fn merge_exchange_takes_session_defaults_then_per_command_values() {
        let merged = session().merge_exchange("AT+X", None);
        assert_eq!(
            json(&merged),
            json(&ExchangeOptions {
                timeout_ms: Some(700),
                terminators: Some(vec!["READY".into()]),
                idle_ms: Some(30),
                rx_prepare: Some(RxPrepareMode::Purge),
                completion_mode: Some(ExchangeCompletionMode::AtFinalLine),
                result_format: Some(AtResultFormat::Numeric),
                command: Some("AT+X".into()),
                solicited_prefixes: Some(vec!["+X:".into()]),
                ..Default::default()
            })
        );
        let per = AtCommandOptions {
            timeout_ms: Some(9),
            terminators: Some(vec!["T".into()]),
            idle_ms: Some(8),
            rx_prepare: Some(RxPrepareMode::None),
            completion_mode: Some(ExchangeCompletionMode::Substring),
            result_format: Some(AtResultFormat::Verbose),
            solicited_prefixes: Some(vec!["+Y:".into()]),
            append_cr: None,
        };
        let merged = session().merge_exchange("AT+Y", Some(&per));
        assert_eq!(
            json(&merged),
            json(&ExchangeOptions {
                timeout_ms: Some(9),
                terminators: Some(vec!["T".into()]),
                idle_ms: Some(8),
                rx_prepare: Some(RxPrepareMode::None),
                completion_mode: Some(ExchangeCompletionMode::Substring),
                result_format: Some(AtResultFormat::Verbose),
                command: Some("AT+Y".into()),
                solicited_prefixes: Some(vec!["+Y:".into()]),
                ..Default::default()
            })
        );
    }

    #[test]
    fn merge_phase_takes_the_phase_then_session_defaults() {
        let phase = AtPhase {
            write: AtPhaseWrite::Text("AT+P".into()),
            completion_mode: Some(ExchangeCompletionMode::AtIntermediate),
            result_format: None,
            timeout_ms: Some(11),
            command: None,
            rx_prepare: None,
        };
        assert_eq!(
            json(&session().merge_phase(&phase, "AT+P")),
            json(&ExchangeOptions {
                timeout_ms: Some(11),
                terminators: Some(vec!["READY".into()]),
                idle_ms: Some(30),
                rx_prepare: Some(RxPrepareMode::Purge),
                completion_mode: Some(ExchangeCompletionMode::AtIntermediate),
                result_format: Some(AtResultFormat::Numeric),
                command: Some("AT+P".into()),
                solicited_prefixes: Some(vec!["+X:".into()]),
                ..Default::default()
            })
        );
    }

    #[test]
    fn expect_ok_rejects_other_statuses_only_when_set() {
        use crate::at::parse::AtParseStatus;
        let strict = AtSessionConfig {
            expect_ok: Some(true),
            ..Default::default()
        };
        assert!(check_expect_ok(&strict, AtParseStatus::Ok, "OK").is_ok());
        assert!(check_expect_ok(&strict, AtParseStatus::Error, "ERROR").is_err());
        let lax = AtSessionConfig::default();
        assert!(check_expect_ok(&lax, AtParseStatus::Error, "ERROR").is_ok());
    }
}
