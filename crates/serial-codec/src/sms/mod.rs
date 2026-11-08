//! SMS codecs: 3GPP TS 23.038 alphabets and TS 23.040 TPDUs as used by
//! TS 27.005 PDU mode (`AT+CMGF=0`).
//!
//! - [`gsm7`]: default alphabet, extension table, national language tables, septet packing.
//! - [`ucs2`]: UTF-16BE user data, surrogate pairs included.
//! - [`pdu`]: decode SMS-DELIVER / SMS-SUBMIT / SMS-STATUS-REPORT, encode SMS-SUBMIT
//!   (with automatic concatenation).
//! - [`concat`]: multipart reassembly keyed by (type, address, reference, total).
//!
//! All decoders are byte based and return errors for malformed input; none panic.

pub mod concat;
pub mod gsm7;
pub mod pdu;
pub mod ucs2;

/// Hex string → bytes. Accepts either case and surrounding whitespace; `None` for odd
/// length or a non-hex character.
pub fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim().as_bytes();
    if s.len() % 2 != 0 {
        return None;
    }
    s.chunks_exact(2)
        .map(|p| Some((hex_val(p[0])? << 4) | hex_val(p[1])?))
        .collect()
}

/// Bytes → uppercase hex.
pub fn hex_encode(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789ABCDEF";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(char::from(D[usize::from(b >> 4)]));
        s.push(char::from(D[usize::from(b & 15)]));
    }
    s
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}
