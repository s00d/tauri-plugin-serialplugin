//! Argument splitting for one AT response / URC line (V.250 §5.4.2, 3GPP TS 27.007 §4).
//!
//! [`split`] cuts a parameter list on top-level commas. Commas inside a quoted string or inside
//! parentheses (`(0-5)`, `(2,"Op","Op","00101",7)`) do not split. Each [`Arg`] keeps whether it was
//! quoted, so `"0001"` (a string) and `0001` (a number) stay distinguishable.
//!
//! ```
//! use serial_codec::args::{split, strip_prefix};
//!
//! let body = strip_prefix("+CMGL: 1,\"REC READ\",\"a,b\",,", "+CMGL").unwrap();
//! let a = split(body);
//! assert_eq!(a.len(), 5);
//! assert_eq!(a[0].int::<u32>(), Some(1));
//! assert_eq!(a[2].str(), Some("a,b"));
//! assert!(a[3].is_empty() && a[4].is_empty());
//! ```

use std::ops::RangeInclusive;
use std::str::FromStr;

/// One comma-separated parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Arg {
    /// Unquoted, unescaped text for a quoted string; trimmed raw text otherwise
    /// (parentheses kept for groups).
    pub text: String,
    /// The parameter was a `"..."` string.
    pub quoted: bool,
}

impl Arg {
    /// Omitted parameter (`,,`), not an empty quoted string `""`.
    pub fn is_empty(&self) -> bool {
        !self.quoted && self.text.is_empty()
    }

    /// Unquoted decimal integer (leading zeros allowed). Quoted digits are accepted too, as many
    /// modems quote numeric values.
    pub fn int<T: FromStr>(&self) -> Option<T> {
        if self.text.is_empty() || !self.text.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        self.text.parse().ok()
    }

    /// Hexadecimal value, quoted or not (`"8BE3"`, `00D30156`, `0x1F`).
    pub fn hex(&self) -> Option<u64> {
        let t = self.text.trim_start_matches("0x").trim_start_matches("0X");
        if t.is_empty() || t.len() > 16 {
            return None;
        }
        u64::from_str_radix(t, 16).ok()
    }

    /// Quoted string value; `None` for an omitted or unquoted parameter.
    pub fn str(&self) -> Option<&str> {
        self.quoted.then_some(self.text.as_str())
    }

    /// String value, quoted or not; `None` when omitted.
    pub fn text(&self) -> Option<&str> {
        (!self.is_empty()).then_some(self.text.as_str())
    }

    /// Contents of a parenthesised group split into arguments: `("SM","ME")` -> `SM`, `ME`.
    /// `None` when the argument is not a group.
    pub fn group(&self) -> Option<Vec<Arg>> {
        if self.quoted {
            return None;
        }
        let inner = self.text.strip_prefix('(')?;
        let inner = inner.strip_suffix(')').unwrap_or(inner);
        Some(split(inner))
    }
}

/// Splits a parameter list on top-level commas.
///
/// Whitespace around parameters is dropped. Inside quotes `\HH` is the character with that hex
/// code (V.250 §5.4.2.2, so a quote is `\22` and a backslash `\5C`); any other backslash is
/// plain text and a quote always closes the string (`"C:\",1` is two parameters). An
/// unterminated string runs to the end of the line. An empty input yields no parameters; `,` yields two empty ones.
pub fn split(s: &str) -> Vec<Arg> {
    let s = s.trim();
    if s.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    let mut in_quote = false;
    let mut depth = 0usize;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quote {
            match c {
                '"' => {
                    in_quote = false;
                    if depth > 0 {
                        cur.push('"');
                    }
                }
                '\\' => match chars.peek().copied() {
                    Some(_) => {
                        let mut look = chars.clone();
                        let hex: String = look.by_ref().take(2).collect();
                        match (depth, u8::from_str_radix(&hex, 16)) {
                            (0, Ok(b))
                                if hex.len() == 2 && hex.bytes().all(|b| b.is_ascii_hexdigit()) =>
                            {
                                cur.push(b as char);
                                chars = look;
                            }
                            _ => cur.push('\\'),
                        }
                    }
                    None => cur.push('\\'),
                },
                _ => cur.push(c),
            }
            continue;
        }
        match c {
            '"' => {
                in_quote = true;
                if depth > 0 {
                    cur.push('"');
                } else {
                    quoted = true;
                    // Text before the opening quote is junk (` "x"`); only spaces are expected.
                    cur.retain(|c| !c.is_whitespace());
                }
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            ',' if depth == 0 => {
                out.push(finish(&mut cur, quoted));
                quoted = false;
            }
            _ if quoted && depth == 0 => {} // junk after the closing quote (`"x" `)
            _ => cur.push(c),
        }
    }
    out.push(finish(&mut cur, quoted));
    out
}

fn finish(cur: &mut String, quoted: bool) -> Arg {
    let text = std::mem::take(cur);
    Arg {
        text: if quoted {
            text
        } else {
            text.trim().to_string()
        },
        quoted,
    }
}

/// Returns the text after `<prefix>:` when `line` starts with `prefix` (ASCII case-insensitive).
/// Surrounding whitespace / CR LF and spaces before or after the colon are allowed.
/// `strip_prefix("+CREG: 1", "+CREG")` is `Some("1")`; `"+CREGX: 1"` does not match.
pub fn strip_prefix<'a>(line: &'a str, prefix: &str) -> Option<&'a str> {
    let line = line.trim();
    let head = line.get(..prefix.len())?;
    if !head.eq_ignore_ascii_case(prefix) {
        return None;
    }
    let rest = line[prefix.len()..].trim_start().strip_prefix(':')?;
    Some(rest.trim())
}

/// Parses a 27.007 range list such as `0-1,3` or `(0-5)` into inclusive ranges.
/// `None` on anything that is not digits, `-` and `,`.
pub fn parse_ranges(s: &str) -> Option<Vec<RangeInclusive<u32>>> {
    let s = s.trim();
    let s = s.strip_prefix('(').unwrap_or(s);
    let s = s.strip_suffix(')').unwrap_or(s).trim();
    if s.is_empty() {
        return Some(Vec::new());
    }
    s.split(',')
        .map(|part| {
            let part = part.trim();
            let (a, b) = part.split_once('-').unwrap_or((part, part));
            let a: u32 = a.trim().parse().ok()?;
            let b: u32 = b.trim().parse().ok()?;
            (a <= b).then_some(a..=b)
        })
        .collect()
}

/// [`parse_ranges`] expanded to the listed values; ranges wider than 1024 values are refused
/// (`None`) so a hostile `(0-4294967295)` cannot allocate gigabytes.
pub fn parse_values(s: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for r in parse_ranges(s)? {
        if r.end() - r.start() >= 1024 {
            return None;
        }
        out.extend(r);
    }
    Some(out)
}
