//! SMS / USSD codecs as properties: encode → decode round trips over every field,
//! reference models for the field codecs (BCD, timestamps, DCS, UDH), a model test for
//! the concatenation reassembler, and "no input panics" for every decoder.

use crate::common::*;
use serial_codec::sms::concat::Reassembler;
use serial_codec::sms::gsm7::{self, Language, Tables, ESC};
use serial_codec::sms::pdu::{
    self, decode_address, decode_semi_octets, decode_smsc, decode_tpdu, encode_address,
    encode_semi_octets, encode_smsc, encode_submit, relative_from_minutes, relative_minutes,
    Address, Alphabet, Body, Dcs, Deliver, Ie, MwiKind, PduError, StatusReport, SubmitRequest,
    Timestamp, Ton, Tpdu, Udh, UserData, Validity,
};
use serial_codec::sms::{hex_decode, hex_encode, ucs2};
use serial_codec::ussd::{self, CbsDcs, Cscs, Cusd, CusdStatus};

pub(crate) const LANGS: [Language; 4] = [
    Language::Default,
    Language::Turkish,
    Language::Spanish,
    Language::Portuguese,
];

fn tables() -> impl Strategy<Value = Tables> {
    (select(LANGS.to_vec()), select(LANGS.to_vec())).prop_map(|(l, s)| {
        Tables::new(
            if l.has_locking_table() {
                l
            } else {
                Language::Default
            },
            s,
        )
    })
}

/// Every character `t` encodes (plain septets and escape pairs).
fn gsm_pool(t: Tables) -> Vec<char> {
    let mut v: Vec<char> = (0..0x80u8)
        .filter(|&s| s != ESC)
        .flat_map(|s| {
            gsm7::decode(&[s], t)
                .chars()
                .chain(gsm7::decode(&[ESC, s], t).chars())
                .collect::<Vec<_>>()
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn default_pool() -> Vec<char> {
    gsm_pool(Tables::DEFAULT)
}

/// Characters of every national table, i.e. the ones whose encoding depends on tables.
fn national_pool() -> Vec<char> {
    let mut v: Vec<char> = LANGS
        .iter()
        .flat_map(|&l| {
            let mut c = gsm_pool(Tables::new(l, Language::Default));
            c.extend(gsm_pool(Tables::new(Language::Default, l)));
            c
        })
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn sms_char() -> impl Strategy<Value = char> {
    prop_oneof![
        8 => select(default_pool()),
        3 => select(national_pool()),
        1 => select(vec!['Ж', '😁', '中', '\u{1F600}', 'é']),
        1 => any::<char>(),
    ]
}

fn sms_text(max: usize) -> impl Strategy<Value = String> {
    vec(sms_char(), 0..=max).prop_map(String::from_iter)
}

fn chars_of(alphabet: &'static str, len: std::ops::RangeInclusive<usize>) -> BoxedStrategy<String> {
    vec(select(alphabet.chars().collect::<Vec<_>>()), len)
        .prop_map(String::from_iter)
        .boxed()
}

// ------------------------------------------------------------------ addresses

/// A TP address as a request writes it, with the address its decoding must give.
#[derive(Debug, Clone)]
struct Dest {
    value: String,
    toa: Option<u8>,
    want: Address,
}

fn non_alnum_toa() -> impl Strategy<Value = u8> {
    any::<u8>().prop_filter("TON 5 is alphanumeric", |t| (t >> 4) & 7 != 5)
}

fn dest() -> impl Strategy<Value = Dest> {
    let intl = chars_of("0123456789", 1..=20).prop_map(|d| Dest {
        value: format!("+{d}"),
        toa: None,
        want: Address {
            toa: pdu::TOA_INTERNATIONAL,
            value: d,
        },
    });
    let plain = chars_of("0123456789*#", 1..=20).prop_map(|d| Dest {
        value: d.clone(),
        toa: None,
        want: Address {
            toa: pdu::TOA_UNKNOWN,
            value: d,
        },
    });
    let with_toa =
        (chars_of("0123456789*#abcABC", 1..=20), non_alnum_toa()).prop_map(|(d, t)| Dest {
            want: Address {
                toa: t,
                value: d.to_lowercase(),
            },
            value: d,
            toa: Some(t),
        });
    let alnum = vec(select(default_pool()), 1..=11)
        .prop_map(String::from_iter)
        .prop_filter("an alphanumeric address", |s| {
            gsm7::septet_len(s, Tables::DEFAULT).is_some_and(|n| n <= 11)
                && !s.starts_with('+')
                && !s
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'*' || b == b'#')
        })
        .prop_map(|s| Dest {
            value: s.clone(),
            toa: None,
            want: Address {
                toa: pdu::TOA_ALPHANUMERIC,
                value: s,
            },
        });
    prop_oneof![intl, plain, with_toa, alnum]
}

/// SMSC as a request writes it, and the decoded SMSC.
fn smsc() -> impl Strategy<Value = (Option<String>, Option<u8>, Option<Address>)> {
    prop_oneof![
        Just((None, None, None)),
        Just((Some(String::new()), None, None)),
        (
            any::<bool>(),
            chars_of("0123456789*#abc", 1..=20),
            proptest::option::of(non_alnum_toa())
        )
            .prop_map(|(plus, d, toa)| {
                let want = Address {
                    toa: toa.unwrap_or(if plus { 0x91 } else { 0x81 }),
                    value: d.clone(),
                };
                let v = if plus { format!("+{d}") } else { d };
                (Some(v), toa, Some(want))
            }),
    ]
}

// ------------------------------------------------------------------ timestamps

fn leap(y: u16) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn days_in(y: u16, m: u8) -> u8 {
    [
        31,
        if leap(y) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ][usize::from(m - 1)]
}

fn valid_ts() -> impl Strategy<Value = Timestamp> {
    (
        1970u16..=2069,
        1u8..=12,
        1u8..=31,
        0u8..24,
        0u8..60,
        0u8..60,
        -79i8..=79,
    )
        .prop_map(
            |(year, month, day, hour, minute, second, tz_quarters)| Timestamp {
                year,
                month,
                day: day.min(days_in(year, month)),
                hour,
                minute,
                second,
                tz_quarters,
            },
        )
}

/// 23.040 §9.2.3.11: each field is two swapped BCD digits; bit 3 of the zone is the sign.
fn ts_bytes(t: &Timestamp) -> [u8; 7] {
    let b = |v: u8| ((v % 10) << 4) | (v / 10);
    let q = t.tz_quarters.unsigned_abs();
    [
        b((t.year % 100) as u8),
        b(t.month),
        b(t.day),
        b(t.hour),
        b(t.minute),
        b(t.second),
        b(q) | if t.tz_quarters < 0 { 0x08 } else { 0 },
    ]
}

/// Reference decoder: `None` unless every digit is BCD and every field in range.
fn ts_model(raw: &[u8; 7]) -> Option<Timestamp> {
    let bcd = |b: u8| ((b & 15) < 10 && (b >> 4) < 10).then_some((b & 15) * 10 + (b >> 4));
    let yy = bcd(raw[0])?;
    let year = u16::from(yy) + if yy >= 70 { 1900 } else { 2000 };
    let (month, day, hour, minute, second) = (
        bcd(raw[1])?,
        bcd(raw[2])?,
        bcd(raw[3])?,
        bcd(raw[4])?,
        bcd(raw[5])?,
    );
    let q = bcd(raw[6] & !0x08)? as i8;
    let ok = (1..=12).contains(&month)
        && (1..=days_in(year, month)).contains(&day)
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
        tz_quarters: if raw[6] & 0x08 != 0 { -q } else { q },
    })
}

fn unix_model(t: &Timestamp) -> i64 {
    let mut days: i64 = (1970..t.year)
        .map(|y| if leap(y) { 366 } else { 365 })
        .sum();
    days += (1..t.month)
        .map(|m| i64::from(days_in(t.year, m)))
        .sum::<i64>();
    days += i64::from(t.day) - 1;
    days * 86_400 + i64::from(t.hour) * 3600 + i64::from(t.minute) * 60 + i64::from(t.second)
        - i64::from(t.tz_quarters) * 900
}

/// Raw octets: valid ones, valid ones with one octet replaced, and noise.
fn ts_raw() -> impl Strategy<Value = [u8; 7]> {
    prop_oneof![
        valid_ts().prop_map(|t| ts_bytes(&t)),
        (valid_ts(), 0usize..7, any::<u8>()).prop_map(|(t, i, b)| {
            let mut r = ts_bytes(&t);
            r[i] = b;
            r
        }),
        any::<[u8; 7]>(),
    ]
}

fn validity() -> impl Strategy<Value = Option<Validity>> {
    prop_oneof![
        Just(None),
        any::<u8>().prop_map(|v| Some(Validity::Relative(v))),
        valid_ts().prop_map(|t| Some(Validity::Absolute(t))),
        any::<[u8; 7]>().prop_map(|b| Some(Validity::Enhanced(b))),
    ]
}

proptest! {
    #![proptest_config(cases(512))]

    /// Every valid timestamp encodes to its BCD form and back; unix time and ISO text
    /// match references.
    #[test]
    fn timestamp_round_trip(t in valid_ts()) {
        prop_assert_eq!(t.encode(), Some(ts_bytes(&t)));
        prop_assert_eq!(Timestamp::decode(&ts_bytes(&t)), Some(t));
        prop_assert_eq!(t.unix_utc(), unix_model(&t));
        prop_assert_eq!(t.offset_minutes(), i32::from(t.tz_quarters) * 15);
        let off = t.offset_minutes().unsigned_abs();
        prop_assert_eq!(
            t.to_string(),
            format!(
                "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{}{:02}:{:02}",
                t.year, t.month, t.day, t.hour, t.minute, t.second,
                if t.tz_quarters < 0 { '-' } else { '+' }, off / 60, off % 60
            )
        );
    }

    /// Decoding any 7 octets agrees with the reference decoder.
    #[test]
    fn timestamp_decode_matches_model(raw in ts_raw()) {
        prop_assert_eq!(Timestamp::decode(&raw), ts_model(&raw));
        prop_assert_eq!(pdu::decode_tz(raw[6]), ts_model(&[0x01, 0x10, 0x10, 0, 0, 0, raw[6]]).map(|t| t.tz_quarters));
    }

    /// A field out of range never encodes.
    #[test]
    fn timestamp_out_of_range_does_not_encode(t in valid_ts(), field in 0usize..7, pick in any::<u8>()) {
        let mut bad = t;
        match field {
            0 => bad.year = if pick % 2 == 0 { 1969 - u16::from(pick) } else { 2070 + u16::from(pick) },
            1 => bad.month = if pick % 2 == 0 { 0 } else { 13 + pick % 243 },
            2 => bad.day = if pick % 2 == 0 { 0 } else { days_in(t.year, t.month) + 1 + pick % 200 },
            3 => bad.hour = 24 + pick % 232,
            4 => bad.minute = 60 + pick % 196,
            5 => bad.second = 60 + pick % 196,
            _ => bad.tz_quarters = if pick % 2 == 0 { -80 - (pick % 49) as i8 } else { 80 + (pick % 48) as i8 },
        }
        prop_assert_eq!(bad.encode(), None);
    }

    /// The relative TP-VP is the smallest octet covering the duration.
    #[test]
    fn relative_validity_rounds_up(minutes in 0u32..700_000) {
        match relative_from_minutes(minutes) {
            Some(vp) => {
                prop_assert!(relative_minutes(vp) >= minutes);
                prop_assert!(vp == 0 || relative_minutes(vp - 1) < minutes);
            }
            None => prop_assert!(minutes > relative_minutes(255)),
        }
    }
}

#[test]
fn every_relative_validity_octet_round_trips() {
    for vp in 0..=255u8 {
        assert_eq!(relative_from_minutes(relative_minutes(vp)), Some(vp));
    }
}

// ------------------------------------------------------------------ semi-octets, hex, TOA

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn semi_octets_round_trip(d in chars_of("0123456789*#abcABC", 0..=40), cut in any::<usize>()) {
        let enc = encode_semi_octets(&d).unwrap();
        prop_assert_eq!(enc.len(), d.len().div_ceil(2));
        let lower = d.to_lowercase();
        prop_assert_eq!(decode_semi_octets(&enc, d.len()), lower.clone());
        // An odd count is padded with F, which ends the digits.
        prop_assert_eq!(decode_semi_octets(&enc, usize::MAX), lower.clone());
        let k = cut % (d.len() + 1);
        prop_assert_eq!(decode_semi_octets(&enc, k), lower[..k].to_string());
    }

    #[test]
    fn semi_octets_name_the_first_bad_character(
        d in chars_of("0123456789*#abc", 0..=10),
        bad in any::<char>().prop_filter("no semi-octet", |c| !"0123456789*#abcABC".contains(*c)),
        tail in any::<String>(),
    ) {
        prop_assert_eq!(encode_semi_octets(&format!("{d}{bad}{tail}")), Err(bad));
    }

    #[test]
    fn hex_round_trip(b in vec(any::<u8>(), 0..64), junk in any::<char>()) {
        let h = hex_encode(&b);
        prop_assert_eq!(h.len(), 2 * b.len());
        prop_assert!(h.bytes().all(|c| c.is_ascii_digit() || (b'A'..=b'F').contains(&c)));
        prop_assert_eq!(hex_decode(&h), Some(b.clone()));
        prop_assert_eq!(hex_decode(&format!(" {} \r\n", h.to_lowercase())), Some(b.clone()));
        prop_assert_eq!(hex_decode(&format!("{h}0")), None);
        if !junk.is_ascii_hexdigit() && !junk.is_whitespace() {
            prop_assert_eq!(hex_decode(&format!("{h}{junk}{junk}")), None);
        }
    }

    #[test]
    fn toa_fields(toa in any::<u8>(), value in chars_of("0123", 0..=2)) {
        let want = [Ton::Unknown, Ton::International, Ton::National, Ton::NetworkSpecific,
            Ton::Subscriber, Ton::Alphanumeric, Ton::Abbreviated, Ton::Reserved];
        let a = Address { toa, value: value.clone() };
        prop_assert_eq!(a.ton(), want[usize::from((toa >> 4) & 7)]);
        prop_assert_eq!(a.npi(), toa & 0x0F);
        let plus = a.ton() == Ton::International && !value.is_empty();
        prop_assert_eq!(a.to_string(), format!("{}{value}", if plus { "+" } else { "" }));
    }

    /// Addresses encode and decode to the same value and TOA; the octet count matches.
    #[test]
    fn address_round_trip(d in dest(), tail in vec(any::<u8>(), 0..4)) {
        let mut enc = encode_address(&d.value, d.toa).unwrap();
        let n = enc.len();
        enc.extend(tail);
        prop_assert_eq!(decode_address(&enc), Ok((d.want, n)));
    }

    #[test]
    fn address_length_limits(n in 21usize..30, alnum in 12usize..=20) {
        prop_assert!(matches!(encode_address(&"1".repeat(n), None), Err(PduError::Invalid(_))));
        let plus = format!("+{}", "1".repeat(n));
        prop_assert!(matches!(encode_address(&plus, None), Err(PduError::Invalid(_))));
        prop_assert!(matches!(encode_address(&"x".repeat(alnum), None), Err(PduError::Invalid(_))));
        prop_assert!(matches!(encode_smsc(Some(&"1".repeat(n)), None), Err(PduError::Invalid(_))));
        let mut b = vec![u8::try_from(n.min(255)).unwrap(), 0x91];
        b.extend(vec![0x21; n]);
        prop_assert!(matches!(decode_address(&b), Err(PduError::Invalid(_))));
    }

    #[test]
    fn smsc_round_trip((value, toa, want) in smsc(), tail in vec(any::<u8>(), 0..4)) {
        let mut enc = encode_smsc(value.as_deref(), toa).unwrap();
        let n = enc.len();
        enc.extend(tail);
        prop_assert_eq!(decode_smsc(&enc), Ok((want, n)));
    }
}

#[test]
fn smsc_length_octet_over_11_is_invalid() {
    for len in 12..=255u8 {
        let mut b = vec![len, 0x91];
        b.extend(vec![0x21; 255]);
        assert!(matches!(decode_smsc(&b), Err(PduError::Invalid(_))));
    }
}

// ------------------------------------------------------------------ DCS, UDH

/// 23.038 §4, written from the table: (alphabet, class, compressed, auto_delete,
/// MWI (active, kind, store), reserved).
type DcsModel = (
    Alphabet,
    Option<u8>,
    bool,
    bool,
    Option<(bool, MwiKind, bool)>,
    bool,
);

fn dcs_model(raw: u8) -> DcsModel {
    let kinds = [
        MwiKind::Voicemail,
        MwiKind::Fax,
        MwiKind::Email,
        MwiKind::Other,
    ];
    let mwi = |store| Some((raw & 0x08 != 0, kinds[usize::from(raw & 3)], store));
    match raw >> 4 {
        0x0..=0x7 => {
            let (alphabet, reserved) = match (raw >> 2) & 3 {
                0 => (Alphabet::Gsm7, false),
                1 => (Alphabet::Bit8, false),
                2 => (Alphabet::Ucs2, false),
                _ => (Alphabet::Gsm7, true),
            };
            let class = (raw & 0x10 != 0).then_some(raw & 3);
            (
                alphabet,
                class,
                raw & 0x20 != 0,
                raw & 0x40 != 0,
                None,
                reserved,
            )
        }
        0xC => (Alphabet::Gsm7, None, false, false, mwi(false), false),
        0xD => (Alphabet::Gsm7, None, false, false, mwi(true), false),
        0xE => (Alphabet::Ucs2, None, false, false, mwi(true), false),
        0xF => {
            let a = if raw & 0x04 != 0 {
                Alphabet::Bit8
            } else {
                Alphabet::Gsm7
            };
            (a, Some(raw & 3), false, false, None, false)
        }
        _ => (Alphabet::Gsm7, None, false, false, None, true),
    }
}

#[test]
fn every_dcs_octet_matches_the_23038_table() {
    for raw in 0..=255u8 {
        let d = Dcs::decode(raw);
        let got = (
            d.alphabet,
            d.class,
            d.compressed,
            d.auto_delete,
            d.mwi.map(|m| (m.active, m.kind, m.store)),
            d.reserved,
        );
        assert_eq!(got, dcs_model(raw), "{raw:#04x}");
        assert_eq!(d.raw, raw);
    }
    for a in [Alphabet::Gsm7, Alphabet::Bit8, Alphabet::Ucs2] {
        for class in [None, Some(0), Some(1), Some(2), Some(3)] {
            let d = Dcs::decode(Dcs::encode(a, class));
            assert_eq!(
                (d.alphabet, d.class, d.compressed, d.auto_delete),
                (a, class, false, false)
            );
        }
    }
}

const IEIS: [u8; 7] = [
    pdu::IEI_CONCAT_8,
    pdu::IEI_PORTS_8,
    pdu::IEI_PORTS_16,
    pdu::IEI_CONCAT_16,
    pdu::IEI_SINGLE_SHIFT,
    pdu::IEI_LOCKING_SHIFT,
    0x70,
];

fn ie() -> impl Strategy<Value = Ie> {
    (select(IEIS.to_vec()), 0usize..6).prop_flat_map(|(id, len)| {
        // National language IEs carry small identifiers so known tables come up.
        let byte = if id == pdu::IEI_SINGLE_SHIFT || id == pdu::IEI_LOCKING_SHIFT {
            (0u8..6).boxed()
        } else {
            any::<u8>().boxed()
        };
        vec(byte, len).prop_map(move |data| Ie { id, data })
    })
}

fn ies() -> impl Strategy<Value = Vec<Ie>> {
    vec(ie(), 0..5)
}

fn last(ies: &[Ie], id: u8) -> Option<&[u8]> {
    ies.iter()
        .rev()
        .find(|i| i.id == id)
        .map(|i| i.data.as_slice())
}

/// UDH semantics from 23.040 §9.2.3.24: the last IE of a kind counts; a 16-bit form of
/// the right size wins over the 8-bit one; `seq` must be in `1..=total`.
fn udh_model(ies: &[Ie]) -> (Option<pdu::Concat>, Option<pdu::Ports>, Tables) {
    let concat = match (last(ies, pdu::IEI_CONCAT_16), last(ies, pdu::IEI_CONCAT_8)) {
        (Some(&[h, l, total, seq]), _) => Some(pdu::Concat {
            reference: u16::from(h) << 8 | u16::from(l),
            total,
            seq,
            ref_16bit: true,
        }),
        (_, Some(&[r, total, seq])) => Some(pdu::Concat {
            reference: u16::from(r),
            total,
            seq,
            ref_16bit: false,
        }),
        _ => None,
    }
    .filter(|c| (1..=c.total).contains(&c.seq));
    let ports = match (last(ies, pdu::IEI_PORTS_16), last(ies, pdu::IEI_PORTS_8)) {
        (Some(&[a, b, c, d]), _) => Some(pdu::Ports {
            dest: u16::from(a) << 8 | u16::from(b),
            orig: u16::from(c) << 8 | u16::from(d),
            wide: true,
        }),
        (_, Some(&[d, o])) => Some(pdu::Ports {
            dest: d.into(),
            orig: o.into(),
            wide: false,
        }),
        _ => None,
    };
    let lang = |id| {
        last(ies, id)
            .and_then(|d| d.first())
            .and_then(|&n| LANGS.get(usize::from(n)).copied())
            .unwrap_or_default()
    };
    let locking = lang(pdu::IEI_LOCKING_SHIFT);
    let locking = if locking == Language::Spanish {
        Language::Default
    } else {
        locking
    };
    (
        concat,
        ports,
        Tables::new(locking, lang(pdu::IEI_SINGLE_SHIFT)),
    )
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn udh_round_trip_and_semantics(ies in ies(), cut in any::<usize>()) {
        let udh = Udh { ies: ies.clone() };
        let enc = udh.encode().unwrap();
        prop_assert_eq!(usize::from(enc[0]), enc.len() - 1);
        prop_assert_eq!(Udh::parse(&enc[1..]), Ok(udh.clone()));
        let (concat, ports, tables) = udh_model(&ies);
        prop_assert_eq!(udh.concat(), concat);
        prop_assert_eq!(udh.ports(), ports);
        prop_assert_eq!(udh.tables(), tables);
        let first = |id| last(&ies, id).and_then(|d| d.first().copied());
        prop_assert_eq!(udh.national_languages(), (first(pdu::IEI_SINGLE_SHIFT), first(pdu::IEI_LOCKING_SHIFT)));
        // A cut parses exactly when it falls on an IE boundary.
        let k = cut % enc.len();
        let mut bounds = vec![0];
        for i in &ies {
            bounds.push(bounds.last().unwrap() + 2 + i.data.len());
        }
        let ok = Udh::parse(&enc[1..1 + k]).is_ok();
        prop_assert_eq!(ok, bounds.contains(&k), "cut at {}", k);
    }

    #[test]
    fn udh_lengths_over_255_are_rejected(lens in vec(0usize..300, 1..4)) {
        let udh = Udh { ies: lens.iter().map(|&n| Ie { id: 0x70, data: vec![0; n] }).collect() };
        let total: usize = lens.iter().map(|n| n + 2).sum();
        let bad = lens.iter().any(|&n| n > 255) || total > 255;
        let enc = udh.encode();
        if bad {
            prop_assert!(matches!(enc, Err(PduError::Invalid(_))));
        } else {
            prop_assert_eq!(enc.map(|e| e.len()), Ok(total + 1));
        }
    }
}

// ------------------------------------------------------------------ TPDU round trips

/// TP-UDL and TP-UD for `payload` (septets or octets) behind an optional UDH, and the
/// user data the decoder must return.
fn user_data(dcs: u8, udh: &Option<Vec<Ie>>, payload: &[u8]) -> (u8, Vec<u8>, UserData) {
    let d = Dcs::decode(dcs);
    let septets = d.alphabet == Alphabet::Gsm7 && !d.compressed;
    let udh = udh.clone().map(|ies| Udh { ies });
    let hdr = udh
        .as_ref()
        .map(|u| u.encode().unwrap())
        .unwrap_or_default();
    let tables = udh.as_ref().map_or(Tables::DEFAULT, Udh::tables);
    let (udl, ud) = if septets {
        let mut s = vec![0; (hdr.len() * 8).div_ceil(7)];
        s.extend_from_slice(payload);
        let packed = gsm7::pack(&s, 0);
        (s.len(), [&hdr[..], &packed[hdr.len()..]].concat())
    } else {
        (hdr.len() + payload.len(), [&hdr[..], payload].concat())
    };
    let text = match (septets, d.alphabet, d.compressed) {
        (true, ..) => Some(gsm7::decode(payload, tables)),
        (false, Alphabet::Ucs2, false) => Some(ucs2::decode(payload)),
        _ => None,
    };
    let want = UserData {
        udh,
        payload: payload.to_vec(),
        text,
    };
    (u8::try_from(udl).unwrap(), ud, want)
}

/// DCS, UDH and a payload that fits one TPDU.
fn ud_parts() -> impl Strategy<Value = (u8, Option<Vec<Ie>>, Vec<u8>)> {
    (any::<u8>(), proptest::option::of(ies())).prop_flat_map(|(dcs, udh)| {
        let d = Dcs::decode(dcs);
        let hdr = udh.as_ref().map_or(0, |ies| {
            1 + ies.iter().map(|i| 2 + i.data.len()).sum::<usize>()
        });
        let payload = if d.alphabet == Alphabet::Gsm7 && !d.compressed {
            vec(0u8..0x80, 0..=160 - (hdr * 8).div_ceil(7)).boxed()
        } else if d.alphabet == Alphabet::Ucs2 && !d.compressed {
            vec(any::<u8>(), 0..=(140 - hdr) / 2)
                .prop_map(|v| v.iter().flat_map(|&b| [b, b ^ 0x5A]).collect())
                .boxed()
        } else {
            vec(any::<u8>(), 0..=140 - hdr).boxed()
        };
        (Just(dcs), Just(udh), payload)
    })
}

fn flags() -> impl Strategy<Value = [bool; 4]> {
    any::<[bool; 4]>()
}

proptest! {
    #![proptest_config(cases(256))]

    /// SMS-DELIVER built field by field decodes to exactly those fields; trailing octets
    /// are ignored.
    #[test]
    fn deliver_round_trip(
        (smsc_v, smsc_toa, smsc_want) in smsc(),
        orig in dest(),
        [mms, lp, sri, rp] in flags(),
        pid in any::<u8>(),
        scts_raw in ts_raw(),
        (dcs, udh, payload) in ud_parts(),
        tail in vec(any::<u8>(), 0..3),
    ) {
        let (udl, ud, want_ud) = user_data(dcs, &udh, &payload);
        let fo = u8::from(mms) << 2 | u8::from(lp) << 3 | u8::from(sri) << 5
            | u8::from(udh.is_some()) << 6 | u8::from(rp) << 7;
        let mut b = encode_smsc(smsc_v.as_deref(), smsc_toa).unwrap();
        b.push(fo);
        b.extend(encode_address(&orig.value, orig.toa).unwrap());
        b.extend([pid, dcs]);
        b.extend(scts_raw);
        b.push(udl);
        b.extend(ud);
        b.extend(tail);
        let want = Deliver {
            mms, lp, sri, udhi: udh.is_some(), rp,
            originator: orig.want,
            pid,
            dcs: Dcs::decode(dcs),
            scts: ts_model(&scts_raw),
            scts_raw,
            user_data: want_ud,
        };
        let p = pdu::decode_hex(&hex_encode(&b)).unwrap();
        prop_assert_eq!(p.smsc, smsc_want);
        prop_assert_eq!(p.tpdu, Tpdu::Deliver(want));
    }

    /// SMS-STATUS-REPORT with every optional field; any cut of the optional tail keeps
    /// the mandatory fields, and what the cut leaves readable is what was sent.
    #[test]
    fn status_report_round_trip_and_cut_tail(
        rcpt in dest(),
        [mms, lp, srq, udhi] in flags(),
        mr in any::<u8>(),
        scts_raw in ts_raw(),
        dt_raw in ts_raw(),
        status in any::<u8>(),
        pi in proptest::option::of(0u8..8),
        pid in any::<u8>(),
        (dcs, udh, payload) in ud_parts(),
        cut in any::<usize>(),
    ) {
        // TP-UDHI promises a header: an empty one when the generator drew none. Its UDHL
        // octet (2 septets) was not reserved by the generator: trim the payload to fit.
        let payload = if udhi && udh.is_none() {
            let d = Dcs::decode(dcs);
            let max = match (d.alphabet, d.compressed) {
                (Alphabet::Gsm7, false) => 158,
                (Alphabet::Ucs2, false) => 138,
                _ => 139,
            };
            payload.into_iter().take(max).collect()
        } else {
            payload
        };
        let udh = if udhi { Some(udh.unwrap_or_default()) } else { None };
        let fo = 0x02 | u8::from(mms) << 2 | u8::from(lp) << 3 | u8::from(srq) << 5 | u8::from(udhi) << 6;
        let mut b = vec![fo, mr];
        b.extend(encode_address(&rcpt.value, rcpt.toa).unwrap());
        b.extend(scts_raw);
        b.extend(dt_raw);
        b.push(status);
        let head = b.len();
        let mut want = StatusReport {
            mms, lp, srq, udhi, mr,
            recipient: rcpt.want,
            scts: ts_model(&scts_raw), scts_raw,
            discharge: ts_model(&dt_raw), discharge_raw: dt_raw,
            status, pi, pid: None, dcs: None, user_data: None,
        };
        if let Some(pi) = pi {
            b.push(pi);
            if pi & 1 != 0 {
                b.push(pid);
                want.pid = Some(pid);
            }
            let dcs = if pi & 2 != 0 {
                b.push(dcs);
                want.dcs = Some(Dcs::decode(dcs));
                dcs
            } else {
                0
            };
            if pi & 4 != 0 {
                // Without TP-DCS the user data is GSM 7-bit: keep the payload septets.
                let payload: Vec<u8> = if pi & 2 != 0 { payload } else { payload.iter().map(|s| s & 0x7F).take(150).collect() };
                let (udl, ud, ud_want) = user_data(dcs, &udh, &payload);
                b.push(udl);
                b.extend(ud);
                want.user_data = Some(ud_want);
            }
        }
        prop_assert_eq!(decode_tpdu(&b), Ok(Tpdu::StatusReport(want.clone())));
        let k = head + cut % (b.len() - head + 1);
        let Ok(Tpdu::StatusReport(r)) = decode_tpdu(&b[..k]) else {
            return Err(TestCaseError::fail(format!("cut at {k} rejected")));
        };
        let mandatory = |r: &StatusReport| (r.mms, r.lp, r.srq, r.udhi, r.mr, r.recipient.clone(),
            r.scts, r.scts_raw, r.discharge, r.discharge_raw, r.status);
        prop_assert_eq!(mandatory(&r), mandatory(&want));
        prop_assert!(r.pi.is_none() || r.pi == want.pi);
        prop_assert!(r.pid.is_none() || r.pid == want.pid);
        prop_assert!(r.dcs.is_none() || r.dcs == want.dcs);
        prop_assert!(r.user_data.is_none() || r.user_data == want.user_data);
    }
}

// ------------------------------------------------------------------ SMS-SUBMIT

fn langs() -> impl Strategy<Value = Vec<Language>> {
    proptest::sample::subsequence(LANGS.to_vec(), 0..=4)
}

fn body() -> impl Strategy<Value = Body> {
    prop_oneof![
        4 => sms_text(400).prop_map(Body::Text),
        1 => vec(any::<u8>(), 0..400).prop_map(Body::Binary),
    ]
}

prop_compose! {
    fn submit_request()(
        d in dest(),
        (smsc_v, smsc_toa, _) in smsc(),
        body in body(),
        alphabet in prop_oneof![6 => Just(None), 1 => Just(Some(Alphabet::Gsm7)),
            1 => Just(Some(Alphabet::Ucs2)), 1 => Just(Some(Alphabet::Bit8))],
        (locking_shift, single_shift) in (langs(), langs()),
        (class, pid, message_ref) in (proptest::option::of(0u8..4), any::<u8>(), any::<u8>()),
        [status_report, reject_duplicates, reply_path, concat_16bit] in flags(),
        validity in validity(),
        concat_ref in prop_oneof![1 => Just(None), 4 => any::<u16>().prop_map(Some)],
    ) -> SubmitRequest {
        SubmitRequest {
            smsc: smsc_v, smsc_toa,
            destination: d.value,
            destination_toa: d.toa,
            body, alphabet, locking_shift, single_shift, class, pid, message_ref,
            status_report, reject_duplicates, reply_path, validity, concat_ref, concat_16bit,
        }
    }
}

/// The alphabet and tables a request must be encoded with (its documented rule), or
/// `None` when the request cannot be encoded.
fn expected_alphabet(req: &SubmitRequest) -> Option<(Alphabet, Tables)> {
    let text = match &req.body {
        Body::Binary(_) => {
            return matches!(req.alphabet, None | Some(Alphabet::Bit8))
                .then_some((Alphabet::Bit8, Tables::DEFAULT))
        }
        Body::Text(t) => t,
    };
    let gsm = gsm7::choose_tables(text, &req.locking_shift, &req.single_shift);
    match req.alphabet {
        None => Some(gsm.map_or((Alphabet::Ucs2, Tables::DEFAULT), |t| (Alphabet::Gsm7, t))),
        Some(Alphabet::Gsm7) => gsm.map(|t| (Alphabet::Gsm7, t)),
        Some(Alphabet::Ucs2) => Some((Alphabet::Ucs2, Tables::DEFAULT)),
        Some(Alphabet::Bit8) => None,
    }
}

fn capacity(alphabet: Alphabet, udh_len: usize) -> usize {
    match alphabet {
        Alphabet::Gsm7 => 160 - (udh_len * 8).div_ceil(7),
        _ => 140 - udh_len,
    }
}

/// Size of the first unit (character or octet) of a part's payload.
fn first_unit(alphabet: Alphabet, payload: &[u8]) -> usize {
    match (alphabet, payload.first()) {
        (Alphabet::Gsm7, Some(&ESC)) => 2,
        (Alphabet::Ucs2, Some(0xD8..=0xDB)) => 4,
        (Alphabet::Ucs2, _) => 2,
        _ => 1,
    }
}

fn check_submit(req: &SubmitRequest, order_seed: u64) -> Result<(), TestCaseError> {
    let Some((alphabet, tables)) = expected_alphabet(req) else {
        prop_assert!(encode_submit(req).is_err());
        return Ok(());
    };
    let reference = req.concat_ref.unwrap_or(0);
    let wide = req.concat_16bit || reference > 255;
    if req.concat_ref.is_none() {
        let n = encode_submit(&SubmitRequest {
            concat_ref: Some(0),
            ..req.clone()
        })
        .unwrap()
        .len();
        if n > 1 {
            prop_assert_eq!(encode_submit(req), Err(PduError::MissingConcatRef));
            return Ok(());
        }
    }
    let parts = encode_submit(req).unwrap();
    let n = parts.len();
    let national: Vec<(u8, Language)> = [
        (pdu::IEI_SINGLE_SHIFT, tables.single),
        (pdu::IEI_LOCKING_SHIFT, tables.locking),
    ]
    .into_iter()
    .filter(|(_, l)| *l != Language::Default)
    .collect();
    let smsc_want = encode_smsc(req.smsc.as_deref(), req.smsc_toa)
        .ok()
        .and_then(|b| decode_smsc(&b).ok())
        .and_then(|(a, _)| a);
    let dest_want = decode_address(&encode_address(&req.destination, req.destination_toa).unwrap())
        .unwrap()
        .0;
    let mut tpdus = Vec::new();
    let mut total = 0;
    for (i, (len, hex)) in parts.iter().enumerate() {
        let bytes = hex_decode(hex).unwrap();
        prop_assert_eq!(*len, bytes.len() - usize::from(bytes[0]) - 1);
        let p = pdu::decode(&bytes).unwrap();
        prop_assert_eq!(&p.smsc, &smsc_want);
        let Tpdu::Submit(s) = &p.tpdu else {
            return Err(TestCaseError::fail("not a SUBMIT"));
        };
        prop_assert_eq!(
            (s.rd, s.srr, s.rp, s.mr, s.pid),
            (
                req.reject_duplicates,
                req.status_report,
                req.reply_path,
                req.message_ref,
                req.pid
            )
        );
        prop_assert_eq!(&s.destination, &dest_want);
        prop_assert_eq!(
            (s.dcs.alphabet, s.dcs.class, s.dcs.compressed),
            (alphabet, req.class, false)
        );
        prop_assert_eq!(s.validity, req.validity);
        let ud = &s.user_data;
        prop_assert_eq!(s.udhi, ud.udh.is_some());
        let udh = ud.udh.clone().unwrap_or_default();
        prop_assert_eq!(udh.ies.len(), usize::from(n > 1) + national.len());
        if n > 1 {
            let c = udh.concat().unwrap();
            prop_assert_eq!(
                (c.reference, c.total, c.seq, c.ref_16bit),
                (reference, n as u8, i as u8 + 1, wide)
            );
        } else {
            prop_assert_eq!(udh.concat(), None);
        }
        prop_assert_eq!(udh.tables(), tables);
        for (id, l) in &national {
            let nli = [l.nli()];
            prop_assert_eq!(last(&udh.ies, *id), Some(&nli[..]));
        }
        // Parts are filled greedily: each holds what fits, and the next one's first
        // character would not have.
        let udh_len = ud.udh.as_ref().map_or(0, |u| u.encode().unwrap().len());
        let cap = capacity(alphabet, udh_len);
        prop_assert!(ud.payload.len() <= cap);
        total += ud.payload.len();
        if let Some((_, next)) = parts.get(i + 1) {
            let next = pdu::decode_hex(next).unwrap();
            let next = &next.tpdu.user_data().unwrap().payload;
            prop_assert!(
                ud.payload.len() + first_unit(alphabet, next) > cap,
                "part {} not full",
                i
            );
        }
        tpdus.push(p.tpdu);
    }
    if n > 1 {
        let single = if national.is_empty() {
            0
        } else {
            1 + 3 * national.len()
        };
        prop_assert!(total > capacity(alphabet, single), "split although it fits");
    }
    let mut asm = Reassembler::new();
    let mut done = None;
    for (k, &i) in permutation(n, order_seed).iter().enumerate() {
        let m = asm.push_tpdu(&tpdus[i], k as u64);
        prop_assert_eq!(m.is_some(), k + 1 == n, "completes with the last part only");
        done = done.or(m);
    }
    let m = done.unwrap();
    prop_assert_eq!(m.parts, n);
    prop_assert_eq!(m.alphabet, alphabet);
    match &req.body {
        Body::Text(t) => prop_assert_eq!(m.text.as_deref(), Some(t.as_str())),
        Body::Binary(b) => prop_assert_eq!((&m.payload, m.text.as_deref()), (b, None)),
    }
    Ok(())
}

proptest! {
    #![proptest_config(cases(128))]

    /// SMS-SUBMIT: every request field survives encode → decode, parts are split
    /// greedily between characters, and the parts reassemble in any order.
    #[test]
    fn submit_round_trip(req in submit_request(), order in any::<u64>()) {
        check_submit(&req, order)?;
    }
}

// ------------------------------------------------------------------ alphabets

/// Reference for [`gsm7::choose_tables`] from its documented rule.
fn choose_model(text: &str, locking: &[Language], single: &[Language]) -> Option<Tables> {
    if gsm7::is_encodable(text, Tables::DEFAULT) {
        return Some(Tables::DEFAULT);
    }
    let with_default = |l: &[Language]| {
        let mut v = vec![Language::Default];
        v.extend(l.iter().copied().filter(|&x| x != Language::Default));
        v
    };
    let mut best: Option<(usize, Tables)> = None;
    for l in with_default(locking)
        .into_iter()
        .filter(|l| l.has_locking_table())
    {
        for &s in &with_default(single) {
            let t = Tables::new(l, s);
            let ies = usize::from(l != Language::Default) + usize::from(s != Language::Default);
            if let Some(n) = gsm7::septet_len(text, t) {
                let cost = n + (3 * ies * 8).div_ceil(7);
                if best.is_none_or(|(c, _)| cost < c) {
                    best = Some((cost, t));
                }
            }
        }
    }
    best.map(|(_, t)| t)
}

/// Runs of table-dependent characters, so different pairs win by small margins.
fn national_runs() -> impl Strategy<Value = String> {
    vec((select(national_pool()), 1usize..13), 1..4)
        .prop_map(|runs| runs.iter().map(|(c, n)| c.to_string().repeat(*n)).collect())
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn gsm7_text_round_trip(t in tables(), picks in vec(any::<prop::sample::Index>(), 0..200)) {
        let pool = gsm_pool(t);
        let text: String = picks.iter().map(|i| *i.get(&pool)).collect();
        let septets = gsm7::encode(&text, t).unwrap();
        prop_assert_eq!(gsm7::septet_len(&text, t), Some(septets.len()));
        prop_assert!(gsm7::is_encodable(&text, t));
        prop_assert!(septets.iter().all(|&s| s < 0x80));
        prop_assert_eq!(gsm7::decode(&septets, t), text.clone());
        // Bit 7 is ignored.
        let high: Vec<u8> = septets.iter().map(|s| s | 0x80).collect();
        prop_assert_eq!(gsm7::decode(&high, t), text);
    }

    #[test]
    fn gsm7_unencodable_is_named(t in tables(), head in sms_text(20), c in any::<char>(), tail in sms_text(20)) {
        let first_bad = head.chars().find(|&x| gsm7::encode_char(x, t).is_none());
        let text = format!("{head}{c}{tail}");
        let want = first_bad.or(gsm7::encode_char(c, t).is_none().then_some(c));
        if let Some(bad) = want {
            prop_assert_eq!(gsm7::encode(&text, t), Err(bad));
            prop_assert_eq!(gsm7::septet_len(&text, t), None);
        }
    }

    #[test]
    fn gsm7_pack_round_trip(s in vec(0u8..0x80, 0..200), fill in 0u8..8, wrap in 0u8..4) {
        let packed = gsm7::pack(&s, fill);
        prop_assert_eq!(packed.len(), (usize::from(fill) + 7 * s.len()).div_ceil(8));
        prop_assert_eq!(gsm7::pack(&s, fill + 8 * wrap), packed.clone());
        prop_assert_eq!(gsm7::unpack(&packed, fill, s.len()), s.clone());
        let all = gsm7::unpack(&packed, fill + 8 * wrap, usize::MAX);
        prop_assert_eq!(all.get(..s.len()), Some(&s[..]));
        let fit = gsm7::septets_in(packed.len(), fill);
        prop_assert_eq!(fit, (packed.len() * 8 - usize::from(fill)) / 7);
        prop_assert!(fit == s.len() || fit == s.len() + 1);
        prop_assert_eq!(gsm7::septets_in(packed.len(), fill + 8 * wrap), fit);
    }

    #[test]
    fn choose_tables_matches_the_cost_rule(
        text in prop_oneof![national_runs(), sms_text(40)],
        locking in langs(),
        single in langs(),
    ) {
        let got = gsm7::choose_tables(&text, &locking, &single);
        prop_assert_eq!(got, choose_model(&text, &locking, &single));
        if let Some(t) = got {
            prop_assert!(gsm7::is_encodable(&text, t));
            let ies = usize::from(t.locking != Language::Default) + usize::from(t.single != Language::Default);
            prop_assert_eq!(t.udh_ie_len(), 3 * ies);
        }
    }

    #[test]
    fn ucs2_round_trip(s in any::<String>()) {
        let b = ucs2::encode(&s);
        prop_assert_eq!(b.len(), 2 * s.chars().map(ucs2::units).sum::<usize>());
        prop_assert_eq!(b.len(), 2 * s.encode_utf16().count());
        prop_assert_eq!(ucs2::decode(&b), s.clone());
        prop_assert_eq!(ucs2::decode_strict(&b), Ok(s));
    }

    /// Bad UTF-16 matches the standard library decoder: U+FFFD (lossy) or the index of
    /// the first unpaired surrogate (strict); an odd trailing octet is U+FFFD / an error.
    #[test]
    fn ucs2_bad_input(units in vec(prop_oneof![any::<u16>(), 0xD800u16..0xE000], 0..20), odd in proptest::option::of(any::<u8>())) {
        let mut b: Vec<u8> = units.iter().flat_map(|u| u.to_be_bytes()).collect();
        let lossy: String = char::decode_utf16(units.iter().copied())
            .map(|r| r.unwrap_or(char::REPLACEMENT_CHARACTER))
            .collect();
        let mut idx = 0;
        let mut strict = Ok(String::new());
        for r in char::decode_utf16(units.iter().copied()) {
            match r {
                Ok(c) => {
                    idx += c.len_utf16();
                    if let Ok(s) = &mut strict { s.push(c) }
                }
                Err(_) => { strict = Err(ucs2::Ucs2Error::UnpairedSurrogate(idx)); break; }
            }
        }
        if let Some(o) = odd {
            b.push(o);
            prop_assert_eq!(ucs2::decode(&b), format!("{lossy}\u{FFFD}"));
            prop_assert_eq!(ucs2::decode_strict(&b), Err(ucs2::Ucs2Error::OddLength));
        } else {
            prop_assert_eq!(ucs2::decode(&b), lossy);
            prop_assert_eq!(ucs2::decode_strict(&b), strict);
        }
    }
}

// ------------------------------------------------------------------ USSD

fn default_text(max: usize) -> impl Strategy<Value = String> {
    vec(select(default_pool()), 0..=max).prop_map(String::from_iter)
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
fn every_cbs_dcs_octet_matches_the_23038_table() {
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
    }
}

proptest! {
    #![proptest_config(cases(512))]

    /// The CR padding rule is invisible after unpacking, for every fill.
    #[test]
    fn ussd_septets_round_trip(s in vec(prop_oneof![0u8..0x80, Just(0x0D)], 0..100), fill in 0u8..8) {
        let packed = ussd::pack_septets(&s, fill);
        // 23.038 §6.1.2.3.1 is ambiguous for text that really ends in CR CR with the
        // first CR on an octet boundary: it packs like a single CR plus the added one.
        let n = s.len();
        let ambiguous = s.ends_with(&[0x0D, 0x0D]) && (usize::from(fill) + 7 * (n - 1)) % 8 == 0;
        if !ambiguous {
            prop_assert_eq!(ussd::unpack_septets(&packed, fill), s.clone());
        }
        let bits = usize::from(fill) + 7 * s.len();
        let padded = bits % 8 == 1 || (bits % 8 == 0 && s.last() == Some(&0x0D));
        if padded {
            prop_assert_eq!(packed, gsm7::pack(&[&s[..], &[0x0D]].concat(), fill));
        } else {
            prop_assert_eq!(packed, gsm7::pack(&s, fill));
        }
    }

    #[test]
    fn ussd_text_round_trip(t in default_text(200)) {
        prop_assert_eq!(ussd::decode_gsm7(&ussd::encode(&t).unwrap()), t);
    }

    /// A USSD string decodes per its DCS: packed GSM 7-bit (with a `xx\r` language
    /// prefix for 0x10), UCS2 (behind two packed language characters for 0x11),
    /// ISO-8859-1 for 8-bit; compressed data is refused.
    #[test]
    fn ussd_decode_follows_the_dcs(dcs in any::<u8>(), gsm in default_text(60), wide in any::<String>(), lang in chars_of("abcdefghij", 2..=2)) {
        let d = CbsDcs::decode(dcs);
        let latin: String = wide.chars().map(|c| char::from((c as u32 & 0xFF) as u8)).collect();
        let (payload, text, language) = match (d.alphabet, d.language_prefix) {
            _ if d.compressed => {
                prop_assert_eq!(ussd::decode(b"\x01\x02", dcs), None);
                return Ok(());
            }
            (Alphabet::Gsm7, true) => (ussd::encode(&format!("{lang}\r{gsm}")).unwrap(), gsm, Some(lang)),
            (Alphabet::Gsm7, false) => (ussd::encode(&gsm).unwrap(), gsm, d.language().map(String::from)),
            (Alphabet::Ucs2, true) => {
                let septets = gsm7::encode(&lang, Tables::DEFAULT).unwrap();
                ([gsm7::pack(&septets, 0), ucs2::encode(&wide)].concat(), wide, Some(lang))
            }
            (Alphabet::Ucs2, false) => (ucs2::encode(&wide), wide, d.language().map(String::from)),
            (Alphabet::Bit8, _) => (latin.chars().map(|c| c as u8).collect(), latin, None),
        };
        let got = ussd::decode(&payload, dcs).unwrap();
        prop_assert_eq!((got.text, got.language), (text, language));
    }
}

// ------------------------------------------------------------------ +CUSD

/// `<str>` in a `+CUSD` line: quoted with `\"` / `\\` escapes, or bare.
fn cusd_str() -> impl Strategy<Value = (String, String)> {
    prop_oneof![
        any::<String>().prop_map(|s| {
            let esc = s.replace('\\', "\\\\").replace('"', "\\\"");
            (format!("\"{esc}\""), s)
        }),
        chars_of("abcdefghij *#", 1..=20)
            .prop_map(|s| s.trim().to_string())
            .prop_filter("non-empty", |s| !s.is_empty())
            .prop_map(|s| (s.clone(), s)),
    ]
}

#[derive(Debug, Clone)]
enum DcsField {
    None,
    Num(u8),
    Bad(u16),
}

fn cusd_line() -> impl Strategy<Value = (String, Cusd)> {
    (
        any::<u8>(),
        proptest::option::of((
            cusd_str(),
            prop_oneof![
                Just(DcsField::None),
                any::<u8>().prop_map(DcsField::Num),
                (256u16..1000).prop_map(DcsField::Bad),
            ],
        )),
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
            let mut tail_ok = ok;
            if let Some(((field, raw), dcs)) = rest {
                // A bare `<str>` followed by a glued `OK` would read as text.
                if !field.starts_with('"') && matches!(dcs, DcsField::None) && ok != "\r\n" {
                    tail_ok = "";
                }
                line.push(',');
                line.push_str(&field);
                want.raw = Some(raw);
                match dcs {
                    DcsField::None => {}
                    DcsField::Num(d) => {
                        line.push_str(&format!(",{d}"));
                        want.dcs = Some(d);
                    }
                    DcsField::Bad(d) => {
                        line.push_str(&format!(", {d}"));
                        want.invalid_dcs = Some(d.to_string());
                    }
                }
            }
            line.push_str(tail_ok);
            (line, want)
        })
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn cusd_line_round_trip((line, want) in cusd_line()) {
        prop_assert_eq!(ussd::parse_cusd(&line), Some(want));
    }

    /// `<str>` is read per `<dcs>` and the TE character set (27.007 §7.15).
    #[test]
    fn cusd_text_follows_dcs_and_cscs(s in any::<String>(), gsm in default_text(40), m in 0u8..6) {
        let cusd = |raw: String, dcs: Option<u8>| Cusd {
            status: CusdStatus::from_code(m), raw: Some(raw), dcs, invalid_dcs: None,
        };
        prop_assert_eq!(cusd(s.clone(), None).text(), Some(s.clone()));
        prop_assert_eq!(cusd(s.clone(), Some(0x0F)).text_as(Cscs::Ira), Some(s.clone()));
        prop_assert_eq!(cusd(hex_encode(&ucs2::encode(&s)), Some(0x0F)).text_as(Cscs::Ucs2), Some(s.clone()));
        let septets = gsm7::encode(&gsm, Tables::DEFAULT).unwrap();
        prop_assert_eq!(cusd(hex_encode(&septets), None).text_as(Cscs::Hex), Some(gsm.clone()));
        // UCS2 / 8-bit DCS: hex octets whatever the character set.
        for cs in [Cscs::Gsm, Cscs::Ucs2, Cscs::Hex] {
            prop_assert_eq!(cusd(hex_encode(&ucs2::encode(&s)), Some(0x48)).text_as(cs), Some(s.clone()));
        }
        let packed = hex_encode(&ussd::encode(&gsm).unwrap());
        prop_assert_eq!(cusd(packed, Some(0x0F)).text_packed_hex(), Some(gsm));
        prop_assert_eq!(cusd(s.clone(), Some(0x60)).text(), None);
    }
}

// ------------------------------------------------------------------ robustness

proptest! {
    #![proptest_config(cases(256))]

    #[test]
    fn decoders_never_panic(bytes in vec(any::<u8>(), 0..400)) {
        if let Ok(p) = pdu::decode(&bytes) {
            if let Some(ud) = p.tpdu.user_data() {
                prop_assert!(ud.payload.len() <= 160);
            }
        }
        let _ = decode_tpdu(&bytes);
        let _ = pdu::decode_hex(&hex_encode(&bytes));
        let _ = decode_address(&bytes);
        let _ = decode_smsc(&bytes);
        let _ = Udh::parse(&bytes);
        for udhi in [false, true] {
            for dcs in [0x00, 0x04, 0x08, 0x24, 0xF0] {
                if let Some((&udl, ud)) = bytes.split_first() {
                    let _ = pdu::decode_user_data(udl, ud, udhi, &Dcs::decode(dcs));
                }
            }
        }
        let _ = gsm7::decode(&bytes, Tables::new(Language::Turkish, Language::Portuguese));
        prop_assert!(gsm7::unpack(&bytes, bytes.first().copied().unwrap_or(0), 500).len() <= bytes.len() * 8 / 7);
        let _ = ucs2::decode(&bytes);
        let _ = ucs2::decode_strict(&bytes);
        let _ = ussd::unpack_septets(&bytes, 3);
        let _ = ussd::decode(&bytes, bytes.first().copied().unwrap_or(0));
        if bytes.len() >= 7 {
            let _ = Timestamp::decode(&bytes[..7].try_into().unwrap());
        }
    }

    #[test]
    fn text_parsers_never_panic(s in ".{0,200}", m in 0u8..10, dcs in any::<u8>()) {
        let _ = pdu::decode_hex(&s);
        let _ = ussd::parse_cusd(&s);
        let line = format!("+CUSD: {m},{s},{dcs}");
        if let Some(c) = ussd::parse_cusd(&line) {
            let _ = c.text();
            let _ = c.text_packed_hex();
        }
        let line = format!("+cusd: {m},\"{s}\",{dcs}OK");
        if let Some(c) = ussd::parse_cusd(&line) {
            for cs in [Cscs::Gsm, Cscs::Ucs2, Cscs::Hex] {
                let _ = c.text_as(cs);
            }
            let _ = c.text_packed_hex();
        }
    }
}

// ------------------------------------------------------------------ stories

/// Given a destination that is empty or a lone `+`
/// When it is encoded alone or as part of a SUBMIT
/// Then the result is an `Invalid` error, never an empty address field
#[test]
fn empty_destination_is_rejected() {
    for to in ["", "+"] {
        assert!(matches!(
            encode_address(to, None),
            Err(PduError::Invalid(_))
        ));
        assert!(matches!(
            encode_submit(&SubmitRequest::new(to, "hi")),
            Err(PduError::Invalid(_))
        ));
    }
    assert!(matches!(
        encode_address("", Some(0xD0)),
        Err(PduError::Invalid(_))
    ));
}

/// Given an SMS-STATUS-REPORT whose TP-PI keeps its extension bit set to the end of
/// the data, or announces fields that are missing
/// When it is decoded
/// Then the mandatory fields stand and the unreadable optional ones are `None`
#[test]
fn status_report_pi_extension_to_the_end() {
    let head = [
        &[0x06, 0x07, 0x04, 0x91, 0x21, 0x43][..],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x65, 0x00],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x75, 0x00],
        &[0x00],
    ]
    .concat();
    for tail in [&[0xFF][..], &[0x87, 0x87], &[0x07], &[0x07, 0x00]] {
        let Ok(Tpdu::StatusReport(r)) = decode_tpdu(&[&head[..], tail].concat()) else {
            panic!("{tail:02X?}")
        };
        assert_eq!((r.mr, r.status, r.pi), (7, 0, Some(tail[0])), "{tail:02X?}");
        assert!(r.scts.is_some() && r.discharge.is_some());
        assert!(r.dcs.is_none() && r.user_data.is_none());
    }
}
