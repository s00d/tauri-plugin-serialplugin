//! AT argument splitting (V.250 §5.4.2): generated parameter lists split back into
//! their fields; stories for escapes, groups and value limits.

use crate::common::*;
use serial_codec::args::{parse_ranges, parse_values, split, strip_prefix};

#[derive(Debug, Clone)]
enum Field {
    Quoted(String),
    Bare(String),
}

fn field() -> impl Strategy<Value = Field> {
    prop_oneof![
        "[A-Za-z0-9 ,.+()-]{0,10}".prop_map(Field::Quoted),
        "[A-Za-z0-9.+-]{0,10}".prop_map(Field::Bare),
    ]
}

proptest! {
    #![proptest_config(cases(512))]

    /// Fields joined with commas (and spaces around them) split back into the same
    /// texts and quoted flags; commas inside quotes do not split.
    #[test]
    fn split_round_trip(fields in vec(field(), 1..8), pad in "[ ]{0,2}") {
        let text: Vec<String> = fields.iter().map(|f| match f {
            Field::Quoted(s) => format!("{pad}\"{s}\"{pad}"),
            Field::Bare(s) => format!("{pad}{s}{pad}"),
        }).collect();
        let line = text.join(",");
        let got: Vec<(String, bool)> = split(&line).into_iter().map(|a| (a.text, a.quoted)).collect();
        let want: Vec<(String, bool)> = fields.iter().map(|f| match f {
            Field::Quoted(s) => (s.clone(), true),
            Field::Bare(s) => (s.clone(), false),
        }).collect();
        if line.trim().is_empty() {
            prop_assert!(got.is_empty());
        } else {
            prop_assert_eq!(got, want);
        }
    }

    /// Numbers and hex values read back; ranges expand to their values.
    #[test]
    fn values(n in any::<u32>(), h in any::<u64>(), ranges in vec((0u32..50, 0u32..20), 0..4)) {
        prop_assert_eq!(split(&n.to_string())[0].int::<u32>(), Some(n));
        prop_assert_eq!(split(&format!("\"{n}\""))[0].int::<u32>(), Some(n));
        prop_assert_eq!(split(&format!("{h:X}"))[0].hex(), Some(h));
        prop_assert_eq!(split(&format!("\"0x{h:x}\""))[0].hex(), Some(h));
        let text: Vec<String> = ranges.iter().map(|(a, w)| format!("{a}-{}", a + w)).collect();
        let want: Vec<u32> = ranges.iter().flat_map(|&(a, w)| a..=a + w).collect();
        prop_assert_eq!(parse_values(&format!("({})", text.join(","))), Some(want));
        let l = format!("\r\n+creg : {n}\r\n");
        prop_assert_eq!(strip_prefix(&l, "+CREG").map(str::to_string), Some(n.to_string()));
    }
}

/// Given quoted strings with `\HH` escapes, lone backslashes, an unterminated quote and
/// a parenthesised group
/// When they are split
/// Then `\HH` is that character, any other backslash is text, a quote always closes,
/// the open string runs to the end, and a group keeps its inner commas
#[test]
fn escapes() {
    let texts = |s: &str| -> Vec<(String, bool)> {
        split(s).into_iter().map(|a| (a.text, a.quoted)).collect()
    };
    assert!(split("").is_empty());
    assert_eq!(texts(","), vec![("".into(), false), ("".into(), false)]);
    assert_eq!(texts(r#""a,\22b\22\5C""#), vec![(r#"a,"b"\"#.into(), true)]);
    assert_eq!(texts(r#""\41\4""#), vec![("A\\4".into(), true)]);
    assert_eq!(
        texts(r#""C:\",1,"x""#),
        vec![
            ("C:\\".into(), true),
            ("1".into(), false),
            ("x".into(), true)
        ]
    );
    assert_eq!(texts(r#""open, end"#), vec![("open, end".into(), true)]);
    assert_eq!(texts(r#",, """#)[2], ("".into(), true));
    let g = split(r#"(2,"A,B","x",7),,(0-4)"#);
    assert_eq!(g.len(), 3);
    let inner = g[0].group().unwrap();
    assert_eq!(
        (inner[1].str(), inner[3].int::<u8>()),
        (Some("A,B"), Some(7))
    );
    assert!(g[1].is_empty());
    // 16 hex digits fit a u64; a 17th is refused even as a leading zero.
    assert_eq!(split("00000000000000001")[0].hex(), None);
    assert_eq!(split("0x")[0].hex(), None);
    assert_eq!(split("\"8BE3\"")[0].int::<u8>(), None);
    assert_eq!(strip_prefix("+CREGX: 1", "+CREG"), None);
    assert_eq!(strip_prefix("+CR", "+CREG"), None);
    assert_eq!(parse_ranges("1-0"), None);
    assert_eq!(parse_values("0-100000"), None);
}
