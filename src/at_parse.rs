//! AT response line parsing and solicited vs URC classification.

use crate::events::AtResultFormat;
use serde::{Deserialize, Serialize};

/// Parsed outcome of an AT exchange response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ParsedAtResponse {
    pub status: AtParseStatus,
    pub lines: Vec<String>,
    pub solicited_body: Vec<String>,
    pub urc_lines: Vec<String>,
    pub final_line: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AtParseStatus {
    Ok,
    Error,
    Cme,
    Cms,
    Unknown,
}

/// How the read loop decided the exchange finished.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ExchangeMatch {
    Ok,
    Error,
    CmeError { code: Option<i32> },
    CmsError { code: Option<i32> },
    NoCarrier,
    Busy,
    NoAnswer,
    NoDialtone,
    SendOk,
    SendFail,
    Intermediate { line: String },
    Substring { term: String },
    Idle,
}

impl ExchangeMatch {
    pub fn to_parse_status(&self) -> AtParseStatus {
        match self {
            ExchangeMatch::Ok | ExchangeMatch::SendOk => AtParseStatus::Ok,
            ExchangeMatch::Error
            | ExchangeMatch::NoCarrier
            | ExchangeMatch::Busy
            | ExchangeMatch::NoAnswer
            | ExchangeMatch::NoDialtone
            | ExchangeMatch::SendFail => AtParseStatus::Error,
            ExchangeMatch::CmeError { .. } => AtParseStatus::Cme,
            ExchangeMatch::CmsError { .. } => AtParseStatus::Cms,
            ExchangeMatch::Intermediate { .. }
            | ExchangeMatch::Substring { .. }
            | ExchangeMatch::Idle => AtParseStatus::Unknown,
        }
    }
}

/// Structured exchange result returned to the frontend.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExchangeResponse {
    pub raw: Vec<u8>,
    pub matched: ExchangeMatch,
    pub lines: Vec<String>,
    pub status: AtParseStatus,
    pub solicited_body: Vec<String>,
    pub urc_lines: Vec<String>,
}

/// AT command result returned by native `at` / `at_phases` / `send_sms_pdu`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AtCommandResult {
    pub command: String,
    pub response: String,
    pub raw: Vec<u8>,
    pub matched: ExchangeMatch,
    pub lines: Vec<String>,
    pub status: AtParseStatus,
    pub solicited_body: Vec<String>,
    pub urc_lines: Vec<String>,
    pub timed_out: bool,
}

impl AtCommandResult {
    pub fn from_exchange(command: String, exchange: ExchangeResponse) -> Self {
        let response = String::from_utf8_lossy(&exchange.raw).into_owned();
        Self {
            command,
            response,
            raw: exchange.raw,
            matched: exchange.matched,
            lines: exchange.lines,
            status: exchange.status,
            solicited_body: exchange.solicited_body,
            urc_lines: exchange.urc_lines,
            timed_out: false,
        }
    }
}

impl ExchangeResponse {
    pub fn from_raw(
        raw: Vec<u8>,
        matched: ExchangeMatch,
        command: &str,
        solicited_prefixes: &[String],
        result_format: AtResultFormat,
    ) -> Self {
        let merged = merge_solicited_prefixes(command, solicited_prefixes);
        let parsed = parse_at_response(&raw, command, &merged, result_format);
        Self {
            status: parsed.status,
            lines: parsed.lines,
            solicited_body: parsed.solicited_body,
            urc_lines: parsed.urc_lines,
            matched,
            raw,
        }
    }
}

/// Merge user-provided prefixes with auto-derived ones from the command string.
pub fn merge_solicited_prefixes(command: &str, user: &[String]) -> Vec<String> {
    let mut out: Vec<String> = derive_solicited_prefixes(command);
    for p in user {
        let t = p.trim().to_string();
        if !t.is_empty() && !out.iter().any(|x| x == &t) {
            out.push(t);
        }
    }
    out
}

/// Derive expected solicited response header from an AT command (e.g. `AT+CSQ` → `+CSQ:`).
pub fn derive_solicited_prefixes(command: &str) -> Vec<String> {
    let cmd = normalize_command(command);
    let upper = cmd.to_ascii_uppercase();
    if !upper.starts_with("AT") {
        return Vec::new();
    }
    let rest = &upper[2..];
    let rest = rest.trim_start_matches(['+', '^', '#', '$', '%', '*', '&']);
    if rest.is_empty() {
        return Vec::new();
    }
    let name: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect();
    if name.is_empty() {
        return Vec::new();
    }
    let prefix_char = cmd.chars().nth(2).filter(|c| "+^#$%*&".contains(*c));
    let header = match prefix_char {
        Some(ch) => format!("{}{}:", ch, name),
        None => format!("{}:", name),
    };
    vec![header]
}

/// Split buffer into non-empty trimmed lines (handles `\r\n` and `\n`).
pub fn split_lines(text: &str) -> Vec<String> {
    text.replace('\r', "")
        .split('\n')
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Vendor extended information line (`+CSQ:`, `^SYSCFG:`, `#SGACT:`, …).
pub fn is_ext_info_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with('+')
        || t.starts_with('^')
        || t.starts_with('#')
        || t.starts_with('$')
        || t.starts_with('%')
        || t.starts_with('*')
}

/// True when the line is a strict V.250 numeric code (`0`…`99`).
pub fn is_numeric_result_line(line: &str) -> bool {
    let t = line.trim();
    !t.is_empty() && t.len() <= 2 && t.chars().all(|c| c.is_ascii_digit())
}

/// Intermediate result codes that must not complete an exchange (V.250 verbose).
pub fn classify_intermediate_line(line: &str) -> bool {
    let t = line.trim();
    t == "CONNECT" || t == ">" || t.starts_with("CONNECT ")
}

/// Intermediate lines including numeric `1` (CONNECT) when `ATV0` is active.
pub fn classify_intermediate_line_with_format(line: &str, format: AtResultFormat) -> bool {
    classify_intermediate_line(line) || (format == AtResultFormat::Numeric && line.trim() == "1")
}

/// V.250 numeric final result codes (`ATV0`). Numeric `1`/`2` are intermediate/URC, not finals.
pub fn classify_numeric_final_line(line: &str) -> Option<ExchangeMatch> {
    if !is_numeric_result_line(line) {
        return None;
    }
    match line.trim() {
        "0" => Some(ExchangeMatch::Ok),
        "3" => Some(ExchangeMatch::NoCarrier),
        "4" => Some(ExchangeMatch::Error),
        "5" => Some(ExchangeMatch::NoDialtone),
        "6" => Some(ExchangeMatch::Busy),
        "7" => Some(ExchangeMatch::NoAnswer),
        _ => None,
    }
}

fn classify_final_line_with_format(line: &str, format: AtResultFormat) -> Option<ExchangeMatch> {
    classify_final_line(line).or_else(|| {
        if format == AtResultFormat::Numeric {
            classify_numeric_final_line(line)
        } else {
            None
        }
    })
}

/// Returns `Some(match)` when the buffer ends with a complete AT final line.
pub fn at_final_line_complete(buf: &[u8], result_format: AtResultFormat) -> Option<ExchangeMatch> {
    if buf.is_empty() || !buf.ends_with(b"\n") {
        return None;
    }
    let text = std::str::from_utf8(buf).ok()?;
    let lines = split_lines(text);
    let last = lines.last()?;
    classify_final_line_with_format(last, result_format)
}

/// Returns `Some(Intermediate)` when the buffer ends with a complete intermediate line.
pub fn at_intermediate_line_complete(
    buf: &[u8],
    result_format: AtResultFormat,
) -> Option<ExchangeMatch> {
    if buf.is_empty() || !buf.ends_with(b"\n") {
        return None;
    }
    let text = std::str::from_utf8(buf).ok()?;
    let lines = split_lines(text);
    let last = lines.last()?;
    if classify_intermediate_line_with_format(last, result_format) {
        Some(ExchangeMatch::Intermediate { line: last.clone() })
    } else {
        None
    }
}

pub fn classify_final_line(line: &str) -> Option<ExchangeMatch> {
    let trimmed = line.trim();
    match trimmed {
        "OK" => Some(ExchangeMatch::Ok),
        "ERROR" => Some(ExchangeMatch::Error),
        "NO CARRIER" => Some(ExchangeMatch::NoCarrier),
        "BUSY" => Some(ExchangeMatch::Busy),
        "NO ANSWER" => Some(ExchangeMatch::NoAnswer),
        "NO DIALTONE" => Some(ExchangeMatch::NoDialtone),
        "SEND OK" => Some(ExchangeMatch::SendOk),
        "SEND FAIL" => Some(ExchangeMatch::SendFail),
        s if s.starts_with("+CME ERROR:") => Some(ExchangeMatch::CmeError {
            code: parse_trailing_code(s),
        }),
        s if s.starts_with("+CMS ERROR:") => Some(ExchangeMatch::CmsError {
            code: parse_trailing_code(s),
        }),
        _ => None,
    }
}

fn parse_trailing_code(s: &str) -> Option<i32> {
    s.rsplit(':').next()?.trim().parse().ok()
}

/// Classify lines into solicited body vs URC using echo / solicited prefixes.
pub fn parse_at_response(
    raw: &[u8],
    command: &str,
    solicited_prefixes: &[String],
    result_format: AtResultFormat,
) -> ParsedAtResponse {
    let merged = merge_solicited_prefixes(command, solicited_prefixes);
    let text = String::from_utf8_lossy(raw);
    let lines = split_lines(&text);
    let cmd_norm = normalize_command(command);
    let final_line = lines.last().cloned();
    let status = final_line
        .as_deref()
        .and_then(|l| classify_final_line_with_format(l, result_format))
        .map(|m| m.to_parse_status())
        .unwrap_or(AtParseStatus::Unknown);

    let body_end = lines.len().saturating_sub(1);
    let mut echo_idx: Option<usize> = None;
    for (i, line) in lines.iter().enumerate().take(body_end) {
        if lines_equivalent(line, &cmd_norm) {
            echo_idx = Some(i);
            break;
        }
    }

    let mut urc_lines = Vec::new();
    let mut solicited_body = Vec::new();

    for (i, line) in lines.iter().enumerate().take(body_end) {
        if Some(i) == echo_idx {
            continue;
        }
        if echo_idx.is_none() && i == 0 && lines_equivalent(line, &cmd_norm) {
            continue;
        }
        if echo_idx.is_none() && is_likely_urc(line, &merged) {
            urc_lines.push(line.clone());
        } else if echo_idx.map(|e| i > e).unwrap_or(true) {
            solicited_body.push(line.clone());
        } else if is_likely_urc(line, &merged) {
            urc_lines.push(line.clone());
        } else {
            solicited_body.push(line.clone());
        }
    }

    ParsedAtResponse {
        status,
        lines,
        solicited_body,
        urc_lines,
        final_line,
    }
}

fn normalize_command(command: &str) -> String {
    command
        .trim()
        .trim_end_matches('\r')
        .trim_end_matches('\n')
        .to_string()
}

fn lines_equivalent(line: &str, command: &str) -> bool {
    line.trim().eq_ignore_ascii_case(command)
}

pub fn is_likely_urc(line: &str, solicited_prefixes: &[String]) -> bool {
    let trimmed = line.trim();
    if !is_ext_info_line(trimmed) {
        return matches!(trimmed, "RING" | "NO CARRIER" | "BUSY" | "NO ANSWER");
    }
    !solicited_prefixes
        .iter()
        .any(|p| trimmed.starts_with(p.trim()))
}

/// Exchange demux phases (live routing during an in-flight exchange).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangePhase {
    AwaitingFirstLine,
    CollectingSolicited,
}

/// Routes bytes during an exchange: URC before echo → live emit; after echo → solicited blob.
#[derive(Debug)]
pub struct ExchangeDemux {
    phase: ExchangePhase,
    command: String,
    solicited_prefixes: Vec<String>,
    partial: String,
    emitted_urc: Vec<String>,
}

impl ExchangeDemux {
    pub fn new(command: &str, solicited_prefixes: &[String]) -> Self {
        Self {
            phase: ExchangePhase::AwaitingFirstLine,
            command: normalize_command(command),
            solicited_prefixes: merge_solicited_prefixes(command, solicited_prefixes),
            partial: String::new(),
            emitted_urc: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.phase = ExchangePhase::AwaitingFirstLine;
        self.partial.clear();
        self.emitted_urc.clear();
    }

    /// Process complete lines from a chunk; returns new URC lines to emit live.
    pub fn process_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        self.partial.push_str(&String::from_utf8_lossy(chunk));
        let mut live_urc = Vec::new();
        while let Some(pos) = self.partial.find('\n') {
            let line = self.partial[..pos]
                .trim()
                .trim_end_matches('\r')
                .to_string();
            self.partial.drain(..=pos);
            if line.is_empty() {
                continue;
            }
            if classify_final_line_with_format(&line, AtResultFormat::Verbose).is_some()
                || classify_intermediate_line(&line)
            {
                if lines_equivalent(&line, &self.command) {
                    self.phase = ExchangePhase::CollectingSolicited;
                }
                continue;
            }
            if lines_equivalent(&line, &self.command) {
                self.phase = ExchangePhase::CollectingSolicited;
                continue;
            }
            match self.phase {
                ExchangePhase::AwaitingFirstLine => {
                    if is_likely_urc(&line, &self.solicited_prefixes)
                        && !self.emitted_urc.contains(&line)
                    {
                        self.emitted_urc.push(line.clone());
                        live_urc.push(line);
                    }
                }
                ExchangePhase::CollectingSolicited => {}
            }
        }
        live_urc
    }
}

/// Extract URC lines from drained bytes (for drainRx forwarding).
pub fn urc_lines_from_drain(bytes: &[u8], solicited_prefixes: &[String]) -> Vec<String> {
    split_lines(&String::from_utf8_lossy(bytes))
        .into_iter()
        .filter(|l| is_likely_urc(l, solicited_prefixes))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn final_line_ok_only_at_end() {
        let buf = b"AT\r\r\nOK\r\n";
        assert!(matches!(
            at_final_line_complete(buf, AtResultFormat::Verbose),
            Some(ExchangeMatch::Ok)
        ));
        assert!(at_final_line_complete(b"AT\r\r\nO", AtResultFormat::Verbose).is_none());
    }

    #[test]
    fn numeric_atv0_ok() {
        let buf = b"AT\r\r\n0\r\n";
        assert!(at_final_line_complete(buf, AtResultFormat::Verbose).is_none());
        assert!(matches!(
            at_final_line_complete(buf, AtResultFormat::Numeric),
            Some(ExchangeMatch::Ok)
        ));
    }

    #[test]
    fn numeric_no_carrier_is_final() {
        assert!(matches!(
            at_final_line_complete(b"ATD\r\r\n3\r\n", AtResultFormat::Numeric),
            Some(ExchangeMatch::NoCarrier)
        ));
    }

    #[test]
    fn numeric_one_is_intermediate_not_final() {
        assert!(at_final_line_complete(b"ATD\r\r\n1\r\n", AtResultFormat::Numeric).is_none());
        assert!(matches!(
            at_intermediate_line_complete(b"ATD\r\r\n1\r\n", AtResultFormat::Numeric),
            Some(ExchangeMatch::Intermediate { .. })
        ));
    }

    #[test]
    fn prompt_is_intermediate_complete() {
        assert!(matches!(
            at_intermediate_line_complete(b"AT+CMGS=1\r\r\n>\r\n", AtResultFormat::Verbose),
            Some(ExchangeMatch::Intermediate { line }) if line == ">"
        ));
    }

    #[test]
    fn connect_is_not_final() {
        assert!(
            at_final_line_complete(b"ATD123\r\r\nCONNECT\r\n", AtResultFormat::Verbose).is_none()
        );
        assert!(classify_intermediate_line("CONNECT 9600"));
    }

    #[test]
    fn no_carrier_is_final() {
        assert!(matches!(
            at_final_line_complete(b"ATD\r\r\nNO CARRIER\r\n", AtResultFormat::Verbose),
            Some(ExchangeMatch::NoCarrier)
        ));
    }

    #[test]
    fn derive_prefix_csq() {
        assert_eq!(
            derive_solicited_prefixes("AT+CSQ"),
            vec!["+CSQ:".to_string()]
        );
    }

    #[test]
    fn derive_prefix_syscfg() {
        assert_eq!(
            derive_solicited_prefixes("AT^SYSCFG?"),
            vec!["^SYSCFG:".to_string()]
        );
    }

    #[test]
    fn csq_is_solicited_not_urc() {
        let raw = b"AT+CSQ\r\r\n+CSQ: 20,99\r\nOK\r\n";
        let parsed = parse_at_response(raw, "AT+CSQ", &[], AtResultFormat::Verbose);
        assert_eq!(parsed.status, AtParseStatus::Ok);
        assert!(parsed.solicited_body.iter().any(|l| l.starts_with("+CSQ:")));
        assert!(parsed.urc_lines.is_empty());
    }

    #[test]
    fn syscfg_solicited_with_derived_prefix() {
        let raw = b"AT^SYSCFG?\r\r\n^SYSCFG: 2,2,3FFFFFFF,1,1\r\nOK\r\n";
        let parsed = parse_at_response(raw, "AT^SYSCFG?", &[], AtResultFormat::Verbose);
        assert!(parsed
            .solicited_body
            .iter()
            .any(|l| l.starts_with("^SYSCFG:")));
        assert!(parsed.urc_lines.is_empty());
    }

    #[test]
    fn creg_in_blob_classified_as_urc_before_echo() {
        let raw = b"+CREG: 1,2\r\nAT\r\r\nOK\r\n";
        let parsed = parse_at_response(raw, "AT", &[], AtResultFormat::Verbose);
        assert!(parsed.urc_lines.iter().any(|l| l.starts_with("+CREG:")));
    }

    #[test]
    fn cme_error_text_status() {
        let raw = b"AT+FOO\r\r\n+CME ERROR: SIM not inserted\r\n";
        let parsed = parse_at_response(raw, "AT+FOO", &[], AtResultFormat::Verbose);
        assert_eq!(parsed.status, AtParseStatus::Cme);
    }

    #[test]
    fn demux_emits_urc_before_echo() {
        let mut demux = ExchangeDemux::new("AT", &[]);
        let urc = demux.process_chunk(b"+CREG: 1\r\nAT\r\n");
        assert!(urc.iter().any(|l| l.starts_with("+CREG")));
    }
}
