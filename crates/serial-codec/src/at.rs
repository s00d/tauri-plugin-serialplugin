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
    /// `+CME ERROR:` (27.007 §9.2); a verbose text (`AT+CMEE=2`) is mapped to its code
    /// (`None` for text the parser does not know).
    CmeError {
        code: Option<i32>,
    },
    /// `+CMS ERROR:` (27.005 §3.2.5), mapped like [`ExchangeMatch::CmeError`].
    CmsError {
        code: Option<i32>,
    },
    NoCarrier,
    Busy,
    NoAnswer,
    NoDialtone,
    /// `SEND OK`: not V.250, the final of module TCP stacks (`AT+CIPSEND`).
    SendOk,
    /// `SEND FAIL`: not V.250, see [`ExchangeMatch::SendOk`].
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

/// Derive expected solicited response headers from an AT command (e.g. `AT+CSQ` →
/// `+CSQ:`); a composite command gets one per part (`AT+CSQ;+CREG?` → `+CSQ:`, `+CREG:`).
/// V.250 basic commands in front of an extended one are skipped (`ATE0V1+CSQ`,
/// `AT&F+CSQ` → `+CSQ:`).
pub fn derive_solicited_prefixes(command: &str) -> Vec<String> {
    let cmd = normalize_command(command);
    if !cmd.get(..2).is_some_and(|at| at.eq_ignore_ascii_case("AT")) {
        return Vec::new();
    }
    let mut out: Vec<String> = Vec::new();
    for header in cmd[2..].split(';').filter_map(part_header) {
        if !out.contains(&header) {
            out.push(header);
        }
    }
    out
}

/// Header of one `;`-separated part: its extended command after any leading V.250
/// basic commands (§5.2.1: `E1`, `V0`, `&F`, `&C1`, `S0=1`, `S3?`), else the part's
/// own name (`I` → `I:`). `D` takes the rest of the part as its dial string.
fn part_header(part: &str) -> Option<String> {
    let part = part.trim();
    let b = part.as_bytes();
    let digits = |mut i: usize| {
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        i
    };
    let mut i = 0;
    while let Some(&c) = b.get(i) {
        i = match c.to_ascii_uppercase() {
            b' ' => i + 1,
            b'&' if b.get(i + 1).is_some_and(u8::is_ascii_alphabetic) => digits(i + 2),
            b'D' => break,
            b'S' if b.get(i + 1).is_some_and(u8::is_ascii_digit) => {
                let j = digits(i + 1);
                match b.get(j) {
                    Some(b'=') => digits(j + 1),
                    Some(b'?') => j + 1,
                    _ => j,
                }
            }
            c if c.is_ascii_alphabetic() => digits(i + 1),
            _ => break,
        };
    }
    match b.get(i) {
        Some(c) if i > 0 && b"+^#$%*".contains(c) => command_header(&part[i..]),
        _ => command_header(part),
    }
}

/// `+CSQ` → `+CSQ:`, `^SYSCFG?` → `^SYSCFG:`, `I` → `I:`.
fn command_header(part: &str) -> Option<String> {
    let part = part.trim();
    let prefix_char = part.chars().next().filter(|c| "+^#$%*&".contains(*c));
    let name: String = part
        .trim_start_matches(['+', '^', '#', '$', '%', '*', '&'])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_uppercase();
    if name.is_empty() {
        return None;
    }
    Some(match prefix_char {
        Some(ch) => format!("{ch}{name}:"),
        None => format!("{name}:"),
    })
}

/// Longest quoted `+CUSD` text kept open across embedded CR/LF (UCS2 hex of 182 chars + slack).
const MAX_CUSD_SPAN: usize = 2_048;

/// Split buffer into non-empty trimmed lines (handles `\r\n`, `\n` and a lone `\r`:
/// V.250 §5.7.1 ends a numeric `ATV0` result code, and the echo, with `S3` only).
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
            for line in line.split('\r').map(str::trim).filter(|l| !l.is_empty()) {
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

/// End (inclusive) of the complete lines in `buf`: the last LF, or in numeric mode
/// also the last CR (V.250 §5.7.1: `ATV0` result codes end with `S3` alone). Bytes
/// after it may already belong to what comes next (e.g. CMUX flags right after
/// `AT+CMUX=0` → `OK`, in the same read).
fn complete_end(buf: &[u8], format: AtResultFormat) -> Option<usize> {
    buf.iter()
        .rposition(|&b| b == b'\n' || (b == b'\r' && format == AtResultFormat::Numeric))
}

/// Returns `Some(match)` when the buffer holds a complete AT final line.
///
/// The last final result after the echo wins; whatever follows it in the same read
/// (`RING`, a two-line `+CMT` with its PDU, `RDY` / `Call Ready`, the rest of a USSD
/// menu) is trailing unsolicited output and does not hide it. Bytes are decoded
/// lossily so line noise or a Latin-1 phonebook entry cannot stall the exchange.
pub fn at_final_line_complete(buf: &[u8], result_format: AtResultFormat) -> Option<ExchangeMatch> {
    scan_completion(buf, result_format, false)
}

/// Shared by both completion checks: one pass of an [`AtCompletion`] over `buf`.
fn scan_completion(
    buf: &[u8],
    format: AtResultFormat,
    intermediate: bool,
) -> Option<ExchangeMatch> {
    AtCompletion::new(intermediate, format).push(buf)
}

/// Incremental form of [`at_final_line_complete`] / [`at_intermediate_line_complete`]:
/// feed the reply chunk by chunk; each line is judged once, so a long reply costs
/// time linear in its size. After each [`push`](Self::push) the answer is the one the
/// one-shot check gives for all bytes pushed so far.
#[derive(Debug)]
pub struct AtCompletion {
    lines: LineAssembler,
    classifier: LineClassifier,
    format: AtResultFormat,
    intermediate: bool,
    last: Option<ExchangeMatch>,
}

impl AtCompletion {
    /// `intermediate`: also complete on `CONNECT` / `1` / the `> ` prompt.
    pub fn new(intermediate: bool, format: AtResultFormat) -> Self {
        Self {
            lines: LineAssembler::new(format),
            classifier: LineClassifier::new("", &[], format),
            format,
            intermediate,
            last: None,
        }
    }

    /// Feed the next bytes; returns the completion state for everything fed so far.
    pub fn push(&mut self, chunk: &[u8]) -> Option<ExchangeMatch> {
        for line in self.lines.push(chunk) {
            self.judge(&line);
        }
        // 27.005 sends the SMS prompt as `\r\n> ` with no line feed after it.
        if self.intermediate && String::from_utf8_lossy(self.lines.tail()).trim() == ">" {
            return Some(ExchangeMatch::Intermediate { line: ">".into() });
        }
        self.last.clone()
    }

    fn judge(&mut self, line: &str) {
        let in_data = self.classifier.in_data();
        match self.classifier.feed(line) {
            LineKind::Final(m) => self.last = Some(m),
            // The data line of a two-line URC (`+CMT` text) is never an echo.
            _ if in_data => {}
            _ if self.intermediate && classify_intermediate_line_with_format(line, self.format) => {
                self.last = Some(ExchangeMatch::Intermediate {
                    line: line.to_string(),
                })
            }
            // Without the command, an echo is any `AT…` line: it starts a new reply,
            // so an earlier final (late reply to a previous command) is stale.
            _ if line.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("AT")) => self.last = None,
            _ => {}
        }
    }
}

/// Completion for prompt-style commands (`AT+CMGS=…` → `> `, `ATD` → `CONNECT`).
///
/// 27.005 sends the SMS prompt as `\r\n> ` with no line feed after it, so the last
/// unterminated line counts too. A final result (`+CMS ERROR: …`, `ERROR`) also
/// completes, otherwise a rejected command would wait out the whole timeout. Data
/// after `CONNECT` in the same read (PPP frames, CMUX flags) does not hide it.
pub fn at_intermediate_line_complete(
    buf: &[u8],
    result_format: AtResultFormat,
) -> Option<ExchangeMatch> {
    scan_completion(buf, result_format, true)
}

/// Final result line, case-insensitive (`OK`, `ok`, …): the V.250 verbose codes,
/// `+CME ERROR:` (27.007) and `+CMS ERROR:` (27.005). `SEND OK` / `SEND FAIL` are
/// not V.250: they are the finals of the TCP/IP stacks of many modules
/// (`AT+CIPSEND`) and stay for compatibility.
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
            let (code, _) = parse_error_detail(&trimmed[11..], CME_TEXT_CODES);
            Some(ExchangeMatch::CmeError { code })
        }
        s if s.starts_with("+CMS ERROR:") => {
            let (code, _) = parse_error_detail(&trimmed[11..], CMS_TEXT_CODES);
            Some(ExchangeMatch::CmsError { code })
        }
        _ => None,
    }
}

/// Verbose `+CME ERROR:` texts (`AT+CMEE=2`) → numeric codes (27.007 §9.2). Texts are
/// compared loosely (see [`text_key`]); a code may have several spellings.
const CME_TEXT_CODES: &[(&str, i32)] = &[
    ("phone failure", 0),
    ("no connection to phone", 1),
    ("no connection", 1),
    ("phone-adaptor link reserved", 2),
    ("link reserved", 2),
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
    ("no network", 30),
    ("network timeout", 31),
    ("timeout", 31),
    ("network not allowed - emergency calls only", 32),
    ("network personalization PIN required", 40),
    ("network personalization PUK required", 41),
    ("network subset personalization PIN required", 42),
    ("network subset personalization PUK required", 43),
    ("service provider personalization PIN required", 44),
    ("service provider personalization PUK required", 45),
    ("corporate personalization PIN required", 46),
    ("corporate personalization PUK required", 47),
    ("hidden key required", 48),
    ("EAP method not supported", 49),
    ("incorrect parameters", 50),
    ("command implemented but currently disabled", 51),
    ("command aborted by user", 52),
    (
        "not attached to network due to MT functionality restrictions",
        53,
    ),
    (
        "modem not allowed - MT restricted to emergency calls only",
        54,
    ),
    (
        "operation not allowed because of MT functionality restrictions",
        55,
    ),
    (
        "fixed dial number only allowed - called number is not a fixed dial number",
        56,
    ),
    ("temporarily out of service due to other MT usage", 57),
    ("language/alphabet not supported", 58),
    ("language or alphabet not supported", 58),
    ("unexpected data value", 59),
    ("system failure", 60),
    ("data missing", 61),
    ("call barred", 62),
    ("message waiting indication subscription failure", 63),
    ("unknown", 100),
    // 27.007 §9.2.2 packet domain
    ("illegal MS", 103),
    ("illegal ME", 106),
    ("GPRS services not allowed", 107),
    ("PLMN not allowed", 111),
    ("location area not allowed", 112),
    ("roaming not allowed in this location area", 113),
    ("service option not supported", 132),
    ("requested service option not subscribed", 133),
    ("service option temporarily out of order", 134),
    ("unspecified GPRS error", 148),
    ("PDP authentication failure", 149),
    ("invalid mobile class", 150),
];

/// Verbose `+CMS ERROR:` texts → numeric codes (27.005 §3.2.5).
const CMS_TEXT_CODES: &[(&str, i32)] = &[
    ("ME failure", 300),
    ("SMS service of ME reserved", 301),
    ("SMS service reserved", 301),
    ("operation not allowed", 302),
    ("operation not supported", 303),
    ("invalid PDU mode parameter", 304),
    ("invalid PDU parameter", 304),
    ("invalid text mode parameter", 305),
    ("invalid text parameter", 305),
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
    ("invalid index", 321),
    ("memory full", 322),
    ("SMSC address unknown", 330),
    ("no network service", 331),
    ("no network", 331),
    ("network timeout", 332),
    ("no +CNMA acknowledgement expected", 340),
    ("unknown error", 500),
];

/// Comparison key for error texts: letters and digits only, lower case, British
/// `-isation` folded to `-ization`, so `SIM-PIN required`, `simpinrequired` and
/// `Network personalisation PIN required.` find their table row.
fn text_key(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
        .replace("isation", "ization")
}

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
    let key = text_key(text);
    let code = table
        .iter()
        .find(|(message, _)| text_key(message) == key)
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
    echo_seen: bool,
    /// Kind of the next line: the data line of a two-line header (`+CMT`, `+CMGR`, …),
    /// plus whether more text-body lines may follow it.
    pending: Option<(LineKind, bool)>,
    /// Inside a text-mode `+CMGR`/`+CMGL` body: only strict URC shapes are URCs.
    in_text_body: bool,
}

impl LineClassifier {
    fn new(command: &str, prefixes: &[String], format: AtResultFormat) -> Self {
        Self {
            command: normalize_command(command),
            prefixes: prefixes.to_vec(),
            format,
            echo_seen: false,
            pending: None,
            in_text_body: false,
        }
    }

    /// The next line is SMS data (a two-line header's payload or a text body).
    fn in_data(&self) -> bool {
        self.pending.is_some() || self.in_text_body
    }

    fn feed(&mut self, line: &str) -> LineKind {
        // SMS text is often lower-case (`ok`): inside a body only exact codes end it.
        let in_data = self.in_data();
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
        // Inside the reply (after the echo, or in an SMS text body), and whenever the
        // command is known (`ATE0` sends no echo), only a strict URC shape with a
        // foreign header is a URC: `+IFC (0,1,2)`, a garbled `+CSCSERROR` or the
        // `+CGMI` of an `AT+CLAC` list stay reply text, `+CMTI: "SM",3` does not.
        let strict = self.in_text_body || self.echo_seen || !self.command.is_empty();
        let kind = if strict {
            if is_urc_shape(line) && is_likely_urc(line, &self.prefixes) {
                LineKind::Urc
            } else {
                LineKind::Body
            }
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
    let complete = match complete_end(raw, result_format) {
        Some(end) => &raw[..=end],
        None => raw,
    };
    let lines = split_lines(&String::from_utf8_lossy(complete));
    let mut classifier = LineClassifier::new(command, &merged, result_format);
    let kinds: Vec<LineKind> = lines.iter().map(|l| classifier.feed(l)).collect();
    let echo_idx = kinds.iter().position(|k| *k == LineKind::Echo);
    let final_idx = kinds
        .iter()
        .rposition(|k| matches!(k, LineKind::Final(_)))
        .filter(|&i| echo_idx.is_none_or(|e| i > e));
    // Without a final result (substring / idle completion, a prompt, a timeout) every
    // line is body or URC.
    let final_pos = final_idx.unwrap_or(lines.len());
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

/// Exchange demux phase, see [`ExchangeDemux::phase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExchangePhase {
    /// The echo of the command has not been seen (yet, or ever with `ATE0`).
    AwaitingFirstLine,
    /// The echo was seen: the reply is being collected.
    CollectingSolicited,
}

/// Longest unterminated line [`ExchangeDemux`] keeps; longer runs without a line
/// feed (data after `CONNECT`, binary noise) cannot be a URC and are dropped.
pub const MAX_DEMUX_LINE: usize = 65_536;

/// Routes bytes during an exchange: URC lines are emitted live, the rest is the reply.
///
/// Each line is judged once, when it completes, so every URC occurrence (two `RING`s
/// in one exchange) is emitted exactly once.
#[derive(Debug)]
pub struct ExchangeDemux {
    phase: ExchangePhase,
    command: String,
    solicited_prefixes: Vec<String>,
    format: AtResultFormat,
    lines: LineAssembler,
    classifier: LineClassifier,
}

/// Turns chunks into logical lines (the [`split_lines`] rules) as they complete. A
/// line ends at LF, in numeric mode also at CR (V.250 §5.7.1); an open `+CUSD` quote
/// waits for more bytes. Only new bytes are searched for a line end.
#[derive(Debug)]
struct LineAssembler {
    numeric: bool,
    /// Bytes of an unfinished line (kept raw so a split UTF-8 char survives).
    partial: Vec<u8>,
    /// Dropping an overlong line until its line end.
    skipping: bool,
}

impl LineAssembler {
    fn new(format: AtResultFormat) -> Self {
        Self {
            numeric: format == AtResultFormat::Numeric,
            partial: Vec::new(),
            skipping: false,
        }
    }

    fn is_end(&self, b: u8) -> bool {
        b == b'\n' || (self.numeric && b == b'\r')
    }

    /// Bytes after the last complete line.
    fn tail(&self) -> &[u8] {
        &self.partial
    }

    fn clear(&mut self) {
        self.partial.clear();
        self.skipping = false;
    }

    fn push(&mut self, mut chunk: &[u8]) -> Vec<String> {
        if self.skipping {
            let Some(e) = chunk.iter().position(|&b| self.is_end(b)) else {
                return Vec::new();
            };
            self.skipping = false;
            chunk = &chunk[e + 1..];
        }
        let Some(e) = chunk.iter().rposition(|&b| self.is_end(b)) else {
            self.partial.extend_from_slice(chunk);
            if self.partial.len() > MAX_DEMUX_LINE {
                self.partial.clear();
                self.skipping = true;
            }
            return Vec::new();
        };
        let end = self.partial.len() + e;
        self.partial.extend_from_slice(chunk);
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
        if self.partial.len() > MAX_DEMUX_LINE {
            self.skipping = !self.partial.last().is_some_and(|&b| self.is_end(b));
            self.partial.clear();
        }
        lines
    }
}

impl ExchangeDemux {
    /// Demux for a verbose (`ATV1`) modem.
    pub fn new(command: &str, solicited_prefixes: &[String]) -> Self {
        Self::with_format(command, solicited_prefixes, AtResultFormat::Verbose)
    }

    /// Demux that judges result codes like the completion check with `format`.
    pub fn with_format(
        command: &str,
        solicited_prefixes: &[String],
        format: AtResultFormat,
    ) -> Self {
        let command = normalize_command(command);
        let solicited_prefixes = merge_solicited_prefixes(&command, solicited_prefixes);
        Self {
            phase: ExchangePhase::AwaitingFirstLine,
            classifier: LineClassifier::new(&command, &solicited_prefixes, format),
            command,
            solicited_prefixes,
            format,
            lines: LineAssembler::new(format),
        }
    }

    /// Whether the echo of the command has been seen.
    pub fn phase(&self) -> ExchangePhase {
        self.phase
    }

    pub fn reset(&mut self) {
        self.phase = ExchangePhase::AwaitingFirstLine;
        self.lines.clear();
        self.classifier = LineClassifier::new(&self.command, &self.solicited_prefixes, self.format);
    }

    /// Process complete lines from a chunk; returns new URC lines to emit live.
    ///
    /// Cost is linear in the chunk: only the new bytes are searched for a line end,
    /// and an unterminated line is capped at [`MAX_DEMUX_LINE`].
    pub fn process_chunk(&mut self, chunk: &[u8]) -> Vec<String> {
        let lines = self.lines.push(chunk);
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
    let mut classifier = LineClassifier::new("", solicited_prefixes, AtResultFormat::Verbose);
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
