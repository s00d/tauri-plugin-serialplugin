//! 3GPP TS 23.040 TPDUs in TS 27.005 PDU-mode form (SMSC address + TPDU).
//!
//! Decoding is strict about lengths (a TP-UDL that points past the data is an error,
//! so is an odd UCS2 payload) and lenient about values the spec tells receivers to
//! tolerate: reserved DCS groups decode as GSM 7-bit (23.038 §4), TOA bit 7 is not
//! checked, trailing octets after the user data are ignored, an SCTS that is not a
//! valid date decodes as `None` (the raw octets stay available).

use super::gsm7::{self, Language, Tables};
use super::{hex_decode, ucs2};
use serde::{Deserialize, Serialize};
use std::fmt;

mod encode;
pub use encode::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PduError {
    /// Input is not an even-length hex string.
    InvalidHex,
    /// Data ended inside the named field.
    Truncated(&'static str),
    /// The named field holds a value the spec does not allow.
    Invalid(&'static str),
    /// TP-MTI 0b11 (reserved, 23.040 §9.2.3.1).
    ReservedMessageType,
    /// Encoding: character not representable in the chosen alphabet.
    Unencodable(char),
    /// Encoding: the message needs more than 255 concatenated parts.
    TooManyParts,
    /// Encoding: the message needs concatenation but
    /// [`SubmitRequest::concat_ref`] is `None`.
    MissingConcatRef,
}

impl fmt::Display for PduError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHex => f.write_str("PDU is not a valid hex string"),
            Self::Truncated(field) => write!(f, "PDU truncated in {field}"),
            Self::Invalid(field) => write!(f, "invalid {field}"),
            Self::ReservedMessageType => f.write_str("reserved TP-MTI"),
            Self::Unencodable(c) => write!(f, "character {c:?} is not encodable"),
            Self::TooManyParts => f.write_str("message needs more than 255 parts"),
            Self::MissingConcatRef => {
                f.write_str("message needs concatenation: set a concatenation reference")
            }
        }
    }
}

impl std::error::Error for PduError {}

type Result<T> = std::result::Result<T, PduError>;

// ---------------------------------------------------------------- addresses

/// Type of number, TOA bits 6–4 (23.040 §9.1.2.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ton {
    Unknown,
    International,
    National,
    NetworkSpecific,
    Subscriber,
    Alphanumeric,
    Abbreviated,
    Reserved,
}

impl Ton {
    pub fn from_toa(toa: u8) -> Self {
        match (toa >> 4) & 7 {
            0 => Self::Unknown,
            1 => Self::International,
            2 => Self::National,
            3 => Self::NetworkSpecific,
            4 => Self::Subscriber,
            5 => Self::Alphanumeric,
            6 => Self::Abbreviated,
            _ => Self::Reserved,
        }
    }
}

/// TOA for international numbers, ISDN/E.164 plan.
pub const TOA_INTERNATIONAL: u8 = 0x91;
/// TOA for unknown type, ISDN/E.164 plan (what most phones send without `+`).
pub const TOA_UNKNOWN: u8 = 0x81;
/// TOA for national numbers, ISDN/E.164 plan.
pub const TOA_NATIONAL: u8 = 0xA1;
/// TOA for alphanumeric (GSM 7-bit packed) addresses.
pub const TOA_ALPHANUMERIC: u8 = 0xD0;

/// An address field: type of address octet and its value — digits (`0-9 * # a b c`)
/// or, for TON 5, the decoded text. The value never carries a `+`; [`Display`] adds
/// one for international numbers.
///
/// [`Display`]: fmt::Display
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Address {
    pub toa: u8,
    pub value: String,
}

impl Address {
    pub fn ton(&self) -> Ton {
        Ton::from_toa(self.toa)
    }

    /// Numbering plan identification, TOA bits 3–0.
    pub fn npi(&self) -> u8 {
        self.toa & 0x0F
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ton() == Ton::International && !self.value.is_empty() {
            f.write_str("+")?;
        }
        f.write_str(&self.value)
    }
}

const SEMI_OCTET: &[u8; 15] = b"0123456789*#abc";

/// Swapped semi-octets → digits; stops at the first `F` filler. `digits` caps the count.
pub fn decode_semi_octets(bytes: &[u8], digits: usize) -> String {
    bytes
        .iter()
        .flat_map(|b| [b & 0x0F, b >> 4])
        .take(digits)
        .take_while(|&n| n != 0x0F)
        .map(|n| char::from(SEMI_OCTET[usize::from(n)]))
        .collect()
}

/// Digits (`0-9 * # a b c`, case-insensitive) → swapped semi-octets, `F`-padded.
/// `Err(c)` names the first character that has no semi-octet value.
pub fn encode_semi_octets(digits: &str) -> std::result::Result<Vec<u8>, char> {
    let nibbles = digits
        .chars()
        .map(|c| {
            SEMI_OCTET
                .iter()
                .position(|&d| char::from(d) == c.to_ascii_lowercase())
                .map(|n| n as u8)
                .ok_or(c)
        })
        .collect::<std::result::Result<Vec<u8>, char>>()?;
    Ok(nibbles
        .chunks(2)
        .map(|p| p[0] | (p.get(1).copied().unwrap_or(0x0F) << 4))
        .collect())
}

fn decode_address_value(toa: u8, body: &[u8], semi_octets: usize) -> String {
    if Ton::from_toa(toa) == Ton::Alphanumeric {
        let septets = gsm7::unpack(body, 0, semi_octets * 4 / 7);
        gsm7::decode(&septets, Tables::DEFAULT)
    } else {
        decode_semi_octets(body, semi_octets)
    }
}

/// TP address field (OA/DA/RA): `len(digits) TOA value`. Returns the address and the
/// octets consumed.
pub fn decode_address(bytes: &[u8]) -> Result<(Address, usize)> {
    let mut r = Reader::new(bytes);
    let digits = usize::from(r.u8("address length")?);
    if digits > 20 {
        return Err(PduError::Invalid("address length"));
    }
    let toa = r.u8("address type")?;
    let body = r.take(digits.div_ceil(2), "address")?;
    let value = decode_address_value(toa, body, digits);
    Ok((Address { toa, value }, r.pos))
}

/// SMSC address field (`len(octets incl. TOA) TOA value`); length 0 → `None`.
pub fn decode_smsc(bytes: &[u8]) -> Result<(Option<Address>, usize)> {
    let mut r = Reader::new(bytes);
    let len = usize::from(r.u8("SMSC length")?);
    if len == 0 {
        return Ok((None, 1));
    }
    if len > 11 {
        return Err(PduError::Invalid("SMSC length"));
    }
    let toa = r.u8("SMSC type")?;
    let body = r.take(len - 1, "SMSC address")?;
    let value = decode_address_value(toa, body, body.len() * 2);
    Ok((Some(Address { toa, value }), r.pos))
}

// ---------------------------------------------------------------- timestamps

/// TP-SCTS / TP-DT / absolute TP-VP (23.040 §9.2.3.11): local time plus the offset to
/// UTC in signed quarter hours. Two-digit years map to 1970–2069.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Timestamp {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub tz_quarters: i8,
}

fn bcd(b: u8) -> Option<u8> {
    let (tens, units) = (b & 0x0F, b >> 4);
    (tens <= 9 && units <= 9).then_some(tens * 10 + units)
}

fn to_bcd(v: u8) -> u8 {
    (v % 10) << 4 | (v / 10)
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

impl Timestamp {
    /// Decode 7 swapped-BCD octets; `None` if any field is out of range.
    pub fn decode(b: &[u8; 7]) -> Option<Self> {
        let yy = bcd(b[0])?;
        let year = if yy >= 70 { 1900 } else { 2000 } + u16::from(yy);
        let (month, day, hour, minute, second) =
            (bcd(b[1])?, bcd(b[2])?, bcd(b[3])?, bcd(b[4])?, bcd(b[5])?);
        let tz_quarters = decode_tz(b[6])?;
        let ok = (1..=12).contains(&month)
            && day >= 1
            && day <= days_in_month(year, month)
            && hour < 24
            && minute < 60
            && second < 60;
        ok.then_some(Timestamp {
            year,
            month,
            day,
            hour,
            minute,
            second,
            tz_quarters,
        })
    }

    /// Encode; `None` if the year is outside 1970–2069, the offset beyond ±79
    /// quarters, or a field out of range.
    pub fn encode(&self) -> Option<[u8; 7]> {
        if !(1970..=2069).contains(&self.year) || self.tz_quarters.unsigned_abs() > 79 {
            return None;
        }
        let out = [
            to_bcd((self.year % 100) as u8),
            to_bcd(self.month),
            to_bcd(self.day),
            to_bcd(self.hour),
            to_bcd(self.minute),
            to_bcd(self.second),
            to_bcd(self.tz_quarters.unsigned_abs()) | if self.tz_quarters < 0 { 0x08 } else { 0 },
        ];
        // Round trip rejects out-of-range fields (month 13, Feb 30, …).
        (Self::decode(&out) == Some(*self)).then_some(out)
    }

    pub fn offset_minutes(&self) -> i32 {
        i32::from(self.tz_quarters) * 15
    }

    /// Seconds since 1970-01-01T00:00:00Z.
    pub fn unix_utc(&self) -> i64 {
        // Howard Hinnant's days_from_civil.
        let (m, d) = (i64::from(self.month), i64::from(self.day));
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + d - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146_097 + doe - 719_468;
        days * 86_400
            + i64::from(self.hour) * 3600
            + i64::from(self.minute) * 60
            + i64::from(self.second)
            - i64::from(self.offset_minutes()) * 60
    }
}

/// The time zone octet alone: signed quarter hours, `None` for a non-BCD digit.
pub fn decode_tz(b: u8) -> Option<i8> {
    let q = bcd(b & 0xF7)? as i8;
    Some(if b & 0x08 != 0 { -q } else { q })
}

impl fmt::Display for Timestamp {
    /// ISO 8601 with offset, e.g. `2011-01-01T12:34:56+01:00`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let off = self.offset_minutes();
        write!(
            f,
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            if off < 0 { '-' } else { '+' },
            off.unsigned_abs() / 60,
            off.unsigned_abs() % 60
        )
    }
}

// ---------------------------------------------------------------- validity period

/// TP-VP (23.040 §9.2.3.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Validity {
    /// Relative format octet; see [`relative_minutes`] / [`relative_from_minutes`].
    Relative(u8),
    Absolute(Timestamp),
    /// Enhanced format, raw 7 octets (bits 2–0 of the first octet give the sub-format).
    Enhanced([u8; 7]),
}

impl Validity {
    /// TP-VPF value for the first octet.
    pub fn vpf(&self) -> u8 {
        match self {
            Self::Relative(_) => 2,
            Self::Absolute(_) => 3,
            Self::Enhanced(_) => 1,
        }
    }
}

/// Minutes encoded by a relative TP-VP octet.
pub fn relative_minutes(vp: u8) -> u32 {
    let v = u32::from(vp);
    match vp {
        0..=143 => (v + 1) * 5,
        144..=167 => 720 + (v - 143) * 30,
        168..=196 => (v - 166) * 1440,
        _ => (v - 192) * 10_080,
    }
}

/// Smallest relative TP-VP octet covering `minutes` (rounded up); `None` beyond the
/// 63-week maximum.
pub fn relative_from_minutes(minutes: u32) -> Option<u8> {
    let v = match minutes {
        0..=720 => minutes.div_ceil(5).max(1) - 1,
        721..=1440 => 143 + (minutes - 720).div_ceil(30),
        1441..=43_200 => 166 + minutes.div_ceil(1440),
        _ => 192 + minutes.div_ceil(10_080),
    };
    u8::try_from(v).ok()
}

// ---------------------------------------------------------------- DCS

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Alphabet {
    Gsm7,
    Bit8,
    Ucs2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MwiKind {
    Voicemail,
    Fax,
    Email,
    Other,
}

/// Message waiting indication from DCS groups 1100/1101/1110 (23.038 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Mwi {
    pub active: bool,
    pub kind: MwiKind,
    /// `false` for the "discard message" group (1100).
    pub store: bool,
}

/// TP-DCS for SMS (23.038 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Dcs {
    pub raw: u8,
    pub alphabet: Alphabet,
    /// Message class 0–3, when the DCS carries one.
    pub class: Option<u8>,
    pub compressed: bool,
    /// Coding group 01xx: delete after reading.
    pub auto_delete: bool,
    pub mwi: Option<Mwi>,
    /// Reserved coding group or alphabet; decoded as GSM 7-bit as 23.038 §4 requires.
    pub reserved: bool,
}

impl Dcs {
    pub fn decode(raw: u8) -> Self {
        let mut d = Dcs {
            raw,
            alphabet: Alphabet::Gsm7,
            class: None,
            compressed: false,
            auto_delete: false,
            mwi: None,
            reserved: false,
        };
        match raw >> 4 {
            0x0..=0x7 => {
                d.auto_delete = raw & 0x40 != 0;
                d.compressed = raw & 0x20 != 0;
                d.class = (raw & 0x10 != 0).then_some(raw & 3);
                match (raw >> 2) & 3 {
                    0 => {}
                    1 => d.alphabet = Alphabet::Bit8,
                    2 => d.alphabet = Alphabet::Ucs2,
                    _ => d.reserved = true,
                }
            }
            0xC..=0xE => {
                d.alphabet = if raw >> 4 == 0xE {
                    Alphabet::Ucs2
                } else {
                    Alphabet::Gsm7
                };
                d.mwi = Some(Mwi {
                    active: raw & 0x08 != 0,
                    kind: [
                        MwiKind::Voicemail,
                        MwiKind::Fax,
                        MwiKind::Email,
                        MwiKind::Other,
                    ][usize::from(raw & 3)],
                    store: raw >> 4 != 0xC,
                });
            }
            0xF => {
                if raw & 0x04 != 0 {
                    d.alphabet = Alphabet::Bit8;
                }
                d.class = Some(raw & 3);
            }
            _ => d.reserved = true,
        }
        d
    }

    /// DCS octet for an outgoing message: general data coding group, uncompressed.
    pub fn encode(alphabet: Alphabet, class: Option<u8>) -> u8 {
        let a = match alphabet {
            Alphabet::Gsm7 => 0x00,
            Alphabet::Bit8 => 0x04,
            Alphabet::Ucs2 => 0x08,
        };
        a | class.map_or(0, |c| 0x10 | (c & 3))
    }
}

// ---------------------------------------------------------------- UDH

/// One information element.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ie {
    pub id: u8,
    pub data: Vec<u8>,
}

/// User data header (23.040 §9.2.3.24).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Udh {
    pub ies: Vec<Ie>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Concat {
    pub reference: u16,
    pub total: u8,
    pub seq: u8,
    /// IEI 0x08 (16-bit reference) rather than 0x00.
    pub ref_16bit: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ports {
    pub dest: u16,
    pub orig: u16,
    /// IEI 0x05 (16-bit ports) rather than 0x04.
    pub wide: bool,
}

pub const IEI_CONCAT_8: u8 = 0x00;
pub const IEI_PORTS_8: u8 = 0x04;
pub const IEI_PORTS_16: u8 = 0x05;
pub const IEI_CONCAT_16: u8 = 0x08;
pub const IEI_SINGLE_SHIFT: u8 = 0x24;
pub const IEI_LOCKING_SHIFT: u8 = 0x25;

impl Udh {
    /// Parse the IEs (the octets after UDHL).
    pub fn parse(mut bytes: &[u8]) -> Result<Self> {
        let mut ies = Vec::new();
        while let [id, len, rest @ ..] = bytes {
            let len = usize::from(*len);
            let data = rest.get(..len).ok_or(PduError::Truncated("UDH element"))?;
            ies.push(Ie {
                id: *id,
                data: data.to_vec(),
            });
            bytes = &rest[len..];
        }
        if bytes.is_empty() {
            Ok(Udh { ies })
        } else {
            Err(PduError::Truncated("UDH element"))
        }
    }

    /// UDHL followed by the IEs. `Invalid("UDH length")` when an IE holds more than 255
    /// octets or the header more than 255 (neither length fits its octet; a UDH that
    /// fits a TPDU is at most 139 octets anyway).
    pub fn encode(&self) -> Result<Vec<u8>> {
        let mut out = vec![0];
        for ie in &self.ies {
            let len = u8::try_from(ie.data.len()).map_err(|_| PduError::Invalid("UDH length"))?;
            out.push(ie.id);
            out.push(len);
            out.extend_from_slice(&ie.data);
        }
        out[0] = u8::try_from(out.len() - 1).map_err(|_| PduError::Invalid("UDH length"))?;
        Ok(out)
    }

    fn ie(&self, id: u8) -> Option<&[u8]> {
        self.ies
            .iter()
            .rev()
            .find(|ie| ie.id == id)
            .map(|ie| ie.data.as_slice())
    }

    /// Concatenation info; `None` without the IE or when total/seq are inconsistent.
    pub fn concat(&self) -> Option<Concat> {
        let c = match (self.ie(IEI_CONCAT_8), self.ie(IEI_CONCAT_16)) {
            (_, Some(&[hi, lo, total, seq])) => Concat {
                reference: u16::from_be_bytes([hi, lo]),
                total,
                seq,
                ref_16bit: true,
            },
            (Some(&[r, total, seq]), _) => Concat {
                reference: u16::from(r),
                total,
                seq,
                ref_16bit: false,
            },
            _ => return None,
        };
        (c.total > 0 && c.seq > 0 && c.seq <= c.total).then_some(c)
    }

    pub fn ports(&self) -> Option<Ports> {
        match (self.ie(IEI_PORTS_8), self.ie(IEI_PORTS_16)) {
            (_, Some(&[a, b, c, d])) => Some(Ports {
                dest: u16::from_be_bytes([a, b]),
                orig: u16::from_be_bytes([c, d]),
                wide: true,
            }),
            (Some(&[dest, orig]), _) => Some(Ports {
                dest: u16::from(dest),
                orig: u16::from(orig),
                wide: false,
            }),
            _ => None,
        }
    }

    /// National language identifiers (single shift, locking shift) as sent.
    pub fn national_languages(&self) -> (Option<u8>, Option<u8>) {
        let one = |id| self.ie(id).and_then(|d| d.first().copied());
        (one(IEI_SINGLE_SHIFT), one(IEI_LOCKING_SHIFT))
    }

    /// GSM 7-bit tables selected by the header; identifiers without a table here fall
    /// back to the default table.
    pub fn tables(&self) -> Tables {
        let (single, locking) = self.national_languages();
        let lang = |n: Option<u8>| n.and_then(Language::from_nli).unwrap_or_default();
        let locking = lang(locking);
        Tables::new(
            if locking.has_locking_table() {
                locking
            } else {
                Language::Default
            },
            lang(single),
        )
    }
}

// ---------------------------------------------------------------- user data

/// TP-UD after UDH parsing.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct UserData {
    pub udh: Option<Udh>,
    /// Unpacked septets for GSM 7-bit, octets otherwise (UDH excluded).
    pub payload: Vec<u8>,
    /// Decoded text; `None` for 8-bit and compressed data.
    pub text: Option<String>,
}

/// Decode TP-UD given TP-UDL, TP-UDHI and the DCS. `ud` may be longer than needed.
pub fn decode_user_data(udl: u8, ud: &[u8], udhi: bool, dcs: &Dcs) -> Result<UserData> {
    let udl = usize::from(udl);
    let septets = dcs.alphabet == Alphabet::Gsm7 && !dcs.compressed;
    let (need, max) = if septets {
        ((udl * 7).div_ceil(8), 160)
    } else {
        (udl, 140)
    };
    if udl > max {
        return Err(PduError::Invalid("TP-UDL"));
    }
    let ud = ud.get(..need).ok_or(PduError::Truncated("user data"))?;
    let (udh, hdr) = if udhi {
        let udhl = usize::from(*ud.first().ok_or(PduError::Invalid("TP-UDHL"))?);
        let ies = ud.get(1..=udhl).ok_or(PduError::Invalid("TP-UDHL"))?;
        (Some(Udh::parse(ies)?), udhl + 1)
    } else {
        (None, 0)
    };
    let tables = udh.as_ref().map_or(Tables::DEFAULT, Udh::tables);
    let (payload, text) = if septets {
        let hdr_septets = (hdr * 8).div_ceil(7);
        let count = udl
            .checked_sub(hdr_septets)
            .ok_or(PduError::Invalid("TP-UDHL"))?;
        let fill = (hdr_septets * 7 - hdr * 8) as u8;
        let s = gsm7::unpack(&ud[hdr..], fill, count);
        let text = gsm7::decode(&s, tables);
        (s, Some(text))
    } else {
        let p = ud[hdr..].to_vec();
        let text = match (dcs.alphabet, dcs.compressed) {
            (Alphabet::Ucs2, false) if p.len() % 2 == 1 => {
                return Err(PduError::Invalid("UCS2 length"))
            }
            (Alphabet::Ucs2, false) => Some(ucs2::decode(&p)),
            _ => None,
        };
        (p, text)
    };
    Ok(UserData { udh, payload, text })
}

// ---------------------------------------------------------------- TPDUs

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deliver {
    /// TP-MMS: set = no more messages waiting.
    pub mms: bool,
    /// TP-LP loop prevention.
    pub lp: bool,
    /// TP-SRI: a status report will be returned to the sender.
    pub sri: bool,
    pub udhi: bool,
    pub rp: bool,
    pub originator: Address,
    pub pid: u8,
    pub dcs: Dcs,
    /// `None` when the octets are not a valid date; see `scts_raw`.
    pub scts: Option<Timestamp>,
    pub scts_raw: [u8; 7],
    pub user_data: UserData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Submit {
    /// TP-RD reject duplicates.
    pub rd: bool,
    /// TP-SRR status report request.
    pub srr: bool,
    pub udhi: bool,
    pub rp: bool,
    pub mr: u8,
    pub destination: Address,
    pub pid: u8,
    pub dcs: Dcs,
    pub validity: Option<Validity>,
    pub user_data: UserData,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusReport {
    pub mms: bool,
    pub lp: bool,
    /// TP-SRQ: report of an SMS-COMMAND rather than an SMS-SUBMIT.
    pub srq: bool,
    pub udhi: bool,
    pub mr: u8,
    pub recipient: Address,
    pub scts: Option<Timestamp>,
    pub scts_raw: [u8; 7],
    pub discharge: Option<Timestamp>,
    pub discharge_raw: [u8; 7],
    /// TP-ST (23.040 §9.2.3.15): 0x00–0x1F delivered, 0x20–0x3F still trying,
    /// 0x40– failed.
    pub status: u8,
    /// TP-PI, when present.
    pub pi: Option<u8>,
    pub pid: Option<u8>,
    pub dcs: Option<Dcs>,
    pub user_data: Option<UserData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tpdu {
    Deliver(Deliver),
    Submit(Submit),
    StatusReport(StatusReport),
}

impl Tpdu {
    pub fn user_data(&self) -> Option<&UserData> {
        match self {
            Self::Deliver(d) => Some(&d.user_data),
            Self::Submit(s) => Some(&s.user_data),
            Self::StatusReport(r) => r.user_data.as_ref(),
        }
    }

    pub fn text(&self) -> Option<&str> {
        self.user_data()?.text.as_deref()
    }

    /// Originator (DELIVER), destination (SUBMIT) or recipient (STATUS-REPORT).
    pub fn address(&self) -> &Address {
        match self {
            Self::Deliver(d) => &d.originator,
            Self::Submit(s) => &s.destination,
            Self::StatusReport(r) => &r.recipient,
        }
    }

    pub fn dcs(&self) -> Option<&Dcs> {
        match self {
            Self::Deliver(d) => Some(&d.dcs),
            Self::Submit(s) => Some(&s.dcs),
            Self::StatusReport(r) => r.dcs.as_ref(),
        }
    }
}

/// A 27.005 PDU: optional SMSC address followed by the TPDU.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pdu {
    pub smsc: Option<Address>,
    pub tpdu: Tpdu,
}

/// Decode a hex PDU as printed by `+CMGR`/`+CMGL`/`+CMT`/`+CDS` (SMSC prefix included).
pub fn decode_hex(hex: &str) -> Result<Pdu> {
    decode(&hex_decode(hex).ok_or(PduError::InvalidHex)?)
}

/// Decode PDU octets with the SMSC prefix. TP-MTI 00 is read as SMS-DELIVER, 01 as
/// SMS-SUBMIT, 10 as SMS-STATUS-REPORT — the directions a modem hands to the host.
pub fn decode(bytes: &[u8]) -> Result<Pdu> {
    let (smsc, n) = decode_smsc(bytes)?;
    Ok(Pdu {
        smsc,
        tpdu: decode_tpdu(&bytes[n..])?,
    })
}

/// Decode a TPDU without the SMSC prefix.
pub fn decode_tpdu(bytes: &[u8]) -> Result<Tpdu> {
    let mut r = Reader::new(bytes);
    let fo = r.u8("first octet")?;
    let bit = |n: u8| fo & (1 << n) != 0;
    match fo & 3 {
        0 => {
            let originator = r.address()?;
            let pid = r.u8("TP-PID")?;
            let dcs = Dcs::decode(r.u8("TP-DCS")?);
            let scts_raw = r.array("TP-SCTS")?;
            let user_data = r.user_data(bit(6), &dcs)?;
            Ok(Tpdu::Deliver(Deliver {
                mms: bit(2),
                lp: bit(3),
                sri: bit(5),
                udhi: bit(6),
                rp: bit(7),
                originator,
                pid,
                dcs,
                scts: Timestamp::decode(&scts_raw),
                scts_raw,
                user_data,
            }))
        }
        1 => {
            let mr = r.u8("TP-MR")?;
            let destination = r.address()?;
            let pid = r.u8("TP-PID")?;
            let dcs = Dcs::decode(r.u8("TP-DCS")?);
            let validity = match (fo >> 3) & 3 {
                0 => None,
                2 => Some(Validity::Relative(r.u8("TP-VP")?)),
                3 => Some(Validity::Absolute(
                    Timestamp::decode(&r.array("TP-VP")?).ok_or(PduError::Invalid("TP-VP"))?,
                )),
                _ => Some(Validity::Enhanced(r.array("TP-VP")?)),
            };
            let user_data = r.user_data(bit(6), &dcs)?;
            Ok(Tpdu::Submit(Submit {
                rd: bit(2),
                srr: bit(5),
                udhi: bit(6),
                rp: bit(7),
                mr,
                destination,
                pid,
                dcs,
                validity,
                user_data,
            }))
        }
        2 => {
            let mr = r.u8("TP-MR")?;
            let recipient = r.address()?;
            let scts_raw = r.array("TP-SCTS")?;
            let discharge_raw = r.array("TP-DT")?;
            let status = r.u8("TP-ST")?;
            let mut rep = StatusReport {
                mms: bit(2),
                lp: bit(3),
                srq: bit(5),
                udhi: bit(6),
                mr,
                recipient,
                scts: Timestamp::decode(&scts_raw),
                scts_raw,
                discharge: Timestamp::decode(&discharge_raw),
                discharge_raw,
                status,
                pi: None,
                pid: None,
                dcs: None,
                user_data: None,
            };
            // TP-PI and what it announces are optional (23.040 §9.2.2.3) and a receiver
            // ignores what it does not understand (§9.2.3.27): a truncated or malformed
            // tail leaves the fields it could not read as `None`, the report stands.
            let mut tail = || -> Result<()> {
                let pi = r.u8("TP-PI")?;
                rep.pi = Some(pi);
                let mut ext = pi;
                while ext & 0x80 != 0 {
                    ext = r.u8("TP-PI")?;
                }
                if pi & 1 != 0 {
                    rep.pid = Some(r.u8("TP-PID")?);
                }
                if pi & 2 != 0 {
                    rep.dcs = Some(Dcs::decode(r.u8("TP-DCS")?));
                }
                if pi & 4 != 0 {
                    let dcs = rep.dcs.unwrap_or(Dcs::decode(0));
                    rep.user_data = Some(r.user_data(bit(6), &dcs)?);
                }
                Ok(())
            };
            let _ = tail();
            Ok(Tpdu::StatusReport(rep))
        }
        _ => Err(PduError::ReservedMessageType),
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, pos: 0 }
    }

    fn u8(&mut self, field: &'static str) -> Result<u8> {
        Ok(self.take(1, field)?[0])
    }

    fn take(&mut self, n: usize, field: &'static str) -> Result<&'a [u8]> {
        let s = self
            .b
            .get(self.pos..self.pos + n)
            .ok_or(PduError::Truncated(field))?;
        self.pos += n;
        Ok(s)
    }

    fn array(&mut self, field: &'static str) -> Result<[u8; 7]> {
        let mut a = [0; 7];
        a.copy_from_slice(self.take(7, field)?);
        Ok(a)
    }

    fn address(&mut self) -> Result<Address> {
        let (a, n) = decode_address(&self.b[self.pos..])?;
        self.pos += n;
        Ok(a)
    }

    fn user_data(&mut self, udhi: bool, dcs: &Dcs) -> Result<UserData> {
        let udl = self.u8("TP-UDL")?;
        decode_user_data(udl, &self.b[self.pos..], udhi, dcs)
    }
}
