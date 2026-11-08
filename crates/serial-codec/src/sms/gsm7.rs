//! GSM 7-bit default alphabet, extension table and national language tables
//! (3GPP TS 23.038 §6.2.1 and Annex A), plus septet packing (§6.1.2.1).
//!
//! Septets are handled unpacked (one `u8` per septet, values 0..=0x7F) by
//! [`encode`]/[`decode`] and bit-packed by [`pack`]/[`unpack`].
//!
//! National tables covered: Turkish (locking + single shift), Spanish (single shift;
//! 23.038 defines no Spanish locking table), Portuguese (locking + single shift).
// ponytail: Indic tables (NLI 4–13: Bengali … Urdu) are not included; add them from
// 23.038 Annex A.2.4+/A.3.4+ when a caller needs them. Unknown NLIs decode with the
// default table (see `Language::from_nli`).

use serde::{Deserialize, Serialize};

/// Escape to the single shift (extension) table.
pub const ESC: u8 = 0x1B;

/// National language identifier (23.038 §6.2.1.2.4), limited to the tables this
/// module carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    Default,
    Turkish,
    Spanish,
    Portuguese,
}

impl Language {
    /// Map a UDH national language identifier (IEI 0x24/0x25 value). `None` for
    /// identifiers whose tables are not carried here.
    pub fn from_nli(nli: u8) -> Option<Self> {
        match nli {
            0 => Some(Self::Default),
            1 => Some(Self::Turkish),
            2 => Some(Self::Spanish),
            3 => Some(Self::Portuguese),
            _ => None,
        }
    }

    pub fn nli(self) -> u8 {
        match self {
            Self::Default => 0,
            Self::Turkish => 1,
            Self::Spanish => 2,
            Self::Portuguese => 3,
        }
    }

    /// Whether 23.038 defines a locking shift table for this language.
    pub fn has_locking_table(self) -> bool {
        !matches!(self, Self::Spanish)
    }

    fn locking(self) -> &'static [char; 128] {
        match self {
            Self::Turkish => &TURKISH,
            Self::Portuguese => &PORTUGUESE,
            Self::Default | Self::Spanish => &DEFAULT,
        }
    }

    fn single(self) -> &'static [(u8, char)] {
        match self {
            Self::Default => DEFAULT_EXT,
            Self::Turkish => TURKISH_EXT,
            Self::Spanish => SPANISH_EXT,
            Self::Portuguese => PORTUGUESE_EXT,
        }
    }
}

/// Locking shift table plus single shift table used for one message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Tables {
    pub locking: Language,
    pub single: Language,
}

impl Tables {
    pub const DEFAULT: Tables = Tables {
        locking: Language::Default,
        single: Language::Default,
    };

    pub fn new(locking: Language, single: Language) -> Self {
        Tables { locking, single }
    }

    /// Number of UDH octets the national language IEs add (3 per non-default table).
    pub fn udh_ie_len(self) -> usize {
        3 * (usize::from(self.locking != Language::Default)
            + usize::from(self.single != Language::Default))
    }
}

// 0x1B is ESC; the slot holds U+00A0 only so the array is complete. It is never
// produced by `decode` and never matched by `encode`.
const DEFAULT: [char; 128] = [
    '@', '£', '$', '¥', 'è', 'é', 'ù', 'ì', 'ò', 'Ç', '\n', 'Ø', 'ø', '\r', 'Å', 'å', //
    'Δ', '_', 'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{A0}', 'Æ', 'æ', 'ß', 'É', //
    ' ', '!', '"', '#', '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
    '¡', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö', 'Ñ', 'Ü', '§', //
    '¿', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à', //
];

// 23.038 A.3.1 Turkish locking shift table.
const TURKISH: [char; 128] = [
    '@', '£', '$', '¥', '€', 'é', 'ù', 'ı', 'ò', 'Ç', '\n', 'Ğ', 'ğ', '\r', 'Å', 'å', //
    'Δ', '_', 'Φ', 'Γ', 'Λ', 'Ω', 'Π', 'Ψ', 'Σ', 'Θ', 'Ξ', '\u{A0}', 'Ş', 'ş', 'ß', 'É', //
    ' ', '!', '"', '#', '¤', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
    'İ', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ä', 'Ö', 'Ñ', 'Ü', '§', //
    'ç', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ä', 'ö', 'ñ', 'ü', 'à', //
];

// 23.038 A.3.3 Portuguese locking shift table.
const PORTUGUESE: [char; 128] = [
    '@', '£', '$', '¥', 'ê', 'é', 'ú', 'í', 'ó', 'ç', '\n', 'Ô', 'ô', '\r', 'Á', 'á', //
    'Δ', '_', 'ª', 'Ç', 'À', '∞', '^', '\\', '€', 'Ó', '|', '\u{A0}', 'Â', 'â', 'Ê', 'É', //
    ' ', '!', '"', '#', 'º', '%', '&', '\'', '(', ')', '*', '+', ',', '-', '.', '/', //
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', ';', '<', '=', '>', '?', //
    'Í', 'A', 'B', 'C', 'D', 'E', 'F', 'G', 'H', 'I', 'J', 'K', 'L', 'M', 'N', 'O', //
    'P', 'Q', 'R', 'S', 'T', 'U', 'V', 'W', 'X', 'Y', 'Z', 'Ã', 'Õ', 'Ú', 'Ü', '§', //
    '~', 'a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j', 'k', 'l', 'm', 'n', 'o', //
    'p', 'q', 'r', 's', 't', 'u', 'v', 'w', 'x', 'y', 'z', 'ã', 'õ', '`', 'ü', 'à', //
];

// 23.038 §6.2.1.1 default extension table.
const DEFAULT_EXT: &[(u8, char)] = &[
    (0x0A, '\u{C}'),
    (0x14, '^'),
    (0x28, '{'),
    (0x29, '}'),
    (0x2F, '\\'),
    (0x3C, '['),
    (0x3D, '~'),
    (0x3E, ']'),
    (0x40, '|'),
    (0x65, '€'),
];

// 23.038 A.2.1 Turkish single shift table.
const TURKISH_EXT: &[(u8, char)] = &[
    (0x0A, '\u{C}'),
    (0x14, '^'),
    (0x28, '{'),
    (0x29, '}'),
    (0x2F, '\\'),
    (0x3C, '['),
    (0x3D, '~'),
    (0x3E, ']'),
    (0x40, '|'),
    (0x47, 'Ğ'),
    (0x49, 'İ'),
    (0x53, 'Ş'),
    (0x63, 'ç'),
    (0x65, '€'),
    (0x67, 'ğ'),
    (0x69, 'ı'),
    (0x73, 'ş'),
];

// 23.038 A.2.2 Spanish single shift table.
const SPANISH_EXT: &[(u8, char)] = &[
    (0x09, 'ç'),
    (0x0A, '\u{C}'),
    (0x14, '^'),
    (0x28, '{'),
    (0x29, '}'),
    (0x2F, '\\'),
    (0x3C, '['),
    (0x3D, '~'),
    (0x3E, ']'),
    (0x40, '|'),
    (0x41, 'Á'),
    (0x49, 'Í'),
    (0x4F, 'Ó'),
    (0x55, 'Ú'),
    (0x61, 'á'),
    (0x65, '€'),
    (0x69, 'í'),
    (0x6F, 'ó'),
    (0x75, 'ú'),
];

// 23.038 A.2.3 Portuguese single shift table.
const PORTUGUESE_EXT: &[(u8, char)] = &[
    (0x05, 'ê'),
    (0x09, 'ç'),
    (0x0A, '\u{C}'),
    (0x0B, 'Ô'),
    (0x0C, 'ô'),
    (0x0E, 'Á'),
    (0x0F, 'á'),
    (0x12, 'Φ'),
    (0x13, 'Γ'),
    (0x14, '^'),
    (0x15, 'Ω'),
    (0x16, 'Π'),
    (0x17, 'Ψ'),
    (0x18, 'Σ'),
    (0x19, 'Θ'),
    (0x1F, 'Ê'),
    (0x28, '{'),
    (0x29, '}'),
    (0x2F, '\\'),
    (0x3C, '['),
    (0x3D, '~'),
    (0x3E, ']'),
    (0x40, '|'),
    (0x41, 'À'),
    (0x49, 'Í'),
    (0x4F, 'Ó'),
    (0x55, 'Ú'),
    (0x5B, 'Ã'),
    (0x5C, 'Õ'),
    (0x61, 'Â'),
    (0x65, '€'),
    (0x69, 'í'),
    (0x6F, 'ó'),
    (0x75, 'ú'),
    (0x7B, 'ã'),
    (0x7C, 'õ'),
    (0x7F, 'â'),
];

/// Septets for one character: `[code]` or `[ESC, code]`. `None` if the character is in
/// neither table.
pub fn encode_char(c: char, tables: Tables) -> Option<([u8; 2], usize)> {
    if let Some(i) = tables
        .locking
        .locking()
        .iter()
        .position(|&t| t == c)
        .filter(|&i| i != usize::from(ESC))
    {
        return Some(([i as u8, 0], 1));
    }
    tables
        .single
        .single()
        .iter()
        .find(|(_, t)| *t == c)
        .map(|&(code, _)| ([ESC, code], 2))
}

/// Text → unpacked septets. `Err(c)` names the first character with no mapping.
pub fn encode(text: &str, tables: Tables) -> Result<Vec<u8>, char> {
    let mut out = Vec::with_capacity(text.len());
    for c in text.chars() {
        let (s, n) = encode_char(c, tables).ok_or(c)?;
        out.extend_from_slice(&s[..n]);
    }
    Ok(out)
}

/// Septet count of `text`, or `None` if it is not encodable with `tables`.
pub fn septet_len(text: &str, tables: Tables) -> Option<usize> {
    text.chars()
        .map(|c| encode_char(c, tables).map(|(_, n)| n))
        .sum()
}

pub fn is_encodable(text: &str, tables: Tables) -> bool {
    septet_len(text, tables).is_some()
}

/// Cheapest table pair able to encode `text`: the default tables if they work,
/// otherwise the combination of the allowed `locking` and `single` shift tables (the
/// default table is always allowed) with the fewest septets plus UDH cost (3 octets per
/// national IE). `None` if nothing fits.
pub fn choose_tables(text: &str, locking: &[Language], single: &[Language]) -> Option<Tables> {
    if is_encodable(text, Tables::DEFAULT) {
        return Some(Tables::DEFAULT);
    }
    let with_default = |l: &[Language]| {
        let mut v = vec![Language::Default];
        v.extend(l.iter().copied().filter(|l| *l != Language::Default));
        v
    };
    let (locking, single) = (with_default(locking), with_default(single));
    let mut best: Option<(usize, Tables)> = None;
    for &l in locking.iter().filter(|l| l.has_locking_table()) {
        for &s in &single {
            let t = Tables::new(l, s);
            if let Some(n) = septet_len(text, t) {
                let cost = n + (t.udh_ie_len() * 8).div_ceil(7);
                if best.is_none_or(|(c, _)| cost < c) {
                    best = Some((cost, t));
                }
            }
        }
    }
    best.map(|(_, t)| t)
}

/// Unpacked septets → text (23.038 §6.2.1.1). Unknown escape sequences show the
/// locking-table character at that index; `ESC ESC` and a trailing lone `ESC` show a
/// space. Bit 7 of every input byte is ignored.
pub fn decode(septets: &[u8], tables: Tables) -> String {
    let locking = tables.locking.locking();
    let single = tables.single.single();
    let mut out = String::with_capacity(septets.len());
    let mut it = septets.iter().map(|b| b & 0x7F);
    while let Some(s) = it.next() {
        if s != ESC {
            out.push(locking[usize::from(s)]);
            continue;
        }
        match it.next() {
            None | Some(ESC) => out.push(' '),
            Some(n) => out.push(
                single
                    .iter()
                    .find(|(code, _)| *code == n)
                    .map_or(locking[usize::from(n)], |&(_, c)| c),
            ),
        }
    }
    out
}

/// Pack septets into octets, LSB first, after `fill_bits` zero bits (0..=7; larger
/// values are taken modulo 8). Used with `fill_bits` > 0 after a UDH (23.040 §9.2.3.24).
pub fn pack(septets: &[u8], fill_bits: u8) -> Vec<u8> {
    let fill = usize::from(fill_bits % 8);
    let total_bits = fill + septets.len() * 7;
    let mut out = vec![0u8; total_bits.div_ceil(8)];
    for (i, &s) in septets.iter().enumerate() {
        let bit = fill + i * 7;
        let v = u16::from(s & 0x7F) << (bit % 8);
        out[bit / 8] |= v as u8;
        if v > 0xFF {
            out[bit / 8 + 1] |= (v >> 8) as u8;
        }
    }
    out
}

/// Unpack up to `count` septets starting after `fill_bits` bits. Returns fewer when the
/// input runs out; never panics.
pub fn unpack(octets: &[u8], fill_bits: u8, count: usize) -> Vec<u8> {
    let fill = usize::from(fill_bits % 8);
    let avail = (octets.len() * 8).saturating_sub(fill) / 7;
    (0..count.min(avail))
        .map(|i| {
            let bit = fill + i * 7;
            let lo = u16::from(octets[bit / 8]);
            let hi = octets.get(bit / 8 + 1).copied().map_or(0, u16::from);
            (((hi << 8 | lo) >> (bit % 8)) & 0x7F) as u8
        })
        .collect()
}

/// Number of septets that fit in `octets` after `fill_bits` bits.
pub fn septets_in(octets: usize, fill_bits: u8) -> usize {
    (octets * 8).saturating_sub(usize::from(fill_bits % 8)) / 7
}
