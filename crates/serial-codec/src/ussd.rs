//! USSD: 7-bit packing with the CBS/USSD padding rule (3GPP TS 23.038 §6.1.2.3.1),
//! CBS data coding scheme (23.038 §5) and the `+CUSD` result line (TS 27.007 §7.15).

use crate::sms::gsm7::{self, Tables};
use crate::sms::{hex_decode, ucs2};
use serde::{Deserialize, Serialize};

const CR: u8 = 0x0D;

/// Pack septets after `fill_bits` zero bits. When the last octet has 7 spare bits they
/// carry a CR; when the text ends with CR exactly on an octet boundary another CR is
/// appended, so the receiver can tell padding from data.
pub fn pack_septets(septets: &[u8], fill_bits: u8) -> Vec<u8> {
    let fill = usize::from(fill_bits % 8);
    let mut s = septets.to_vec();
    let bits = fill + 7 * s.len();
    // 7 spare bits → CR fill; CR ending on an octet boundary → extra CR.
    if bits % 8 == 1 || (bits % 8 == 0 && s.last() == Some(&CR)) {
        s.push(CR);
    }
    gsm7::pack(&s, fill_bits)
}

/// Unpack every septet after `fill_bits`, dropping the padding CR (7 spare bits) and
/// the extra CR added after a CR that ended on an octet boundary.
pub fn unpack_septets(octets: &[u8], fill_bits: u8) -> Vec<u8> {
    let fill = usize::from(fill_bits % 8);
    let n = gsm7::septets_in(octets.len(), fill_bits);
    let mut s = gsm7::unpack(octets, fill_bits, n);
    let exact = fill + 7 * n == 8 * octets.len();
    let boundary_before_last = n > 0 && (fill + 7 * (n - 1)) % 8 == 0;
    let doubled = s.len() >= 2 && s[s.len() - 2] == CR && boundary_before_last;
    if s.last() == Some(&CR) && (exact || doubled) {
        s.pop();
    }
    s
}

/// Text → packed USSD string (GSM 7-bit default alphabet).
pub fn encode(text: &str) -> Result<Vec<u8>, char> {
    Ok(pack_septets(&gsm7::encode(text, Tables::DEFAULT)?, 0))
}

/// Packed USSD string → text (GSM 7-bit default alphabet).
pub fn decode_gsm7(octets: &[u8]) -> String {
    gsm7::decode(&unpack_septets(octets, 0), Tables::DEFAULT)
}

/// What a CBS/USSD data coding scheme says about the payload (23.038 §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CbsDcs {
    pub raw: u8,
    pub alphabet: crate::sms::pdu::Alphabet,
    pub compressed: bool,
    /// The payload starts with a language indication (DCS 0x10: two characters + CR
    /// in GSM 7-bit; DCS 0x11: two GSM 7-bit characters packed into two octets).
    pub language_prefix: bool,
    /// Reserved value, decoded as GSM 7-bit as the spec requires.
    pub reserved: bool,
}

impl CbsDcs {
    /// ISO 639-1 language named by the DCS itself (23.038 §5: group 0000 languages 0–14,
    /// group 0010 values 0–4). `None` for "unspecified", reserved values and the
    /// 0x10/0x11 forms, whose language is in the payload.
    pub fn language(&self) -> Option<&'static str> {
        const G0: [&str; 15] = [
            "de", "en", "it", "fr", "es", "nl", "sv", "da", "pt", "fi", "no", "el", "tr", "hu",
            "pl",
        ];
        const G2: [&str; 5] = ["cs", "he", "ar", "ru", "is"];
        let table: &[&str] = match self.raw >> 4 {
            0 => &G0,
            2 => &G2,
            _ => return None,
        };
        table.get(usize::from(self.raw & 0x0F)).copied()
    }

    pub fn decode(raw: u8) -> Self {
        use crate::sms::pdu::Alphabet::*;
        let mut d = CbsDcs {
            raw,
            alphabet: Gsm7,
            compressed: false,
            language_prefix: false,
            reserved: false,
        };
        let alpha = |bits: u8| match bits & 3 {
            0 => (Gsm7, false),
            1 => (Bit8, false),
            2 => (Ucs2, false),
            _ => (Gsm7, true),
        };
        match raw >> 4 {
            0x0 => {}
            0x1 => match raw {
                0x10 => d.language_prefix = true,
                0x11 => (d.alphabet, d.language_prefix) = (Ucs2, true),
                _ => d.reserved = true,
            },
            0x2 => d.reserved = raw > 0x24,
            0x4..=0x7 => {
                d.compressed = raw & 0x20 != 0;
                (d.alphabet, d.reserved) = alpha(raw >> 2);
            }
            0x9 => (d.alphabet, d.reserved) = alpha(raw >> 2),
            0xF => {
                if raw & 0x04 != 0 {
                    d.alphabet = Bit8;
                }
            }
            _ => d.reserved = true,
        }
        d
    }
}

/// A decoded USSD string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UssdText {
    pub text: String,
    /// ISO 639 language: from a DCS 0x10/0x11 prefix, else the one the DCS names
    /// ([`CbsDcs::language`]).
    pub language: Option<String>,
}

/// Decode a network USSD string (the octets, not hex) per its DCS. 8-bit data is read
/// as ISO-8859-1; compressed data (23.042) is not supported and returns `None`.
pub fn decode(payload: &[u8], dcs: u8) -> Option<UssdText> {
    use crate::sms::pdu::Alphabet::*;
    let d = CbsDcs::decode(dcs);
    if d.compressed {
        return None;
    }
    let (language, body): (Option<String>, &[u8]) = match (d.language_prefix, d.alphabet) {
        (true, Ucs2) => {
            let lang = payload.get(..2)?;
            let l = gsm7::decode(&gsm7::unpack(lang, 0, 2), Tables::DEFAULT);
            (Some(l), &payload[2..])
        }
        _ => (None, payload),
    };
    let mut text = match d.alphabet {
        Gsm7 => decode_gsm7(body),
        Ucs2 => ucs2::decode(body),
        Bit8 => body.iter().map(|&b| char::from(b)).collect(),
    };
    let mut language = language.or_else(|| d.language().map(String::from));
    if d.language_prefix && d.alphabet == Gsm7 {
        let mut it = text.chars();
        let lang: String = it.by_ref().take(2).collect();
        if it.next() == Some('\r') {
            language = Some(lang);
            text = it.collect();
        }
    }
    Some(UssdText { text, language })
}

/// `<m>` of `+CUSD` (27.007 §7.15).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CusdStatus {
    /// 0: no further user action required.
    Done,
    /// 1: further user action required (session stays open).
    ActionRequired,
    /// 2: terminated by the network.
    Terminated,
    /// 3: another local client has responded.
    OtherClient,
    /// 4: operation not supported.
    NotSupported,
    /// 5: network timeout.
    Timeout,
    Other(u8),
}

impl CusdStatus {
    pub fn from_code(m: u8) -> Self {
        match m {
            0 => Self::Done,
            1 => Self::ActionRequired,
            2 => Self::Terminated,
            3 => Self::OtherClient,
            4 => Self::NotSupported,
            5 => Self::Timeout,
            n => Self::Other(n),
        }
    }

    pub fn session_active(self) -> bool {
        self == Self::ActionRequired
    }
}

/// One `+CUSD: <m>[,<str>[,<dcs>]]` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cusd {
    pub status: CusdStatus,
    /// `<str>` as sent (quotes removed, `\"` and `\\` unescaped), not decoded; see
    /// [`Cusd::text`], [`Cusd::text_as`] and [`Cusd::text_packed_hex`].
    pub raw: Option<String>,
    /// `<dcs>` when it is a number 0–255.
    pub dcs: Option<u8>,
    /// A `<dcs>` that is present but not a number 0–255 (e.g. `300`), as sent. `dcs` is
    /// then `None` and the text is read with the default DCS 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_dcs: Option<String>,
}

/// Parse one logical `+CUSD` line (prefix case-insensitive). The string may hold
/// commas, CR/LF and quotes; the closing quote is the last one on the line. A trailing
/// `OK` glued to the line after the string or the DCS is ignored; an `OK` inside an
/// unclosed string is kept as text. `None` if this is not a `+CUSD` line.
pub fn parse_cusd(line: &str) -> Option<Cusd> {
    let line = line.trim();
    let rest = line
        .get(..6)
        .filter(|p| p.eq_ignore_ascii_case("+CUSD:"))
        .map(|_| line[6..].trim_start())?;
    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    let status = CusdStatus::from_code(rest[..digits].parse().ok()?);
    let mut cusd = Cusd {
        status,
        raw: None,
        dcs: None,
        invalid_dcs: None,
    };
    let tail = rest[digits..].trim();
    if strip_ok(tail).is_empty() {
        return Some(cusd);
    }
    let tail = tail.strip_prefix(',')?.trim_start();
    let (raw, after) = if let Some(q) = tail.strip_prefix('"') {
        match q.rfind('"') {
            Some(end) => (unescape(&q[..end]), q[end + 1..].trim()),
            None => (unescape(q), ""),
        }
    } else {
        // Unquoted: the last `,<digits>` is the DCS.
        let tail = strip_ok(tail);
        match tail.rfind(',') {
            Some(i) if is_number(tail[i + 1..].trim()) => {
                (tail[..i].trim().to_string(), &tail[i..])
            }
            _ => (tail.to_string(), ""),
        }
    };
    cusd.raw = Some(raw);
    match strip_ok(after).strip_prefix(',').map(str::trim) {
        None | Some("") => {}
        Some(d) => match d.parse::<u8>() {
            Ok(v) => cusd.dcs = Some(v),
            Err(_) => cusd.invalid_dcs = Some(d.to_string()),
        },
    }
    Some(cusd)
}

fn is_number(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// Drop an `OK` glued to the end of the line (after the DCS, a quote or nothing).
fn strip_ok(s: &str) -> &str {
    let t = s.trim_end();
    if let Some(head) = t.strip_suffix("OK").or_else(|| t.strip_suffix("ok")) {
        let h = head.trim_end();
        if h.is_empty() || h.ends_with('"') || h.ends_with(|c: char| c.is_ascii_digit()) {
            return h;
        }
    }
    t
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        match (c, it.peek()) {
            ('\\', Some(&n @ ('"' | '\\'))) => {
                out.push(n);
                it.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// TE character set chosen with `AT+CSCS` (27.007 §5.5). With a GSM 7-bit `<dcs>` the
/// modem converts `<str>` into it (27.007 §7.15); the parser cannot see `+CSCS`, so the
/// caller says which one is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cscs {
    /// `"GSM"`: the characters themselves; `<str>` is returned as sent.
    #[default]
    Gsm,
    /// `"IRA"` and the other text sets (`"8859-1"`, `"PCCP437"`, `"UTF-8"`): as sent.
    Ira,
    /// `"UCS2"`: hex of UTF-16BE code units (`"0031"` → `"1"`).
    Ucs2,
    /// `"HEX"`: one hex octet per unpacked GSM 7-bit character (`"4869"` → `"Hi"`).
    Hex,
}

impl Cusd {
    /// Decoded text with `+CSCS="GSM"` or `"IRA"` (the default): same as
    /// `text_as(Cscs::Gsm)`.
    pub fn text(&self) -> Option<String> {
        self.text_as(Cscs::Gsm)
    }

    /// Decoded text per 27.007 §7.15 with the given TE character set.
    ///
    /// - 8-bit / UCS2 `<dcs>`: `<str>` is the octets in hex whatever `+CSCS` is; they are
    ///   decoded with [`decode`]. A `<str>` that is not hex is returned as sent.
    /// - GSM 7-bit `<dcs>` (or none: 27.007 default 0): `<str>` is in `cscs`. `None` when
    ///   it is not valid hex for [`Cscs::Ucs2`] / [`Cscs::Hex`].
    ///
    /// `None` without `<str>` and for compressed data (23.042, not supported). Firmware
    /// that sends packed septets in hex under a GSM 7-bit DCS needs
    /// [`Cusd::text_packed_hex`].
    pub fn text_as(&self, cscs: Cscs) -> Option<String> {
        use crate::sms::pdu::Alphabet::*;
        let raw = self.raw.as_deref()?;
        let d = CbsDcs::decode(self.dcs.unwrap_or(0));
        if d.compressed {
            return None;
        }
        match (d.alphabet, cscs) {
            (Ucs2 | Bit8, _) => match hex_decode(raw) {
                Some(b) => decode(&b, d.raw).map(|t| t.text),
                None => Some(raw.to_string()),
            },
            (Gsm7, Cscs::Gsm | Cscs::Ira) => Some(raw.to_string()),
            (Gsm7, Cscs::Ucs2) => hex_decode(raw).map(|b| ucs2::decode(&b)),
            (Gsm7, Cscs::Hex) => hex_decode(raw).map(|b| gsm7::decode(&b, Tables::DEFAULT)),
        }
    }

    /// Not standard: some firmware (Huawei E160 and others, seen in gammu's dumps) sends
    /// the network's packed 7-bit USSD octets in hex under a GSM 7-bit `<dcs>` instead of
    /// converting them to the TE character set. Decode that (`"AA58ACA6AA8D1A"` →
    /// `"*115*5#"`). `None` when `<str>` is not hex; with an 8-bit / UCS2 `<dcs>` this is
    /// [`Cusd::text`].
    pub fn text_packed_hex(&self) -> Option<String> {
        let d = CbsDcs::decode(self.dcs.unwrap_or(0));
        if d.alphabet != crate::sms::pdu::Alphabet::Gsm7 {
            return self.text();
        }
        decode(&hex_decode(self.raw.as_deref()?)?, d.raw).map(|t| t.text)
    }
}
