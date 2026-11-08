//! AT response line parsing and solicited vs URC classification.

use serde::{Deserialize, Serialize};

/// AT result line format (V.250 verbose vs numeric `ATV0`).
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AtResultFormat {
    #[default]
    Verbose,
    Numeric,
}

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
    CmeError {
        code: Option<i32>,
        /// Error text (`AT+CMEE=2`), e.g. `SIM not inserted`. Absent for numeric codes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    CmsError {
        code: Option<i32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    NoCarrier,
    Busy,
    NoAnswer,
    NoDialtone,
    SendOk,
    SendFail,
    Intermediate {
        line: String,
    },
    Substring {
        term: String,
    },
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

    pub fn failed(command: String, reason: String) -> Self {
        Self {
            command,
            response: reason.clone(),
            raw: reason.into_bytes(),
            matched: ExchangeMatch::Error,
            lines: Vec::new(),
            status: AtParseStatus::Error,
            solicited_body: Vec::new(),
            urc_lines: Vec::new(),
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

/// Longest quoted `+CUSD` text kept open across embedded CR/LF (UCS2 hex of 182 chars + slack).
const MAX_CUSD_SPAN: usize = 2_048;

/// Split buffer into non-empty trimmed lines (handles `\r\n` and `\n`).
///
/// Two modem quirks are undone here, so every caller sees the same lines:
/// - a final result glued to the next URC without CRLF (`OK+CUSD: …`) becomes two lines;
/// - the quoted text of a `+CUSD:` may carry raw `\r`/`\n` (USSD menus): the line stays
///   open until the quote closes. A result code, another `+XXX:` line or 2 KiB end an
///   unclosed quote early (truncated `+CUSD`). Inside a `+CUSD` line CR/LF are kept.
pub fn split_lines(text: &str) -> Vec<String> {
    let segments: Vec<&str> = text.split('\n').collect();
    group_segments(&segments, false).0
}

/// Group LF-separated segments into logical lines. With `wait_for_quote`, an open
/// `+CUSD` quote at the end stops the scan; the second value is how many segments
/// were consumed (the rest must be fed again with more bytes).
fn group_segments(segments: &[&str], wait_for_quote: bool) -> (Vec<String>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < segments.len() {
        let emitted = out.len();
        let mut rest = segments[i].trim_start();
        if let Some(head) = glued_final_head(rest.trim_end()) {
            out.push(head.to_string());
            rest = &rest[head.len()..];
        }
        let line = rest.trim_end();
        if line.starts_with("+CUSD:") {
            if has_open_quote(line) {
                match close_cusd_quote(segments, i, line.len()) {
                    QuoteEnd::Closed(j) => {
                        let mut joined = rest.to_string();
                        for seg in &segments[i + 1..=j] {
                            joined.push('\n');
                            joined.push_str(seg);
                        }
                        out.push(joined.trim_end().to_string());
                        i = j + 1;
                        continue;
                    }
                    QuoteEnd::Open if wait_for_quote => {
                        out.truncate(emitted);
                        return (out, i);
                    }
                    QuoteEnd::Open | QuoteEnd::Cut => {}
                }
            }
            out.push(line.to_string());
        } else {
            let line = line.replace('\r', "");
            let line = line.trim();
            if !line.is_empty() {
                out.push(line.to_string());
            }
        }
        i += 1;
    }
    (out, segments.len())
}

enum QuoteEnd {
    /// Quote closes in segment `j`.
    Closed(usize),
    /// Ended early by a modem line / size cap: keep the first segment as a truncated line.
    Cut,
    /// No closing quote yet.
    Open,
}

fn close_cusd_quote(segments: &[&str], first: usize, first_len: usize) -> QuoteEnd {
    let mut span = first_len;
    for (j, seg) in segments.iter().enumerate().skip(first + 1) {
        span += seg.len() + 1;
        let seg = seg.trim();
        if span > MAX_CUSD_SPAN || classify_final_line(seg).is_some() || is_urc_shape(seg) {
            return QuoteEnd::Cut;
        }
        if has_open_quote(seg) {
            return QuoteEnd::Closed(j);
        }
    }
    QuoteEnd::Open
}

fn has_open_quote(s: &str) -> bool {
    s.matches('"').count() % 2 == 1
}

/// `OK+CUSD: …` → `Some("OK")`: a verbose final result glued to a URC line.
fn glued_final_head(line: &str) -> Option<&str> {
    let idx = line.find('+').filter(|&i| i > 0)?;
    let (head, rest) = line.split_at(idx);
    (classify_final_line(head).is_some() && is_urc_shape(rest)).then_some(head)
}

/// Strict shape of an unsolicited line: `RING`, or `+NAME:` / `^NAME:` / `#NAME:` … with
/// an upper-case name. SMS text such as `*100#`, `+7 900 …` or `#tag` does not match.
fn is_urc_shape(line: &str) -> bool {
    let t = line.trim();
    if t == "RING" {
        return true;
    }
    let Some(body) = t.strip_prefix(['+', '^', '#', '$', '%', '*']) else {
        return false;
    };
    body.split_once(':').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_uppercase())
            && name
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
    })
}

/// Header whose next line is data (27.005): `Some(true)` when more text lines may
/// follow (text-mode `+CMGR`/`+CMGL` bodies), `Some(false)` for exactly one data line
/// (`+CMT`, `+CBM`, PDU-mode `+CDS: <length>`).
fn two_line_header(line: &str) -> Option<bool> {
    if line.starts_with("+CMGR:") || line.starts_with("+CMGL:") {
        return Some(true);
    }
    if line.starts_with("+CMT:") || line.starts_with("+CBM:") {
        return Some(false);
    }
    let len = line.strip_prefix("+CDS:")?.trim();
    (!len.is_empty() && len.chars().all(|c| c.is_ascii_digit())).then_some(false)
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

/// V.250 numeric final result codes (`ATV0`): `0` OK, `3` NO CARRIER, `4` ERROR,
/// `6` NO DIALTONE, `7` BUSY, `8` NO ANSWER. `1` (CONNECT) is intermediate, `2` (RING)
/// unsolicited, `5` is unassigned.
///
/// In numeric mode a one-digit SMS body (`0`, `4`, …) in a `+CMGR`/`+CMGL` reply is
/// indistinguishable from a result code and ends the exchange early; read SMS in
/// verbose mode (`ATV1`).
pub fn classify_numeric_final_line(line: &str) -> Option<ExchangeMatch> {
    if !is_numeric_result_line(line) {
        return None;
    }
    match line.trim() {
        "0" => Some(ExchangeMatch::Ok),
        "3" => Some(ExchangeMatch::NoCarrier),
        "4" => Some(ExchangeMatch::Error),
        "6" => Some(ExchangeMatch::NoDialtone),
        "7" => Some(ExchangeMatch::Busy),
        "8" => Some(ExchangeMatch::NoAnswer),
        _ => None,
    }
}

fn classify_final_line_with_format(
    line: &str,
    format: AtResultFormat,
    ignore_case: bool,
) -> Option<ExchangeMatch> {
    classify_final_line_cased(line, ignore_case).or_else(|| {
        if format == AtResultFormat::Numeric {
            classify_numeric_final_line(line)
        } else {
            None
        }
    })
}

/// Returns `Some(match)` when the buffer holds a complete AT final line.
///
/// The last final result after the echo wins; whatever follows it in the same read
/// (`RING`, a two-line `+CMT` with its PDU, `RDY` / `Call Ready`, the rest of a USSD
/// menu) is trailing unsolicited output and does not hide it. Bytes are decoded
/// lossily so line noise or a Latin-1 phonebook entry cannot stall the exchange.
pub fn at_final_line_complete(buf: &[u8], result_format: AtResultFormat) -> Option<ExchangeMatch> {
    // Judge only complete lines: bytes after the last LF may already belong to what
    // comes next (e.g. CMUX flags right after `AT+CMUX=0` → `OK`, in the same read).
    let end = buf.iter().rposition(|&b| b == b'\n')?;
    let lines = split_lines(&String::from_utf8_lossy(&buf[..=end]));
    let mut classifier = LineClassifier::new("", &[], result_format, true);
    let mut last = None;
    for line in &lines {
        match classifier.feed(line) {
            LineKind::Final(m) => last = Some(m),
            // Without the command, an echo is any `AT…` line: it starts a new reply,
            // so an earlier final (late reply to a previous command) is stale.
            _ if line.starts_with("AT") => last = None,
            _ => {}
        }
    }
    last
}

/// Completion for prompt-style commands (`AT+CMGS=…` → `> `, `ATD` → `CONNECT`).
///
/// 27.005 sends the SMS prompt as `\r\n> ` with no line feed after it, so the last
/// unterminated line counts too. A final result (`+CMS ERROR: …`, `ERROR`) also
/// completes, otherwise a rejected command would wait out the whole timeout.
pub fn at_intermediate_line_complete(
    buf: &[u8],
    result_format: AtResultFormat,
) -> Option<ExchangeMatch> {
    if buf.is_empty() {
        return None;
    }
    let text = String::from_utf8_lossy(buf);
    let tail = text.rsplit('\n').next().unwrap_or_default().trim();
    if tail == ">" {
        return Some(ExchangeMatch::Intermediate { line: ">".into() });
    }
    if !buf.ends_with(b"\n") {
        return None;
    }
    let lines = split_lines(&text);
    let last = lines.last()?;
    if classify_intermediate_line_with_format(last, result_format) {
        Some(ExchangeMatch::Intermediate { line: last.clone() })
    } else {
        at_final_line_complete(buf, result_format)
    }
}

/// V.250 / 27.007 / 27.005 final result line, case-insensitive (`OK`, `ok`, …).
pub fn classify_final_line(line: &str) -> Option<ExchangeMatch> {
    classify_final_line_cased(line, true)
}

fn classify_final_line_cased(line: &str, ignore_case: bool) -> Option<ExchangeMatch> {
    let trimmed = line.trim();
    let upper;
    let key = if ignore_case {
        upper = trimmed.to_ascii_uppercase();
        upper.as_str()
    } else {
        trimmed
    };
    // ASCII upper-casing keeps byte offsets, so `trimmed[11..]` is the original detail.
    match key {
        "OK" => Some(ExchangeMatch::Ok),
        "ERROR" => Some(ExchangeMatch::Error),
        "NO CARRIER" => Some(ExchangeMatch::NoCarrier),
        "BUSY" => Some(ExchangeMatch::Busy),
        "NO ANSWER" => Some(ExchangeMatch::NoAnswer),
        "NO DIALTONE" => Some(ExchangeMatch::NoDialtone),
        "SEND OK" => Some(ExchangeMatch::SendOk),
        "SEND FAIL" => Some(ExchangeMatch::SendFail),
        s if s.starts_with("+CME ERROR:") => {
            let (code, message) = parse_error_detail(&trimmed[11..], CME_TEXT_CODES);
            Some(ExchangeMatch::CmeError { code, message })
        }
        s if s.starts_with("+CMS ERROR:") => {
            let (code, message) = parse_error_detail(&trimmed[11..], CMS_TEXT_CODES);
            Some(ExchangeMatch::CmsError { code, message })
        }
        _ => None,
    }
}

/// Verbose `+CME ERROR:` texts (`AT+CMEE=2`) → numeric codes (27.007 §9.2).
const CME_TEXT_CODES: &[(&str, i32)] = &[
    ("phone failure", 0),
    ("no connection to phone", 1),
    ("phone-adaptor link reserved", 2),
    ("operation not allowed", 3),
    ("operation not supported", 4),
    ("PH-SIM PIN required", 5),
    ("PH-FSIM PIN required", 6),
    ("PH-FSIM PUK required", 7),
    ("SIM not inserted", 10),
    ("SIM PIN required", 11),
    ("SIM PUK required", 12),
    ("SIM failure", 13),
    ("SIM busy", 14),
    ("SIM wrong", 15),
    ("incorrect password", 16),
    ("SIM PIN2 required", 17),
    ("SIM PUK2 required", 18),
    ("memory full", 20),
    ("invalid index", 21),
    ("not found", 22),
    ("memory failure", 23),
    ("text string too long", 24),
    ("invalid characters in text string", 25),
    ("dial string too long", 26),
    ("invalid characters in dial string", 27),
    ("no network service", 30),
    ("network timeout", 31),
    ("network not allowed - emergency calls only", 32),
    ("unknown", 100),
];

/// Verbose `+CMS ERROR:` texts → numeric codes (27.005 §3.2.5).
const CMS_TEXT_CODES: &[(&str, i32)] = &[
    ("ME failure", 300),
    ("SMS service of ME reserved", 301),
    ("operation not allowed", 302),
    ("operation not supported", 303),
    ("invalid PDU mode parameter", 304),
    ("invalid text mode parameter", 305),
    ("SIM not inserted", 310),
    ("SIM PIN required", 311),
    ("PH-SIM PIN required", 312),
    ("SIM failure", 313),
    ("SIM busy", 314),
    ("SIM wrong", 315),
    ("SIM PUK required", 316),
    ("SIM PIN2 required", 317),
    ("SIM PUK2 required", 318),
    ("memory failure", 320),
    ("invalid memory index", 321),
    ("memory full", 322),
    ("SMSC address unknown", 330),
    ("no network service", 331),
    ("network timeout", 332),
    ("no +CNMA acknowledgement expected", 340),
    ("unknown error", 500),
];

/// `14` → (14, None); `10: SIM not inserted` → (10, text); `SIM busy` → (table, text).
fn parse_error_detail(detail: &str, table: &[(&str, i32)]) -> (Option<i32>, Option<String>) {
    let detail = detail.trim();
    if let Ok(code) = detail.parse() {
        return (Some(code), None);
    }
    if let Some((code, text)) = detail.split_once(':') {
        if let Ok(code) = code.trim().parse() {
            let text = text.trim();
            return (Some(code), (!text.is_empty()).then(|| text.to_string()));
        }
    }
    let text = detail.trim_matches('"').trim();
    let code = table
        .iter()
        .find(|(message, _)| message.eq_ignore_ascii_case(text))
        .map(|(_, code)| *code);
    (code, (!text.is_empty()).then(|| text.to_string()))
}

/// What a reply line is, judged in order (see [`LineClassifier::feed`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum LineKind {
    Echo,
    Body,
    Urc,
    Final(ExchangeMatch),
}

/// Line-by-line solicited / URC classification shared by the one-shot parser, the
/// completion check and the live demux, so they never disagree about a line.
#[derive(Debug)]
struct LineClassifier {
    command: String,
    prefixes: Vec<String>,
    format: AtResultFormat,
    /// Parser: lines after the echo are the reply. Live demux: still URC-checked.
    body_after_echo: bool,
    echo_seen: bool,
    /// Kind of the next line: the data line of a two-line header (`+CMT`, `+CMGR`, …),
    /// plus whether more text-body lines may follow it.
    pending: Option<(LineKind, bool)>,
    /// Inside a text-mode `+CMGR`/`+CMGL` body: only strict URC shapes are URCs.
    in_text_body: bool,
}

impl LineClassifier {
    fn new(
        command: &str,
        prefixes: &[String],
        format: AtResultFormat,
        body_after_echo: bool,
    ) -> Self {
        Self {
            command: normalize_command(command),
            prefixes: prefixes.to_vec(),
            format,
            body_after_echo,
            echo_seen: false,
            pending: None,
            in_text_body: false,
        }
    }

    fn feed(&mut self, line: &str) -> LineKind {
        // SMS text is often lower-case (`ok`): inside a body only exact codes end it.
        let in_data = self.pending.is_some() || self.in_text_body;
        if let Some(m) = classify_final_line_with_format(line, self.format, !in_data) {
            self.pending = None;
            self.in_text_body = false;
            return LineKind::Final(m);
        }
        if let Some((kind, text_body)) = self.pending.take() {
            self.in_text_body = text_body;
            return kind;
        }
        if !self.echo_seen && !self.command.is_empty() && lines_equivalent(line, &self.command) {
            self.echo_seen = true;
            return LineKind::Echo;
        }
        let kind = if self.in_text_body {
            if is_urc_shape(line) && is_likely_urc(line, &self.prefixes) {
                LineKind::Urc
            } else {
                LineKind::Body
            }
        } else if self.echo_seen && self.body_after_echo {
            LineKind::Body
        } else if is_likely_urc(line, &self.prefixes) {
            LineKind::Urc
        } else {
            LineKind::Body
        };
        if let Some(text_body) = two_line_header(line) {
            self.pending = Some((kind.clone(), text_body));
        }
        kind
    }
}

/// Classify lines into solicited body vs URC using echo / solicited prefixes.
///
/// The data line after `+CMGR:`/`+CMGL:`/`+CMT:`/`+CBM:`/`+CDS: <len>` belongs to its
/// header (body or URC, kept adjacent). Lines after the final result are URCs.
pub fn parse_at_response(
    raw: &[u8],
    command: &str,
    solicited_prefixes: &[String],
    result_format: AtResultFormat,
) -> ParsedAtResponse {
    let merged = merge_solicited_prefixes(command, solicited_prefixes);
    // Same view as completion: complete lines only, and the final result code may be
    // followed by URC lines (or CMUX flags) that arrived in the same read.
    let complete = match raw.iter().rposition(|&b| b == b'\n') {
        Some(end) => &raw[..=end],
        None => raw,
    };
    let lines = split_lines(&String::from_utf8_lossy(complete));
    let mut classifier = LineClassifier::new(command, &merged, result_format, true);
    let kinds: Vec<LineKind> = lines.iter().map(|l| classifier.feed(l)).collect();
    let echo_idx = kinds.iter().position(|k| *k == LineKind::Echo);
    let final_idx = kinds
        .iter()
        .rposition(|k| matches!(k, LineKind::Final(_)))
        .filter(|&i| echo_idx.is_none_or(|e| i > e));
    let final_pos = final_idx.unwrap_or(lines.len().saturating_sub(1));
    let final_line = lines.get(final_pos).cloned();
    let status = match final_idx.map(|i| &kinds[i]) {
        Some(LineKind::Final(m)) => m.to_parse_status(),
        _ => AtParseStatus::Unknown,
    };

    let mut urc_lines = Vec::new();
    let mut solicited_body = Vec::new();
    for (line, kind) in lines.iter().zip(&kinds).take(final_pos) {
        match kind {
            LineKind::Echo => {}
            LineKind::Urc => urc_lines.push(line.clone()),
            LineKind::Body => solicited_body.push(line.clone()),
            // An earlier result-like line: `NO CARRIER` before the echo is a URC.
            LineKind::Final(_) if is_likely_urc(line, &merged) => urc_lines.push(line.clone()),
            LineKind::Final(_) => solicited_body.push(line.clone()),
        }
    }
    if let Some(i) = final_idx {
        urc_lines.extend(lines.iter().skip(i + 1).cloned());
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

/// Routes bytes during an exchange: URC lines are emitted live, the rest is the reply.
///
/// Each line is judged once, when it completes, so every URC occurrence (two `RING`s
/// in one exchange) is emitted exactly once.
#[derive(Debug)]
pub struct ExchangeDemux {
    phase: ExchangePhase,
    command: String,
    solicited_prefixes: Vec<String>,
    /// Bytes of an unfinished line (kept raw so a split UTF-8 char survives).
    partial: Vec<u8>,
    classifier: LineClassifier,
}

impl ExchangeDemux {
    pub fn new(command: &str, solicited_prefixes: &[String]) -> Self {
        let command = normalize_command(command);
        let solicited_prefixes = merge_solicited_prefixes(&command, solicited_prefixes);
        Self {
            phase: ExchangePhase::AwaitingFirstLine,
            classifier: Self::classifier(&command, &solicited_prefixes),
            command,
            solicited_prefixes,
            partial: Vec::new(),
        }
    }

    fn classifier(command: &str, prefixes: &[String]) -> LineClassifier {
        LineClassifier::new(command, prefixes, AtResultFormat::Verbose, false)
    }

    pub fn reset(&mut self) {
        self.phase = ExchangePhase::AwaitingFirstLine;
        self.partial.clear();
        self.classifier = Self::classifier(&self.command, &self.solicited_prefixes);
    }

    /// Process complete lines from a chunk; returns new URC lines to emit live.
    pub fn process_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        self.partial.extend_from_slice(chunk);
        let Some(end) = self.partial.iter().rposition(|&b| b == b'\n') else {
            return Vec::new();
        };
        let text = String::from_utf8_lossy(&self.partial[..end]).into_owned();
        let segments: Vec<&str> = text.split('\n').collect();
        let (lines, used) = group_segments(&segments, true);
        // LF is never part of a multi-byte sequence, so counting it in the raw bytes
        // maps consumed segments back to a byte offset.
        let cut = match used {
            0 => 0,
            n if n == segments.len() => end + 1,
            n => self
                .partial
                .iter()
                .enumerate()
                .filter(|(_, &b)| b == b'\n')
                .nth(n - 1)
                .map_or(0, |(i, _)| i + 1),
        };
        self.partial.drain(..cut);

        let mut live_urc = Vec::new();
        for line in lines {
            match self.classifier.feed(&line) {
                LineKind::Echo => self.phase = ExchangePhase::CollectingSolicited,
                LineKind::Urc => live_urc.push(line),
                LineKind::Body | LineKind::Final(_) => {}
            }
        }
        live_urc
    }
}

/// Extract URC lines from drained bytes (for drainRx forwarding). The data line of a
/// two-line URC (`+CMT: ,23` + PDU) comes right after its header.
pub fn urc_lines_from_drain(bytes: &[u8], solicited_prefixes: &[String]) -> Vec<String> {
    let mut classifier =
        LineClassifier::new("", solicited_prefixes, AtResultFormat::Verbose, false);
    split_lines(&String::from_utf8_lossy(bytes))
        .into_iter()
        .filter(|l| match classifier.feed(l) {
            LineKind::Urc => true,
            LineKind::Final(_) => is_likely_urc(l, solicited_prefixes),
            LineKind::Echo | LineKind::Body => false,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::AtResultFormat as F;

    #[test]
    fn urc_after_echo_is_still_emitted() {
        let mut demux = ExchangeDemux::new("AT+CSQ", &[]);
        let urc = demux.process_chunk(b"AT+CSQ\r\r\n+CMTI: \"SM\",3\r\n+CSQ: 20,0\r\n\r\nOK\r\n");
        assert_eq!(urc, vec!["+CMTI: \"SM\",3".to_string()]);
    }

    #[test]
    fn sms_prompt_without_trailing_lf_completes() {
        let m = at_intermediate_line_complete(b"AT+CMGS=20\r\r\n> ", F::Verbose);
        assert!(
            matches!(m, Some(ExchangeMatch::Intermediate { .. })),
            "{m:?}"
        );
    }

    #[test]
    fn prompt_command_rejected_with_cms_error_completes() {
        let m = at_intermediate_line_complete(b"AT+CMGS=20\r\r\n+CMS ERROR: 304\r\n", F::Verbose);
        assert!(
            matches!(
                m,
                Some(ExchangeMatch::CmsError {
                    code: Some(304),
                    ..
                })
            ),
            "{m:?}"
        );
    }

    /// Seen on a Quectel M26: `OK` and the first CMUX flags arrive in one read.
    #[test]
    fn trailing_bytes_after_final_line_do_not_hide_it() {
        let m = at_final_line_complete(b"AT+CMUX=0\r\r\nOK\r\n\xF9\xF9\xF9\xF9", F::Verbose);
        assert_eq!(m, Some(ExchangeMatch::Ok));
    }

    #[test]
    fn parsed_status_ignores_trailing_bytes_and_urcs() {
        let p = parse_at_response(
            b"AT+CMUX=0\r\r\nOK\r\n\xF9\xF9",
            "AT+CMUX=0",
            &[],
            F::Verbose,
        );
        assert_eq!(p.status, AtParseStatus::Ok);
        let p = parse_at_response(b"AT\r\r\nOK\r\n\r\nRING\r\n", "AT", &[], F::Verbose);
        assert_eq!(p.status, AtParseStatus::Ok);
        assert_eq!(p.urc_lines, vec!["RING".to_string()]);
    }

    #[test]
    fn non_utf8_noise_does_not_block_final_line() {
        let m = at_final_line_complete(b"\xFF\xFEAT\r\r\nOK\r\n", F::Verbose);
        assert_eq!(m, Some(ExchangeMatch::Ok));
    }

    #[test]
    fn urc_in_same_read_after_ok_does_not_hide_it() {
        let m = at_final_line_complete(b"AT\r\r\nOK\r\n\r\nRING\r\n", F::Verbose);
        assert_eq!(m, Some(ExchangeMatch::Ok));
        assert_eq!(at_final_line_complete(b"+CSQ: 20,0\r\n", F::Verbose), None);
    }

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
