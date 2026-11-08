//! UCS2 user data (23.038 §6.2.3). Encoded as UTF-16BE, so characters outside the BMP
//! travel as surrogate pairs, as every current handset expects.

/// Text → UTF-16BE bytes.
pub fn encode(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_be_bytes).collect()
}

/// Number of UTF-16 code units (2 octets each) for `c`.
pub fn units(c: char) -> usize {
    c.len_utf16()
}

/// UTF-16BE bytes → text. Unpaired surrogates and a trailing odd octet become U+FFFD.
pub fn decode(bytes: &[u8]) -> String {
    let mut out: String = char::decode_utf16(code_units(bytes))
        .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect();
    if bytes.len() % 2 == 1 {
        out.push(char::REPLACEMENT_CHARACTER);
    }
    out
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ucs2Error {
    /// Odd number of octets.
    OddLength,
    /// Unpaired surrogate at this code-unit index.
    UnpairedSurrogate(usize),
}

impl std::fmt::Display for Ucs2Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OddLength => f.write_str("UCS2 data has an odd number of octets"),
            Self::UnpairedSurrogate(i) => write!(f, "unpaired UTF-16 surrogate at unit {i}"),
        }
    }
}

impl std::error::Error for Ucs2Error {}

/// Strict variant of [`decode`].
pub fn decode_strict(bytes: &[u8]) -> Result<String, Ucs2Error> {
    if bytes.len() % 2 == 1 {
        return Err(Ucs2Error::OddLength);
    }
    let mut out = String::with_capacity(bytes.len() / 2);
    let mut idx = 0;
    for r in char::decode_utf16(code_units(bytes)) {
        let c = r.map_err(|_| Ucs2Error::UnpairedSurrogate(idx))?;
        idx += c.len_utf16();
        out.push(c);
    }
    Ok(out)
}

fn code_units(bytes: &[u8]) -> impl Iterator<Item = u16> + '_ {
    bytes
        .chunks_exact(2)
        .map(|p| u16::from_be_bytes([p[0], p[1]]))
}
