//! Model: AT parameter lists (V.250 §5.4.2).
//!
//! proptest builds parameter lists field by field (bare values, strings with `\HH` escapes and
//! lone backslashes, groups with strings and nested groups inside, an unterminated string at
//! the end) and the model says what each field must read back as. Values and range lists are
//! drawn valid and malformed; the model says which ones are refused.

use crate::common::*;
use serial_codec::args::{parse_ranges, parse_values, split, strip_prefix, Arg};
use std::ops::RangeInclusive;

/// A field as written, the argument it is, and what `group()` must return.
#[derive(Debug, Clone)]
struct Field {
    wire: String,
    want: Arg,
    group: Option<Vec<Arg>>,
}

fn arg(text: &str, quoted: bool) -> Arg {
    Arg {
        text: text.into(),
        quoted,
    }
}

fn bare() -> impl Strategy<Value = Field> {
    "[A-Za-z0-9.+-]{0,10}".prop_map(|s| Field {
        want: arg(&s, false),
        wire: s,
        group: None,
    })
}

/// A piece of a string, as written and as read.
fn piece() -> impl Strategy<Value = (String, String)> {
    prop_oneof![
        4 => "[A-Za-z0-9 ,.+()-]{1,6}".prop_map(|s| (s.clone(), s)),
        // `\HH` is that character: a quote, a backslash, a control character, Latin-1.
        2 => (any::<u8>(), any::<bool>()).prop_map(|(b, upper)| {
            let hex = if upper { format!("\\{b:02X}") } else { format!("\\{b:02x}") };
            (hex, char::from(b).to_string())
        }),
        // Any other backslash is text.
        1 => select(vec!["\\x1", "\\:", "\\4g", "\\\\:"]).prop_map(|s| (s.into(), s.into())),
    ]
}

fn quoted() -> impl Strategy<Value = Field> {
    // A backslash right before the closing quote is text too: a quote always closes.
    (vec(piece(), 0..4), select(vec!["", "\\"])).prop_map(|(p, end)| {
        let (wire, text): (String, String) = p.into_iter().unzip();
        Field {
            wire: format!("\"{wire}{end}\""),
            want: arg(&format!("{text}{end}"), true),
            group: None,
        }
    })
}

/// `(…)` is kept whole as text and split by `group()`; commas and brackets inside its strings
/// and nested groups do not split it.
fn group() -> impl Strategy<Value = Field> {
    let item = prop_oneof![
        "[A-Za-z0-9.+-]{1,6}".prop_map(|s| (s.clone(), arg(&s, false))),
        "[A-Za-z0-9 ,()]{0,8}".prop_map(|s| (format!("\"{s}\""), arg(&s, true))),
        (0u8..9, 0u8..9).prop_map(|(a, b)| {
            let s = format!("({a}-{b},9)");
            (s.clone(), arg(&s, false))
        }),
    ];
    vec(item, 0..4).prop_map(|items| {
        let (wires, args): (Vec<String>, Vec<Arg>) = items.into_iter().unzip();
        let wire = format!("({})", wires.join(","));
        Field {
            want: arg(&wire, false),
            wire,
            group: Some(args),
        }
    })
}

/// A string whose quote never closes: it runs to the end of the line.
fn open() -> impl Strategy<Value = Field> {
    "[A-Za-z0-9 ,()]{0,8}[a-z]".prop_map(|s| Field {
        wire: format!("\"{s}"),
        want: arg(&s, true),
        group: None,
    })
}

proptest! {
    #![proptest_config(cases(512))]

    /// A written list splits back into exactly its fields.
    #[test]
    fn lists_split_into_their_fields(
        fields in vec(prop_oneof![3 => bare(), 3 => quoted(), 1 => group()], 0..8),
        last in proptest::option::of(open()),
        pad in "[ ]{0,2}",
    ) {
        let mut wires: Vec<String> = fields.iter().map(|f| format!("{pad}{}{pad}", f.wire)).collect();
        let mut want = fields;
        if let Some(f) = last {
            wires.push(format!("{pad}{}", f.wire));
            want.push(f);
        }
        let line = wires.join(",");
        let got = split(&line);
        if line.trim().is_empty() {
            prop_assert!(got.is_empty(), "{:?}", got);
            return Ok(());
        }
        let args: Vec<Arg> = want.iter().map(|f| f.want.clone()).collect();
        prop_assert_eq!(&got, &args, "{}", line);
        for (a, f) in got.iter().zip(&want) {
            let omitted = !a.quoted && a.text.is_empty();
            prop_assert_eq!(a.is_empty(), omitted);
            prop_assert_eq!(a.str(), a.quoted.then_some(a.text.as_str()));
            prop_assert_eq!(a.text(), (!omitted).then_some(a.text.as_str()));
            prop_assert_eq!(a.group(), f.group.clone(), "{}", f.wire);
        }
    }

    /// Decimal values must fit their type, hexadecimal ones have 1-16 digits (a 17th is refused
    /// even as a leading zero); both are read quoted or not.
    #[test]
    fn numbers_read_back(
        dec in "[0-9]{0,12}", hex in "[0-9a-fA-F]{0,18}", x in select(vec!["", "0x", "0X"]),
        junk in "[g-wyz+-]{1,3}", quote in any::<bool>(),
    ) {
        let one = |s: &str| {
            let a = split(&if quote { format!("\"{s}\"") } else { format!(" {s} ") });
            a.first().cloned().unwrap_or_else(|| arg("", false))
        };
        prop_assert_eq!(one(&dec).int::<u32>(), dec.parse().ok());
        prop_assert_eq!(one(&dec).int::<u8>(), dec.parse().ok());
        prop_assert_eq!(one(&format!("{dec}{junk}")).int::<u32>(), None);
        let want = if hex.len() > 16 { None } else { u64::from_str_radix(&hex, 16).ok() };
        prop_assert_eq!(one(&format!("{x}{hex}")).hex(), want);
        prop_assert_eq!(one(&format!("{hex}{junk}")).hex(), None);
    }

    /// Range lists, with or without brackets: values in order, `None` for a reversed range or
    /// junk, and no expansion of a range of 1024 values or more.
    #[test]
    fn ranges_expand(
        parts in vec(prop_oneof![
            6 => (0u32..50, prop_oneof![4 => 0u32..20, 1 => 1020u32..1028]).prop_map(Ok),
            1 => select(vec!["x", "1-", "-1", "5-3", "1-2-3"]).prop_map(Err),
        ], 0..4),
        brackets in any::<bool>(),
    ) {
        let text: Vec<String> = parts.iter().map(|p| match p {
            Ok((a, 0)) => a.to_string(),
            Ok((a, w)) => format!("{a} - {}", a + w),
            Err(s) => s.to_string(),
        }).collect();
        let text = text.join(", ");
        let text = if brackets { format!(" ({text}) ") } else { text };
        let want: Option<Vec<RangeInclusive<u32>>> =
            parts.iter().map(|p| p.ok().map(|(a, w)| a..=a + w)).collect();
        prop_assert_eq!(parse_ranges(&text), want.clone(), "{}", text);
        let values = want
            .filter(|r| r.iter().all(|r| r.end() - r.start() < 1024))
            .map(|r| r.into_iter().flatten().collect::<Vec<u32>>());
        prop_assert_eq!(parse_values(&text), values, "{}", text);
    }

    /// `<prefix>:` in any case and spacing gives the rest; a longer or shorter command does not.
    #[test]
    fn prefixes_match_whole_commands(body in "([0-9a-z][0-9a-z,]{0,8})?", mask in any::<u8>(), style in 0u8..3) {
        let name: String = "+CREG".chars().enumerate()
            .map(|(i, c)| if mask >> i & 1 == 1 { c.to_ascii_lowercase() } else { c })
            .collect();
        let line = |name: &str| match style {
            0 => format!("{name}: {body}"),
            1 => format!("\r\n{name} : {body}\r\n"),
            _ => format!(" {name}:{body} "),
        };
        let strip = |l: &str| strip_prefix(l, "+CREG").map(str::to_string);
        prop_assert_eq!(strip(&line(&name)), Some(body.clone()));
        prop_assert_eq!(strip(&line("+CREGX")), None);
        prop_assert_eq!(strip(&line("+CR")), None);
        prop_assert_eq!(strip(&name), None);
        prop_assert_eq!(strip("é+"), None);
    }
}
