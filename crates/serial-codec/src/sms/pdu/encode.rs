//! SMS-SUBMIT encoder with automatic alphabet choice and concatenation.

use super::{
    Alphabet, Dcs, Ie, PduError, Result, Udh, Validity, IEI_CONCAT_16, IEI_CONCAT_8,
    IEI_LOCKING_SHIFT, IEI_SINGLE_SHIFT, TOA_ALPHANUMERIC, TOA_INTERNATIONAL, TOA_UNKNOWN,
};
use crate::sms::gsm7::{self, Language, Tables};
use crate::sms::{hex_encode, ucs2};

/// Message body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Body {
    Text(String),
    /// 8-bit data (DCS 0x04), split by octets.
    Binary(Vec<u8>),
}

impl Default for Body {
    fn default() -> Self {
        Body::Text(String::new())
    }
}

/// Everything an SMS-SUBMIT needs. `SubmitRequest::new(to, text)` gives the usual
/// defaults: no SMSC (modem's), auto alphabet, MR 0, no VP, no status report, no
/// concatenation reference (set one for text that may not fit one part).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SubmitRequest {
    /// SMSC address; `None` or empty → `00` (use the one stored in the modem).
    pub smsc: Option<String>,
    /// TOA for the SMSC; default `0x91` with a leading `+`, else `0x81`.
    pub smsc_toa: Option<u8>,
    /// `+` + digits (international), digits / `*` / `#` (unknown TON) or text
    /// (alphanumeric, up to 11 GSM 7-bit characters). Set `destination_toa` to send the
    /// semi-octets `a b c` as digits.
    pub destination: String,
    /// TOA override for the destination (e.g. [`super::TOA_NATIONAL`]). TON 5 forces
    /// alphanumeric encoding.
    pub destination_toa: Option<u8>,
    pub body: Body,
    /// Force an alphabet; `None` picks GSM 7-bit, then national tables from
    /// `locking_shift`/`single_shift` (if that avoids UCS2), then UCS2.
    pub alphabet: Option<Alphabet>,
    /// National locking shift tables the recipient supports (23.038 §6.2.1.2.4).
    pub locking_shift: Vec<Language>,
    /// National single shift tables the recipient supports.
    pub single_shift: Vec<Language>,
    pub class: Option<u8>,
    pub pid: u8,
    pub message_ref: u8,
    pub status_report: bool,
    pub reject_duplicates: bool,
    pub reply_path: bool,
    pub validity: Option<Validity>,
    /// Concatenation reference (23.040 §9.2.3.24.1), required when the message needs
    /// more than one part: [`encode_submit`] returns [`PduError::MissingConcatRef`]
    /// without it. It must differ between long messages to the same recipient that may
    /// be in flight together, or the receiver can merge their parts; senders keep a
    /// counter (modulo 256, or 65536 with `concat_16bit`) per recipient or globally.
    /// Values above 255 imply a 16-bit reference IE.
    pub concat_ref: Option<u16>,
    pub concat_16bit: bool,
}

impl SubmitRequest {
    pub fn new(destination: impl Into<String>, text: impl Into<String>) -> Self {
        SubmitRequest {
            destination: destination.into(),
            body: Body::Text(text.into()),
            ..Default::default()
        }
    }
}

/// Encode a TP address field (length counts digits, not `+`). An empty value (or a
/// lone `+`) is `Invalid("address")`.
pub fn encode_address(value: &str, toa: Option<u8>) -> Result<Vec<u8>> {
    let digits = value.strip_prefix('+');
    if digits.unwrap_or(value).is_empty() {
        return Err(PduError::Invalid("address"));
    }
    let toa = toa.unwrap_or(match digits {
        Some(_) => TOA_INTERNATIONAL,
        None if !value.is_empty()
            && value
                .bytes()
                .all(|b| b.is_ascii_digit() || b == b'*' || b == b'#') =>
        {
            TOA_UNKNOWN
        }
        None => TOA_ALPHANUMERIC,
    });
    if super::Ton::from_toa(toa) == super::Ton::Alphanumeric {
        let septets = gsm7::encode(value, Tables::DEFAULT).map_err(PduError::Unencodable)?;
        let semi = (septets.len() * 7).div_ceil(4);
        if semi > 20 {
            return Err(PduError::Invalid("address length"));
        }
        let mut out = vec![semi as u8, toa];
        out.extend(gsm7::pack(&septets, 0));
        return Ok(out);
    }
    let digits = digits.unwrap_or(value);
    if digits.chars().count() > 20 {
        return Err(PduError::Invalid("address length"));
    }
    let body = super::encode_semi_octets(digits).map_err(PduError::Unencodable)?;
    let mut out = vec![digits.len() as u8, toa];
    out.extend(body);
    Ok(out)
}

/// Encode the SMSC prefix (length counts TOA + octets); `None`/empty → `[0]`.
pub fn encode_smsc(value: Option<&str>, toa: Option<u8>) -> Result<Vec<u8>> {
    let Some(v) = value.filter(|v| !v.is_empty()) else {
        return Ok(vec![0]);
    };
    let digits = v.strip_prefix('+');
    let toa = toa.unwrap_or(if digits.is_some() {
        TOA_INTERNATIONAL
    } else {
        TOA_UNKNOWN
    });
    let digits = digits.unwrap_or(v);
    if digits.chars().count() > 20 {
        return Err(PduError::Invalid("SMSC length"));
    }
    let body = super::encode_semi_octets(digits).map_err(PduError::Unencodable)?;
    let mut out = vec![body.len() as u8 + 1, toa];
    out.extend(body);
    Ok(out)
}

/// One encodable unit: a character (1–2 septets / 2–4 UCS2 octets) or a data octet.
/// Parts are split between units only, so ESC pairs and surrogate pairs stay whole.
struct Units {
    alphabet: Alphabet,
    tables: Tables,
    units: Vec<Vec<u8>>,
}

fn units(req: &SubmitRequest) -> Result<Units> {
    let text = match &req.body {
        Body::Binary(b) => {
            if req.alphabet.is_some_and(|a| a != Alphabet::Bit8) {
                return Err(PduError::Invalid("alphabet for binary body"));
            }
            return Ok(Units {
                alphabet: Alphabet::Bit8,
                tables: Tables::DEFAULT,
                units: b.iter().map(|&x| vec![x]).collect(),
            });
        }
        Body::Text(t) => t,
    };
    let gsm = |tables: Tables| -> Result<Units> {
        let units = text
            .chars()
            .map(|c| {
                gsm7::encode_char(c, tables)
                    .map(|(s, n)| s[..n].to_vec())
                    .ok_or(PduError::Unencodable(c))
            })
            .collect::<Result<_>>()?;
        Ok(Units {
            alphabet: Alphabet::Gsm7,
            tables,
            units,
        })
    };
    let ucs = || Units {
        alphabet: Alphabet::Ucs2,
        tables: Tables::DEFAULT,
        units: text
            .chars()
            .map(|c| ucs2::encode(c.encode_utf8(&mut [0; 4])))
            .collect(),
    };
    match req.alphabet {
        Some(Alphabet::Gsm7) => {
            let tables = gsm7::choose_tables(text, &req.locking_shift, &req.single_shift)
                .ok_or_else(|| first_unencodable(text))?;
            gsm(tables)
        }
        Some(Alphabet::Ucs2) => Ok(ucs()),
        Some(Alphabet::Bit8) => Err(PduError::Invalid("alphabet for text body")),
        None => Ok(
            match gsm7::choose_tables(text, &req.locking_shift, &req.single_shift) {
                Some(t) => gsm(t)?,
                None => ucs(),
            },
        ),
    }
}

/// Error for a forced GSM 7-bit text no table pair can encode: the first character
/// outside the default tables (or `Invalid` when the conflict is between tables).
fn first_unencodable(text: &str) -> PduError {
    gsm7::encode(text, Tables::DEFAULT)
        .err()
        .map_or(PduError::Invalid("text"), PduError::Unencodable)
}

/// Capacity of one part in units of the alphabet's length (septets or octets) for a
/// UDH of `udh_len` octets (UDHL included; 0 = no UDH).
fn capacity(alphabet: Alphabet, udh_len: usize) -> usize {
    match alphabet {
        Alphabet::Gsm7 => 160 - (udh_len * 8).div_ceil(7),
        _ => 140 - udh_len,
    }
}

/// Encode an SMS-SUBMIT, splitting into concatenated parts when needed (which requires
/// [`SubmitRequest::concat_ref`]).
///
/// Returns `(tpdu_len, hex)` per part: `tpdu_len` is the `<length>` for `AT+CMGS`
/// (TPDU octets, SMSC prefix excluded); `hex` is the full PDU to send after the prompt.
pub fn encode_submit(req: &SubmitRequest) -> Result<Vec<(usize, String)>> {
    let u = units(req)?;
    let smsc = encode_smsc(req.smsc.as_deref(), req.smsc_toa)?;
    let da = encode_address(&req.destination, req.destination_toa)?;

    let mut national = Vec::new();
    if u.tables.single != Language::Default {
        national.push(Ie {
            id: IEI_SINGLE_SHIFT,
            data: vec![u.tables.single.nli()],
        });
    }
    if u.tables.locking != Language::Default {
        national.push(Ie {
            id: IEI_LOCKING_SHIFT,
            data: vec![u.tables.locking.nli()],
        });
    }
    let national_len = national.iter().map(|ie| 2 + ie.data.len()).sum::<usize>();

    let total: usize = u.units.iter().map(Vec::len).sum();
    let single_udh = if national.is_empty() {
        0
    } else {
        1 + national_len
    };
    let concat_ref = req.concat_ref.unwrap_or(0);
    let wide = req.concat_16bit || concat_ref > 255;
    let parts: Vec<&[Vec<u8>]> = if total <= capacity(u.alphabet, single_udh) {
        vec![&u.units[..]]
    } else {
        let cap = capacity(u.alphabet, 1 + national_len + if wide { 6 } else { 5 });
        let mut parts = Vec::new();
        let (mut start, mut used) = (0, 0);
        for (i, unit) in u.units.iter().enumerate() {
            if used + unit.len() > cap {
                parts.push(&u.units[start..i]);
                (start, used) = (i, 0);
            }
            used += unit.len();
        }
        parts.push(&u.units[start..]);
        parts
    };
    if parts.len() > 255 {
        return Err(PduError::TooManyParts);
    }
    if parts.len() > 1 && req.concat_ref.is_none() {
        return Err(PduError::MissingConcatRef);
    }

    let (vpf, vp) = match &req.validity {
        None => (0, Vec::new()),
        Some(v @ Validity::Relative(r)) => (v.vpf(), vec![*r]),
        Some(v @ Validity::Absolute(t)) => (
            v.vpf(),
            t.encode().ok_or(PduError::Invalid("TP-VP"))?.to_vec(),
        ),
        Some(v @ Validity::Enhanced(b)) => (v.vpf(), b.to_vec()),
    };
    let dcs = Dcs::encode(u.alphabet, req.class);
    let n = parts.len();
    let mut out = Vec::with_capacity(n);
    for (i, part) in parts.iter().enumerate() {
        let mut ies = Vec::new();
        if n > 1 {
            let (seq, total) = ((i + 1) as u8, n as u8);
            ies.push(if wide {
                let r = concat_ref.to_be_bytes();
                Ie {
                    id: IEI_CONCAT_16,
                    data: vec![r[0], r[1], total, seq],
                }
            } else {
                Ie {
                    id: IEI_CONCAT_8,
                    data: vec![concat_ref as u8, total, seq],
                }
            });
        }
        ies.extend(national.iter().cloned());
        let udh = (!ies.is_empty())
            .then(|| Udh { ies }.encode())
            .transpose()?;
        let udh_len = udh.as_ref().map_or(0, Vec::len);
        let body: Vec<u8> = part.concat();
        let (udl, packed) = match u.alphabet {
            Alphabet::Gsm7 => {
                let hdr_septets = (udh_len * 8).div_ceil(7);
                let fill = (hdr_septets * 7 - udh_len * 8) as u8;
                (hdr_septets + body.len(), gsm7::pack(&body, fill))
            }
            _ => (udh_len + body.len(), body),
        };
        let fo = 0x01
            | u8::from(req.reject_duplicates) << 2
            | vpf << 3
            | u8::from(req.status_report) << 5
            | u8::from(udh.is_some()) << 6
            | u8::from(req.reply_path) << 7;
        let mut tpdu = vec![fo, req.message_ref];
        tpdu.extend_from_slice(&da);
        tpdu.extend_from_slice(&[req.pid, dcs]);
        tpdu.extend_from_slice(&vp);
        tpdu.push(udl as u8);
        tpdu.extend(udh.unwrap_or_default());
        tpdu.extend(packed);
        let mut pdu = smsc.clone();
        pdu.extend_from_slice(&tpdu);
        out.push((tpdu.len(), hex_encode(&pdu)));
    }
    Ok(out)
}
