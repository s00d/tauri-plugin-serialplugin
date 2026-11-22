//! Model: USSD strings (23.038 §5, §6.1.2.3.1) and the `+CUSD` line (27.007 §7.15).
//!
//! proptest draws septets, texts and data coding schemes, and `+CUSD` lines in the forms modems
//! send them (quoted with and without escapes, unclosed, bare with commas, a bad `<dcs>`, a
//! glued `OK`). The model says which fields each line has and how its text reads.

use crate::common::*;
use crate::model_sms::{chars_of, default_pool};
use proptest::option::of;
use serial_codec::sms::gsm7::{self, Tables};
use serial_codec::sms::pdu::Alphabet;
use serial_codec::sms::{hex_decode, hex_encode, ucs2};
use serial_codec::ussd::{self, CbsDcs, Cscs, Cusd, CusdStatus};

/// Default-alphabet text. CR CR at the end is left out: on an octet boundary it packs like one
/// CR plus the padding one (23.038 §6.1.2.3.1).
fn default_text(max: usize) -> impl Strategy<Value = String> {
    vec(select(default_pool()), 0..=max).prop_map(|v| {
        let mut s = String::from_iter(v);
        if s.ends_with("\r\r") {
            s.push('x');
        }
        s
    })
}

/// 23.038 §5, written from the table: (alphabet, compressed, language prefix, reserved,
/// language).
fn cbs_model(raw: u8) -> (Alphabet, bool, bool, bool, Option<&'static str>) {
    const G0: [&str; 15] = [
        "de", "en", "it", "fr", "es", "nl", "sv", "da", "pt", "fi", "no", "el", "tr", "hu", "pl",
    ];
    const G2: [&str; 5] = ["cs", "he", "ar", "ru", "is"];
    let low = usize::from(raw & 15);
    let coded = |bits: u8| match bits & 3 {
        0 => (Alphabet::Gsm7, false),
        1 => (Alphabet::Bit8, false),
        2 => (Alphabet::Ucs2, false),
        _ => (Alphabet::Gsm7, true),
    };
    match raw >> 4 {
        0 => (Alphabet::Gsm7, false, false, false, G0.get(low).copied()),
        1 if raw == 0x10 => (Alphabet::Gsm7, false, true, false, None),
        1 if raw == 0x11 => (Alphabet::Ucs2, false, true, false, None),
        2 if low <= 4 => (Alphabet::Gsm7, false, false, false, Some(G2[low])),
        4..=7 => {
            let (a, r) = coded(raw >> 2);
            (a, raw & 0x20 != 0, false, r, None)
        }
        9 => {
            let (a, r) = coded(raw >> 2);
            (a, false, false, r, None)
        }
        0xF if raw & 4 != 0 => (Alphabet::Bit8, false, false, false, None),
        0xF => (Alphabet::Gsm7, false, false, false, None),
        _ => (Alphabet::Gsm7, false, false, true, None),
    }
}

#[test]
fn every_cbs_dcs_octet_matches_23038() {
    for raw in 0..=255u8 {
        let d = CbsDcs::decode(raw);
        assert_eq!(
            (
                d.alphabet,
                d.compressed,
                d.language_prefix,
                d.reserved,
                d.language()
            ),
            cbs_model(raw),
            "{raw:#04x}"
        );
        assert_eq!(d.raw, raw);
    }
}

proptest! {
    #![proptest_config(cases(512))]

    /// The CR padding rule is invisible after unpacking, for every fill.
    #[test]
    fn septets_round_trip(s in vec(prop_oneof![0u8..0x80, Just(0x0D)], 0..100), fill in 0u8..8) {
        let packed = ussd::pack_septets(&s, fill);
        // Text that really ends in CR CR with the first CR on an octet boundary packs like a
        // single CR plus the added one.
        let n = s.len();
        let ambiguous = s.ends_with(&[0x0D, 0x0D]) && (usize::from(fill) + 7 * (n - 1)) % 8 == 0;
        if !ambiguous {
            prop_assert_eq!(ussd::unpack_septets(&packed, fill), s.clone());
        }
        let bits = usize::from(fill) + 7 * n;
        let padded = bits % 8 == 1 || (bits % 8 == 0 && s.last() == Some(&0x0D));
        let want = if padded { [&s[..], &[0x0D]].concat() } else { s };
        prop_assert_eq!(packed, gsm7::pack(&want, fill));
    }

    /// A string decodes by its DCS: packed GSM 7-bit (with an `xx\r` language prefix only under
    /// 0x10, and only when it is there), UCS2 (behind two packed language characters for 0x11),
    /// ISO-8859-1 for 8-bit; compressed data is refused.
    #[test]
    fn strings_decode_by_their_dcs(
        dcs in prop_oneof![4 => any::<u8>(), 1 => 0x0Fu8..=0x11],
        gsm in prop_oneof![3 => default_text(60), 1 => "[a-z]{2}\r[a-z]{0,4}"],
        wide in any::<String>(),
        lang in of(chars_of("abcdefghij", 2..=2)),
    ) {
        prop_assert_eq!(ussd::decode_gsm7(&ussd::encode(&gsm).unwrap()), gsm.clone());
        let d = CbsDcs::decode(dcs);
        let named = d.language().map(String::from);
        let latin: String = wide.chars().map(|c| char::from((c as u32 & 0xFF) as u8)).collect();
        let (payload, text, language) = match (d.alphabet, d.language_prefix) {
            _ if d.compressed => {
                prop_assert_eq!(ussd::decode(b"\x01\x02", dcs), None);
                return Ok(());
            }
            (Alphabet::Gsm7, true) => {
                let own = gsm.chars().nth(2) == Some('\r');
                match lang {
                    Some(l) => (ussd::encode(&format!("{l}\r{gsm}")).unwrap(), gsm, Some(l)),
                    None if own => return Ok(()),
                    None => (ussd::encode(&gsm).unwrap(), gsm, None),
                }
            }
            (Alphabet::Gsm7, false) => (ussd::encode(&gsm).unwrap(), gsm, named),
            (Alphabet::Ucs2, true) => {
                prop_assert_eq!(ussd::decode(b"\x61", dcs), None, "no room for the language");
                let l = lang.unwrap_or_else(|| "ar".into());
                let septets = gsm7::encode(&l, Tables::DEFAULT).unwrap();
                ([gsm7::pack(&septets, 0), ucs2::encode(&wide)].concat(), wide, Some(l))
            }
            (Alphabet::Ucs2, false) => (ucs2::encode(&wide), wide, named),
            (Alphabet::Bit8, _) => (latin.chars().map(|c| c as u8).collect(), latin, None),
        };
        let got = ussd::decode(&payload, dcs).unwrap();
        prop_assert_eq!((got.text, got.language), (text, language));
    }
}

// ------------------------------------------------------------------ +CUSD

/// `<dcs>` as written, and as read.
#[derive(Debug, Clone)]
enum DcsField {
    None,
    Num(u8),
    Bad(String),
}

/// `<str>` as written and as read, and whether text or a `<dcs>` may follow it.
#[derive(Debug, Clone)]
enum Field {
    /// Quoted: escapes, or bare inner quotes (the closing quote is the last one).
    Quoted(String, String),
    /// A quote that never closes: all the rest is text.
    Open(String),
    /// Without quotes: the last `,<digits>` is the DCS, other commas are text.
    Bare(String),
}

fn field() -> impl Strategy<Value = Field> {
    prop_oneof![
        3 => any::<String>().prop_map(|s| {
            let esc = s.replace('\\', "\\\\").replace('"', "\\\"");
            Field::Quoted(format!("\"{esc}\""), s)
        }),
        1 => "[a-z,\" 0-9]{0,12}".prop_map(|s| Field::Quoted(format!("\"{s}\""), s)),
        1 => "[a-z0-9 ,*#]{0,15}[a-z0-9]".prop_map(Field::Open),
        2 => "[a-j*#]([a-j ,*#]{0,10}[a-j*#,])?".prop_map(Field::Bare),
    ]
}

fn cusd_line() -> impl Strategy<Value = (String, Cusd)> {
    let dcs = prop_oneof![
        Just(DcsField::None),
        any::<u8>().prop_map(DcsField::Num),
        (256u16..1000).prop_map(|d| DcsField::Bad(d.to_string())),
        Just(DcsField::Bad("x".into())),
    ];
    (
        any::<u8>(),
        of((field(), dcs)),
        select(vec!["+CUSD:", "+cusd:", "+CUSD: ", "\r\n+Cusd:  "]),
        select(vec!["", "OK", " OK", "\r\n"]),
    )
        .prop_map(|(m, rest, head, ok)| {
            let mut want = Cusd {
                status: CusdStatus::from_code(m),
                raw: None,
                dcs: None,
                invalid_dcs: None,
            };
            let mut line = format!("{head}{m}");
            match rest {
                None => {}
                Some((Field::Open(s), _)) => {
                    line += &format!(",\"{s}{ok}");
                    want.raw = Some(format!("{s}{ok}").trim_end().into());
                    return (line, want);
                }
                // An `OK` glued to bare text, or a `<dcs>` that is no number, is text too.
                Some((Field::Bare(s), DcsField::None)) => {
                    line += &format!(",{s}{ok}");
                    want.raw = Some(format!("{s}{ok}").trim_end().into());
                    return (line, want);
                }
                Some((Field::Bare(s), DcsField::Bad(d))) if d == "x" => {
                    line += &format!(",{s},{d}");
                    want.raw = Some(format!("{s},{d}"));
                }
                Some((Field::Quoted(wire, raw), dcs)) => {
                    line += &format!(",{wire}");
                    want.raw = Some(raw);
                    push_dcs(&mut line, &mut want, dcs);
                }
                Some((Field::Bare(s), dcs)) => {
                    line += &format!(",{s}");
                    want.raw = Some(s);
                    push_dcs(&mut line, &mut want, dcs);
                }
            }
            // A glued `OK` after the string, the DCS or `<m>` is dropped.
            if want.raw.as_deref().is_some_and(|r| r.ends_with(",x")) && want.dcs.is_none() {
                return (line, want);
            }
            if matches!(want.invalid_dcs.as_deref(), Some("x")) {
                return (line, want);
            }
            line.push_str(ok);
            (line, want)
        })
}

fn push_dcs(line: &mut String, want: &mut Cusd, dcs: DcsField) {
    match dcs {
        DcsField::None => {}
        DcsField::Num(d) => {
            line.push_str(&format!(",{d}"));
            want.dcs = Some(d);
        }
        DcsField::Bad(d) => {
            line.push_str(&format!(", {d}"));
            want.invalid_dcs = Some(d);
        }
    }
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn cusd_lines_parse_to_their_fields((line, want) in cusd_line(), other in select(vec!["+CUSD: x", "+CUSD:", "+CUSD: 256", "+CMTI: 1", "+CUSD 1"])) {
        prop_assert_eq!(ussd::parse_cusd(&line), Some(want.clone()), "{}", line);
        let m = line.trim_start().split_once(':').unwrap().1.trim_start();
        let m: u8 = m[..m.find(|c: char| !c.is_ascii_digit()).unwrap_or(m.len())].parse().unwrap();
        use CusdStatus::*;
        let status = [Done, ActionRequired, Terminated, OtherClient, NotSupported, Timeout];
        prop_assert_eq!(want.status, status.get(usize::from(m)).copied().unwrap_or(Other(m)));
        prop_assert_eq!(want.status.session_active(), m == 1);
        prop_assert_eq!(ussd::parse_cusd(other), None);
    }

    /// `<str>` is read by `<dcs>` and the TE character set: as sent under a GSM 7-bit DCS (also
    /// when it looks like hex), hex under `UCS2` / `HEX` and the UCS2 / 8-bit DCS (text that is
    /// no hex stays as sent there); packed septets only on request.
    #[test]
    fn cusd_text_follows_dcs_and_cscs(
        s in prop_oneof![any::<String>(), "([0-9A-F]{2}){0,6}"], wide in any::<String>(),
        gsm in default_text(40), m in 0u8..6,
        dcs in prop_oneof![Just(None), Just(Some(0x0F)), Just(Some(0x00))],
    ) {
        let cusd = |raw: String, dcs: Option<u8>| Cusd {
            status: CusdStatus::from_code(m), raw: Some(raw), dcs, invalid_dcs: None,
        };
        let octets = hex_decode(&s);
        let plain = cusd(s.clone(), dcs);
        prop_assert_eq!(plain.text(), Some(s.clone()));
        prop_assert_eq!(plain.text_as(Cscs::Ira), Some(s.clone()));
        prop_assert_eq!(plain.text_as(Cscs::Ucs2), octets.clone().map(|b| ucs2::decode(&b)));
        prop_assert_eq!(plain.text_as(Cscs::Hex), octets.clone().map(|b| gsm7::decode(&b, Tables::DEFAULT)));
        prop_assert_eq!(plain.text_packed_hex().is_some(), octets.is_some());
        prop_assert_eq!(cusd(hex_encode(&ucs2::encode(&wide)), dcs).text_as(Cscs::Ucs2), Some(wide.clone()));
        let septets = gsm7::encode(&gsm, Tables::DEFAULT).unwrap();
        prop_assert_eq!(cusd(hex_encode(&septets), dcs).text_as(Cscs::Hex), Some(gsm.clone()));
        let packed = hex_encode(&ussd::encode(&gsm).unwrap());
        prop_assert_eq!(cusd(packed, Some(0x0F)).text_packed_hex(), Some(gsm));
        for cs in [Cscs::Gsm, Cscs::Ira, Cscs::Ucs2, Cscs::Hex] {
            let c = cusd(hex_encode(&ucs2::encode(&wide)), Some(0x48));
            prop_assert_eq!(c.text_as(cs), Some(wide.clone()));
            prop_assert_eq!(c.text_packed_hex(), Some(wide.clone()));
            if octets.is_none() {
                prop_assert_eq!(cusd(s.clone(), Some(0x48)).text_as(cs), Some(s.clone()));
            }
        }
        prop_assert_eq!(cusd(s.clone(), Some(0x60)).text(), None, "compressed");
        prop_assert_eq!(Cusd { raw: None, ..plain }.text(), None);
    }

    #[test]
    fn any_text_parses_or_not(s in ".{0,200}", bytes in vec(any::<u8>(), 0..200), m in 0u8..10, dcs in any::<u8>()) {
        let _ = ussd::unpack_septets(&bytes, dcs);
        let _ = ussd::decode(&bytes, dcs);
        let _ = ussd::parse_cusd(&s);
        for line in [format!("+CUSD: {m},{s},{dcs}"), format!("+cusd: {m},\"{s}\",{dcs}OK")] {
            let Some(c) = ussd::parse_cusd(&line) else { continue };
            for cs in [Cscs::Gsm, Cscs::Ucs2, Cscs::Hex] {
                let _ = c.text_as(cs);
            }
            let _ = c.text_packed_hex();
        }
    }
}
