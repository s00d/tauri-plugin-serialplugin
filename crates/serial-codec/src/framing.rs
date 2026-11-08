//! Byte stream framing: turn arbitrary read chunks into complete frames.
//!
//! The parsers follow the semantics of the Node.js `@serialport/parser-*`
//! packages (their test suites are ported in this module's tests), with these
//! deliberate differences:
//!
//! - Time is passed in (`now: Instant`) instead of using timers, so a polling
//!   read loop drives timeouts with [`Framer::poll`] / [`Framer::deadline`].
//! - Frames are bytes. Node's `encoding` option (readline, regex) only changes how
//!   Node renders a frame as a string; decoding is left to the caller.
//! - Buffers are bounded (`maxBufferSize`, default 64 KiB; SLIP uses the default,
//!   byteLength and ccTalk are bounded by their frame size, ready keeps only a
//!   marker-sized tail): a stream that never completes a frame cannot grow memory
//!   forever. Delimiter, readline, regex, inter-byte timeout, packetLength and SLIP
//!   emit the full buffer as a frame; start/end drops the overlong open frame
//!   (keeping a later start marker).
//! - Work per pushed byte is bounded: no parser rescans its whole buffer on every
//!   push, so a stream without delimiters fed byte by byte stays linear. `regex`
//!   looks back [`REGEX_LOOKBACK`] bytes for a match that started in earlier data,
//!   and rescans the whole buffer only each time it has doubled: a longer match
//!   that arrives in pieces is still found, at the latest when the buffered text
//!   has doubled (or with the next match).
//! - Unknown option names are rejected (a typo would otherwise silently fall back
//!   to a default); the Node names and the aliases listed on each field are accepted.
//! - `ready` finds its marker after a false start: Node resets its match on a
//!   mismatching byte without re-checking that byte, so `"RREADY"` never matches
//!   `"READY"`.
//! - `packetLength` scans for a multi-byte delimiter with a sliding window: Node
//!   drops the *newest* byte on a mismatch, so it only finds a multi-byte delimiter
//!   at stream start or right after a packet (`00 ab ab ba` misses `0xababba`).
//!   At end of stream only a started packet is flushed (Node also flushes the
//!   partial delimiter window, byte-reversed). A packet ends once it is *at least*
//!   `len + packetOverhead` long, so a config where the header outgrows that
//!   cannot buffer forever.
//! - `regex` keeps an incomplete UTF-8 sequence for the next chunk (Node decodes
//!   each chunk alone and turns a split character into U+FFFD). The pattern is
//!   [`regex_lite`] syntax, and capture groups are not spliced into the output
//!   the way JavaScript `String.split` does.
//! - Empty frames are never emitted (Node streams drop empty pushes too).

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::{Duration, Instant};

/// Default cap on buffered, not yet framed bytes.
pub const DEFAULT_MAX_BUFFER: usize = 65_536;

/// How far back (in bytes) the `regex` framer re-scans old text on every push for
/// a match that may end in newly pushed data.
pub const REGEX_LOOKBACK: usize = 32;

/// A stateful framer fed with read chunks.
pub trait Framer: Send {
    /// Feed bytes received at `now`; complete frames are appended to `out`.
    fn push(&mut self, data: &[u8], now: Instant, out: &mut Vec<Vec<u8>>);
    /// Emit frames that complete by time alone (inter-byte timeout).
    fn poll(&mut self, _now: Instant, _out: &mut Vec<Vec<u8>>) {}
    /// When [`poll`](Framer::poll) next has something to do.
    fn deadline(&self) -> Option<Instant> {
        None
    }
    /// End of stream: the buffered partial frame, if any.
    fn flush(&mut self) -> Option<Vec<u8>>;
}

/// Invalid parser options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError(pub String);

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid parser options: {}", self.0)
    }
}

impl std::error::Error for ConfigError {}

fn bad<T>(msg: &str) -> Result<T, ConfigError> {
    Err(ConfigError(msg.into()))
}

/// Bytes given as a UTF-8 string or as an array of byte values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ByteSeq {
    Text(String),
    Bytes(Vec<u8>),
}

impl ByteSeq {
    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            ByteSeq::Text(s) => s.as_bytes().to_vec(),
            ByteSeq::Bytes(b) => b.clone(),
        }
    }
}

impl From<&str> for ByteSeq {
    fn from(s: &str) -> Self {
        ByteSeq::Text(s.into())
    }
}

impl From<&[u8]> for ByteSeq {
    fn from(b: &[u8]) -> Self {
        ByteSeq::Bytes(b.to_vec())
    }
}

fn non_empty(seq: &ByteSeq, name: &str) -> Result<Vec<u8>, ConfigError> {
    let v = seq.to_vec();
    if v.is_empty() {
        return bad(&format!("\"{name}\" has a 0 length"));
    }
    Ok(v)
}

fn max_buf(v: Option<usize>) -> Result<usize, ConfigError> {
    match v {
        Some(0) => bad("\"maxBufferSize\" must be greater than 0"),
        Some(n) => Ok(n),
        None => Ok(DEFAULT_MAX_BUFFER),
    }
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from > hay.len() || needle.len() > hay.len() - from {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

fn emit(out: &mut Vec<Vec<u8>>, frame: &[u8]) {
    if !frame.is_empty() {
        out.push(frame.to_vec());
    }
}

fn take_flush(buf: &mut Vec<u8>) -> Option<Vec<u8>> {
    let rest = std::mem::take(buf);
    (!rest.is_empty()).then_some(rest)
}

/// Parser selection, as sent from JavaScript (`{ type: "readline", … }`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum FramerConfig {
    /// Split on a byte sequence (`@serialport/parser-delimiter`).
    Delimiter {
        delimiter: ByteSeq,
        #[serde(default)]
        include_delimiter: bool,
        #[serde(default)]
        max_buffer_size: Option<usize>,
    },
    /// Split on `"\n"` by default (`@serialport/parser-readline`).
    Readline {
        #[serde(default)]
        delimiter: Option<ByteSeq>,
        #[serde(default)]
        include_delimiter: bool,
        #[serde(default)]
        max_buffer_size: Option<usize>,
        /// Node's string-rendering option; accepted and ignored (frames are bytes).
        #[serde(default)]
        encoding: Option<String>,
    },
    /// Fixed-size frames (`@serialport/parser-byte-length`).
    ByteLength { length: usize },
    /// A frame ends after `interval_ms` of silence (`@serialport/parser-inter-byte-timeout`).
    InterByteTimeout {
        #[serde(alias = "interval")]
        interval_ms: u64,
        #[serde(default)]
        max_buffer_size: Option<usize>,
    },
    /// Delimiter + little-endian length field (`@serialport/parser-packet-length`).
    PacketLength {
        /// One value or a list; read big-endian over `delimiterBytes` wire bytes.
        #[serde(default)]
        delimiter: Option<OneOrMany>,
        #[serde(default)]
        delimiter_bytes: Option<usize>,
        #[serde(default)]
        packet_overhead: Option<usize>,
        #[serde(default)]
        length_bytes: Option<usize>,
        #[serde(default)]
        length_offset: Option<usize>,
        #[serde(default)]
        max_len: Option<usize>,
        #[serde(default)]
        max_buffer_size: Option<usize>,
    },
    /// Drop everything until a marker, then pass data through (`@serialport/parser-ready`).
    Ready { delimiter: ByteSeq },
    /// Split on a regular expression over UTF-8 text (`@serialport/parser-regex`).
    Regex {
        regex: String,
        #[serde(default)]
        max_buffer_size: Option<usize>,
        /// Node's string-rendering option; accepted and ignored (frames are bytes).
        #[serde(default)]
        encoding: Option<String>,
    },
    /// Frames between a start and an end marker (`@serialport/parser-start-end`).
    StartEnd {
        start_delimiter: ByteSeq,
        end_delimiter: ByteSeq,
        #[serde(default)]
        include_start_delimiter: bool,
        #[serde(default)]
        include_end_delimiter: bool,
        #[serde(default)]
        max_buffer_size: Option<usize>,
    },
    /// SLIP (RFC 1055) decoder (`@serialport/parser-slip-encoder` `SlipDecoder`).
    Slip(SlipOptions),
    /// ccTalk frames, length in byte 1 (`@serialport/parser-cctalk`).
    CcTalk {
        /// Drop a partial frame after this gap; 0 disables. Default 50.
        #[serde(default, alias = "maxDelayBetweenBytesMs")]
        max_delay_ms: Option<u64>,
    },
}

/// A number or a list of numbers (Node accepts both for `delimiter`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(u32),
    Many(Vec<u32>),
}

impl FramerConfig {
    /// Validate options and build the framer.
    pub fn build(&self) -> Result<Box<dyn Framer>, ConfigError> {
        Ok(match self {
            FramerConfig::Delimiter {
                delimiter,
                include_delimiter,
                max_buffer_size,
            } => Box::new(DelimiterFramer::new(
                non_empty(delimiter, "delimiter")?,
                *include_delimiter,
                max_buf(*max_buffer_size)?,
            )),
            FramerConfig::Readline {
                delimiter,
                include_delimiter,
                max_buffer_size,
                encoding: _,
            } => {
                let delimiter = match delimiter {
                    Some(d) => non_empty(d, "delimiter")?,
                    None => b"\n".to_vec(),
                };
                Box::new(DelimiterFramer::new(
                    delimiter,
                    *include_delimiter,
                    max_buf(*max_buffer_size)?,
                ))
            }
            FramerConfig::ByteLength { length } => {
                if *length == 0 {
                    return bad("\"length\" must be greater than 0");
                }
                Box::new(ByteLengthFramer {
                    length: *length,
                    buf: Vec::new(),
                })
            }
            FramerConfig::InterByteTimeout {
                interval_ms,
                max_buffer_size,
            } => {
                if *interval_ms == 0 {
                    return bad("\"intervalMs\" must be greater than 0");
                }
                Box::new(InterByteTimeoutFramer {
                    interval: Duration::from_millis(*interval_ms),
                    max: max_buf(*max_buffer_size)?,
                    buf: Vec::new(),
                    deadline: None,
                })
            }
            FramerConfig::PacketLength {
                delimiter,
                delimiter_bytes,
                packet_overhead,
                length_bytes,
                length_offset,
                max_len,
                max_buffer_size,
            } => {
                let p = PacketLengthFramer {
                    delimiters: match delimiter {
                        None => vec![0xaa],
                        Some(OneOrMany::One(d)) => vec![*d],
                        Some(OneOrMany::Many(d)) => d.clone(),
                    },
                    delimiter_bytes: delimiter_bytes.unwrap_or(1),
                    packet_overhead: packet_overhead.unwrap_or(2),
                    length_bytes: length_bytes.unwrap_or(1),
                    length_offset: length_offset.unwrap_or(1),
                    max_len: max_len.unwrap_or(0xff),
                    max: max_buf(*max_buffer_size)?,
                    buf: Vec::new(),
                    started: false,
                };
                if p.delimiters.is_empty() || !(1..=4).contains(&p.delimiter_bytes) {
                    return bad("\"delimiter\" needs 1..=4 \"delimiterBytes\"");
                }
                if !(1..=8).contains(&p.length_bytes) {
                    return bad("\"lengthBytes\" must be 1..=8");
                }
                if p.length_offset < p.delimiter_bytes {
                    return bad("\"lengthOffset\" overlaps the delimiter");
                }
                // The header must fit in the buffer, or no packet could ever start.
                if p.length_offset
                    .checked_add(p.length_bytes)
                    .is_none_or(|end| end > p.max)
                {
                    return bad("\"lengthOffset\" + \"lengthBytes\" exceeds \"maxBufferSize\"");
                }
                Box::new(p)
            }
            FramerConfig::Ready { delimiter } => Box::new(ReadyFramer {
                delimiter: non_empty(delimiter, "delimiter")?,
                tail: Vec::new(),
                ready: false,
            }),
            FramerConfig::Regex {
                regex,
                max_buffer_size,
                encoding: _,
            } => Box::new(RegexFramer {
                regex: regex_lite::Regex::new(regex)
                    .map_err(|e| ConfigError(format!("\"regex\": {e}")))?,
                max: max_buf(*max_buffer_size)?,
                text: String::new(),
                pending: Vec::new(),
                scanned: 0,
                full_scan: 0,
            }),
            FramerConfig::StartEnd {
                start_delimiter,
                end_delimiter,
                include_start_delimiter,
                include_end_delimiter,
                max_buffer_size,
            } => Box::new(StartEndFramer {
                start: non_empty(start_delimiter, "startDelimiter")?,
                end: non_empty(end_delimiter, "endDelimiter")?,
                include_start: *include_start_delimiter,
                include_end: *include_end_delimiter,
                max: max_buf(*max_buffer_size)?,
                buf: Vec::new(),
                open: None,
                scan_from: 0,
            }),
            FramerConfig::Slip(o) => Box::new(SlipDecoder::new(o.clone())),
            FramerConfig::CcTalk { max_delay_ms } => Box::new(CcTalkFramer {
                max_delay: Duration::from_millis(max_delay_ms.unwrap_or(50)),
                buf: Vec::new(),
                last: None,
            }),
        })
    }
}

/// Split on a delimiter; also used for readline.
struct DelimiterFramer {
    delimiter: Vec<u8>,
    include: bool,
    max: usize,
    buf: Vec<u8>,
}

impl DelimiterFramer {
    fn new(delimiter: Vec<u8>, include: bool, max: usize) -> Self {
        Self {
            delimiter,
            include,
            max,
            buf: Vec::new(),
        }
    }
}

impl Framer for DelimiterFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        // Only the tail that can still hold part of a delimiter needs rescanning.
        let mut from = self.buf.len().saturating_sub(self.delimiter.len() - 1);
        self.buf.extend_from_slice(data);
        let mut start = 0;
        while let Some(pos) = find(&self.buf, &self.delimiter, from.max(start)) {
            let end = pos
                + if self.include {
                    self.delimiter.len()
                } else {
                    0
                };
            emit(out, &self.buf[start..end]);
            start = pos + self.delimiter.len();
            from = start;
        }
        self.buf.drain(..start);
        // ponytail: an overlong line is cut at max bytes, like a full receive buffer.
        while self.buf.len() >= self.max {
            let rest = self.buf.split_off(self.max);
            out.push(std::mem::replace(&mut self.buf, rest));
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        take_flush(&mut self.buf)
    }
}

struct ByteLengthFramer {
    length: usize,
    buf: Vec<u8>,
}

impl Framer for ByteLengthFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        self.buf.extend_from_slice(data);
        let full = self.buf.len() / self.length * self.length;
        for frame in self.buf[..full].chunks(self.length) {
            out.push(frame.to_vec());
        }
        self.buf.drain(..full);
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        take_flush(&mut self.buf)
    }
}

struct InterByteTimeoutFramer {
    interval: Duration,
    max: usize,
    buf: Vec<u8>,
    deadline: Option<Instant>,
}

impl Framer for InterByteTimeoutFramer {
    fn push(&mut self, data: &[u8], now: Instant, out: &mut Vec<Vec<u8>>) {
        // A late poll must not merge a packet that already timed out with new data.
        self.poll(now, out);
        for &b in data {
            self.buf.push(b);
            if self.buf.len() >= self.max {
                out.push(std::mem::take(&mut self.buf));
            }
        }
        if !data.is_empty() {
            self.deadline = Some(now + self.interval);
        }
    }

    fn poll(&mut self, now: Instant, out: &mut Vec<Vec<u8>>) {
        if self.deadline.is_some_and(|d| now >= d) {
            self.deadline = None;
            emit(out, &std::mem::take(&mut self.buf));
        }
    }

    fn deadline(&self) -> Option<Instant> {
        self.deadline.filter(|_| !self.buf.is_empty())
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        self.deadline = None;
        take_flush(&mut self.buf)
    }
}

struct PacketLengthFramer {
    delimiters: Vec<u32>,
    delimiter_bytes: usize,
    packet_overhead: usize,
    length_bytes: usize,
    length_offset: usize,
    max_len: usize,
    max: usize,
    buf: Vec<u8>,
    started: bool,
}

impl Framer for PacketLengthFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        for &b in data {
            self.buf.push(b);
            if !self.started {
                // Sliding window over the last `delimiter_bytes` bytes, read big-endian
                // (wire order), the same value Node builds by prepending bytes.
                if self.buf.len() > self.delimiter_bytes {
                    self.buf.remove(0);
                }
                if self.buf.len() == self.delimiter_bytes {
                    let v = self.buf.iter().fold(0u32, |a, &x| (a << 8) | x as u32);
                    self.started = self.delimiters.contains(&v);
                }
                continue;
            }
            let end = self.length_offset + self.length_bytes;
            if self.buf.len() >= end {
                let len = self.buf[self.length_offset..end]
                    .iter()
                    .rev()
                    .fold(0u64, |a, &x| (a << 8) | x as u64);
                let total = len.saturating_add(self.packet_overhead as u64);
                if self.buf.len() as u64 >= total || len > self.max_len as u64 {
                    out.push(std::mem::take(&mut self.buf));
                    self.started = false;
                    continue;
                }
            }
            if self.buf.len() >= self.max {
                // A packet that can never complete within maxBufferSize.
                out.push(std::mem::take(&mut self.buf));
                self.started = false;
            }
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        let started = std::mem::take(&mut self.started);
        let rest = std::mem::take(&mut self.buf);
        (started && !rest.is_empty()).then_some(rest)
    }
}

struct ReadyFramer {
    delimiter: Vec<u8>,
    tail: Vec<u8>,
    ready: bool,
}

impl Framer for ReadyFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        if self.ready {
            emit(out, data);
            return;
        }
        let keep = self.tail.len();
        self.tail.extend_from_slice(data);
        if let Some(pos) = find(&self.tail, &self.delimiter, 0) {
            self.ready = true;
            let rest = pos + self.delimiter.len();
            emit(out, &self.tail[rest.max(keep)..]);
            self.tail = Vec::new();
        } else {
            let cut = self.tail.len().saturating_sub(self.delimiter.len() - 1);
            self.tail.drain(..cut);
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        None
    }
}

struct RegexFramer {
    regex: regex_lite::Regex,
    max: usize,
    text: String,
    /// Incomplete UTF-8 sequence at the end of the last chunk.
    pending: Vec<u8>,
    /// Length of `text` at the last search.
    scanned: usize,
    /// Length of `text` at the last search from its start.
    full_scan: usize,
}

impl RegexFramer {
    fn decode(&mut self, data: &[u8]) {
        let mut bytes = std::mem::take(&mut self.pending);
        bytes.extend_from_slice(data);
        let mut rest: &[u8] = &bytes;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    self.text.push_str(s);
                    return;
                }
                Err(e) => {
                    let (ok, tail) = rest.split_at(e.valid_up_to());
                    self.text
                        .push_str(std::str::from_utf8(ok).unwrap_or_default());
                    match e.error_len() {
                        Some(n) => {
                            self.text.push('\u{FFFD}');
                            rest = &tail[n..];
                        }
                        None => {
                            self.pending = tail.to_vec();
                            return;
                        }
                    }
                }
            }
        }
    }
}

impl Framer for RegexFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        self.decode(data);
        // Matches that ended in old text were split off by an earlier push, so only
        // a match reaching into the new text can be missing. Look for one from a
        // short lookback, and from the start each time the text has doubled (linear
        // in total). The full split runs only when there is something to split, and
        // then consumes everything before the last match.
        let mut from = self.scanned.saturating_sub(REGEX_LOOKBACK);
        if self.text.len() >= 2 * self.full_scan {
            from = 0;
            self.full_scan = self.text.len();
        }
        while !self.text.is_char_boundary(from) {
            from -= 1;
        }
        if self.regex.find_at(&self.text, from).is_some() {
            let mut parts: Vec<&str> = self.regex.split(&self.text).collect();
            let last = parts.pop().unwrap_or_default().to_string();
            for p in parts {
                emit(out, p.as_bytes());
            }
            self.text = last;
            self.full_scan = self.text.len();
        }
        self.scanned = self.text.len();
        if self.text.len() >= self.max {
            out.push(std::mem::take(&mut self.text).into_bytes());
            self.scanned = 0;
            self.full_scan = 0;
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        self.scanned = 0;
        self.full_scan = 0;
        let mut rest = std::mem::take(&mut self.text).into_bytes();
        rest.append(&mut self.pending);
        (!rest.is_empty()).then_some(rest)
    }
}

struct StartEndFramer {
    start: Vec<u8>,
    end: Vec<u8>,
    include_start: bool,
    include_end: bool,
    max: usize,
    buf: Vec<u8>,
    /// Start of the open frame (a start marker found, no end yet).
    open: Option<usize>,
    /// Where the next search (for a start marker, or for the end of the open
    /// frame) resumes; everything before it has been searched.
    scan_from: usize,
}

impl Framer for StartEndFramer {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        self.buf.extend_from_slice(data);
        let mut consumed = 0;
        loop {
            let s = match self.open {
                Some(s) => s,
                None => {
                    let Some(s) = find(&self.buf, &self.start, self.scan_from) else {
                        let tail = self.buf.len().saturating_sub(self.start.len() - 1);
                        self.scan_from = tail.max(consumed);
                        break;
                    };
                    self.open = Some(s);
                    self.scan_from = s + self.start.len();
                    s
                }
            };
            let Some(e) = find(&self.buf, &self.end, self.scan_from) else {
                let tail = self.buf.len().saturating_sub(self.end.len() - 1);
                self.scan_from = tail.max(s + self.start.len());
                break;
            };
            let from = if self.include_start {
                s
            } else {
                s + self.start.len()
            };
            let to = if self.include_end {
                e + self.end.len()
            } else {
                e
            };
            emit(out, &self.buf[from..to]);
            consumed = e + self.end.len();
            self.open = None;
            self.scan_from = consumed;
        }
        self.buf.drain(..consumed);
        self.open = self.open.map(|s| s - consumed);
        self.scan_from -= consumed;
        if self.buf.len() >= self.max {
            // Rescanned from scratch: once per max bytes at most.
            self.open = None;
            self.scan_from = 0;
            // An open frame that never ends: keep only from the last start marker.
            match self
                .buf
                .windows(self.start.len())
                .rposition(|w| w == self.start)
            {
                Some(p) if p > 0 => {
                    self.buf.drain(..p);
                }
                _ => self.buf.clear(),
            }
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        self.open = None;
        self.scan_from = 0;
        take_flush(&mut self.buf)
    }
}

/// SLIP framing bytes (RFC 1055 defaults; `start`/`esc_start` optional).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default, deny_unknown_fields)]
pub struct SlipOptions {
    #[serde(alias = "START")]
    pub start: Option<u8>,
    #[serde(alias = "ESC")]
    pub esc: u8,
    #[serde(alias = "END")]
    pub end: u8,
    #[serde(alias = "ESC_START")]
    pub esc_start: Option<u8>,
    #[serde(alias = "ESC_END")]
    pub esc_end: u8,
    #[serde(alias = "ESC_ESC")]
    pub esc_esc: u8,
}

impl Default for SlipOptions {
    fn default() -> Self {
        Self {
            start: None,
            esc: 0xdb,
            end: 0xc0,
            esc_start: None,
            esc_end: 0xdc,
            esc_esc: 0xdd,
        }
    }
}

/// SLIP-encode one packet. `bluetooth_quirk` adds a leading END and skips empty packets.
pub fn slip_encode(data: &[u8], o: &SlipOptions, bluetooth_quirk: bool) -> Vec<u8> {
    if bluetooth_quirk && data.is_empty() {
        return Vec::new();
    }
    let mut v = Vec::with_capacity(data.len() * 2 + 3);
    if bluetooth_quirk {
        v.push(o.end);
    }
    if let Some(s) = o.start {
        v.push(s);
    }
    for &b in data {
        match b {
            b if Some(b) == o.start && o.esc_start.is_some() => {
                v.extend([o.esc, o.esc_start.unwrap_or_default()])
            }
            b if b == o.end => v.extend([o.esc, o.esc_end]),
            b if b == o.esc => v.extend([o.esc, o.esc_esc]),
            b => v.push(b),
        }
    }
    v.push(o.end);
    v
}

/// SLIP decoder with Node `SlipDecoder` semantics.
pub struct SlipDecoder {
    o: SlipOptions,
    buf: Vec<u8>,
    escape: bool,
    started: bool,
}

impl SlipDecoder {
    pub fn new(o: SlipOptions) -> Self {
        Self {
            o,
            buf: Vec::new(),
            escape: false,
            started: false,
        }
    }
}

impl Framer for SlipDecoder {
    fn push(&mut self, data: &[u8], _now: Instant, out: &mut Vec<Vec<u8>>) {
        let o = &self.o;
        for &raw in data {
            let mut byte = raw;
            if Some(byte) == o.start {
                self.started = true;
                continue;
            }
            if o.start.is_none() {
                self.started = true;
            }
            if self.escape {
                if o.start.is_some() && Some(byte) == o.esc_start {
                    byte = o.start.unwrap_or_default();
                } else if byte == o.esc_esc {
                    byte = o.esc;
                } else if byte == o.esc_end {
                    byte = o.end;
                } else {
                    // Invalid escape: end the frame, the byte starts the next one.
                    emit(out, &std::mem::take(&mut self.buf));
                }
            } else if byte == o.esc {
                self.escape = true;
                continue;
            } else if byte == o.end {
                emit(out, &std::mem::take(&mut self.buf));
                self.escape = false;
                self.started = false;
                continue;
            }
            self.escape = false;
            if self.started {
                self.buf.push(byte);
                if self.buf.len() >= DEFAULT_MAX_BUFFER {
                    out.push(std::mem::take(&mut self.buf));
                }
            }
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        self.escape = false;
        self.started = false;
        take_flush(&mut self.buf)
    }
}

struct CcTalkFramer {
    max_delay: Duration,
    buf: Vec<u8>,
    last: Option<Instant>,
}

impl Framer for CcTalkFramer {
    fn push(&mut self, data: &[u8], now: Instant, out: &mut Vec<Vec<u8>>) {
        if !self.max_delay.is_zero() {
            if self
                .last
                .is_some_and(|t| now.duration_since(t) > self.max_delay)
            {
                self.buf.clear();
            }
            self.last = Some(now);
        }
        self.buf.extend_from_slice(data);
        while self.buf.len() > 1 && self.buf.len() >= self.buf[1] as usize + 5 {
            let n = self.buf[1] as usize + 5;
            out.push(self.buf.drain(..n).collect());
        }
    }

    fn flush(&mut self) -> Option<Vec<u8>> {
        // Node's ccTalk parser has no flush: a partial frame is dropped.
        self.buf.clear();
        None
    }
}

#[cfg(test)]
mod tests;
