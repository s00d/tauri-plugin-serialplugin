//! Encode → decode round trips for the cases earlier implementations got wrong, plus
//! property tests that no input makes a decoder panic.

use proptest::prelude::*;
use serial_codec::sms::concat::{Fragment, Key, Kind, Reassembler};
use serial_codec::sms::gsm7::{self, Language, Tables, ESC};
use serial_codec::sms::pdu::{
    decode, decode_hex, decode_tpdu, encode_address, encode_submit, relative_from_minutes,
    relative_minutes, Alphabet, Body, Pdu, SubmitRequest, Timestamp, Tpdu, Validity, IEI_CONCAT_16,
    IEI_CONCAT_8, IEI_LOCKING_SHIFT, IEI_SINGLE_SHIFT,
};
use serial_codec::sms::{hex_encode, ucs2};
use serial_codec::ussd;

/// Encode (with reference 1 when the request has none) and decode every part.
fn parts(req: &SubmitRequest) -> Vec<Pdu> {
    let mut req = req.clone();
    req.concat_ref.get_or_insert(1);
    encode_submit(&req)
        .unwrap()
        .iter()
        .map(|(len, hex)| {
            let p = decode_hex(hex).unwrap();
            // tpdu_len excludes the SMSC prefix.
            let smsc = usize::from(u8::from_str_radix(&hex[..2], 16).unwrap()) + 1;
            assert_eq!(*len, hex.len() / 2 - smsc);
            p
        })
        .collect()
}

fn reassemble(ps: &[Pdu]) -> String {
    let mut asm = Reassembler::new();
    let mut out = None;
    for (n, p) in ps.iter().enumerate() {
        out = asm.push_tpdu(&p.tpdu, n as u64);
        assert_eq!(
            out.is_some(),
            n + 1 == ps.len(),
            "completes on the last part only"
        );
    }
    out.unwrap().text.unwrap()
}

fn submit(p: &Pdu) -> &serial_codec::sms::pdu::Submit {
    match &p.tpdu {
        Tpdu::Submit(s) => s,
        t => panic!("not a submit: {t:?}"),
    }
}

#[test]
fn ref_above_255_switches_to_16bit_ie_with_correct_fill() {
    let text = "x".repeat(400);
    let mut req = SubmitRequest::new("+15551234567", text.clone());
    req.concat_ref = Some(300);
    let ps = parts(&req);
    // 16-bit IE: 7-octet UDH, 8 header septets, 152 text septets per part.
    assert_eq!(ps.len(), 3);
    for (n, p) in ps.iter().enumerate() {
        let ud = &submit(p).user_data;
        let udh = ud.udh.as_ref().unwrap();
        assert_eq!(udh.ies[0].id, IEI_CONCAT_16);
        let c = udh.concat().unwrap();
        assert_eq!((c.reference, c.total, c.seq), (300, 3, n as u8 + 1));
        assert_eq!(ud.payload.len(), if n < 2 { 152 } else { 96 });
    }
    assert_eq!(reassemble(&ps), text);

    // UCS2 with a 16-bit ref: 66 characters per part.
    let text = "Ж".repeat(140);
    req.body = Body::Text(text.clone());
    let ps = parts(&req);
    assert_eq!(ps.len(), 3);
    assert_eq!(submit(&ps[0]).user_data.payload.len(), 132);
    assert_eq!(reassemble(&ps), text);

    // 8-bit ref by default, 16-bit when asked even for small refs.
    req.concat_ref = Some(7);
    assert_eq!(
        submit(&parts(&req)[0]).user_data.udh.as_ref().unwrap().ies[0].id,
        IEI_CONCAT_8
    );
    req.concat_16bit = true;
    assert_eq!(
        submit(&parts(&req)[0]).user_data.udh.as_ref().unwrap().ies[0].id,
        IEI_CONCAT_16
    );
}

#[test]
fn escape_pair_never_split() {
    // 152 plain septets, then '€' (ESC 0x65): it would straddle the 153-septet boundary.
    let text = format!("{}€{}", "a".repeat(152), "b".repeat(10));
    let ps = parts(&SubmitRequest::new("123", text.clone()));
    assert_eq!(ps.len(), 2);
    let first = &submit(&ps[0]).user_data.payload;
    assert_eq!(first.len(), 152);
    assert_ne!(first.last(), Some(&ESC));
    assert_eq!(submit(&ps[1]).user_data.payload[..2], [ESC, 0x65]);
    assert_eq!(reassemble(&ps), text);
}

#[test]
fn surrogate_pair_never_split() {
    // UCS2 (Cyrillic forces it): 66 units, then an emoji (2 units) at the 67-unit boundary.
    let text = format!("{}😁{}", "Ж".repeat(66), "Ж".repeat(5));
    let ps = parts(&SubmitRequest::new("123", text.clone()));
    assert_eq!(ps.len(), 2);
    for p in &ps {
        let t = p.tpdu.text().unwrap();
        assert!(!t.contains(char::REPLACEMENT_CHARACTER), "{t:?}");
    }
    assert_eq!(submit(&ps[0]).user_data.payload.len(), 132);
    assert_eq!(reassemble(&ps), text);
}

#[test]
fn surrogate_split_by_sender_is_joined_as_bytes() {
    // Another sender split a surrogate pair across parts: join octets, then decode.
    let b = ucs2::encode("a😁b");
    let key = Key {
        kind: Kind::Deliver,
        address: "+1".into(),
        reference: 9,
        ref_16bit: false,
        total: 2,
    };
    let frag = |payload: &[u8]| Fragment {
        alphabet: Alphabet::Ucs2,
        tables: Tables::DEFAULT,
        payload: payload.to_vec(),
    };
    let mut asm = Reassembler::new();
    assert!(asm.push(key.clone(), 2, frag(&b[4..]), 5).is_none()); // out of order
    let m = asm.push(key, 1, frag(&b[..4]), 6).unwrap();
    assert_eq!(m.text.as_deref(), Some("a😁b"));
}

#[test]
fn national_tables_avoid_ucs2() {
    let text = "Ağaç işçi şoför"; // ğ ç ş ı are not in the default alphabet
    let mut req = SubmitRequest::new("+90", text);
    // Without national tables: UCS2.
    assert_eq!(parts(&req)[0].tpdu.dcs().unwrap().alphabet, Alphabet::Ucs2);
    req.single_shift = vec![Language::Turkish];
    let p = &parts(&req)[0];
    let ud = p.tpdu.user_data().unwrap();
    assert_eq!(p.tpdu.dcs().unwrap().alphabet, Alphabet::Gsm7);
    assert_eq!(
        ud.udh.as_ref().unwrap().national_languages(),
        (Some(1), None)
    );
    assert_eq!(p.tpdu.text(), Some(text));
    req.locking_shift = vec![Language::Turkish];
    let p = &parts(&req)[0];
    let ies: Vec<u8> = p
        .tpdu
        .user_data()
        .unwrap()
        .udh
        .as_ref()
        .unwrap()
        .ies
        .iter()
        .map(|i| i.id)
        .collect();
    assert!(ies.contains(&IEI_LOCKING_SHIFT), "{ies:?}");
    assert_eq!(p.tpdu.text(), Some(text));

    // Long Portuguese text: concat IE + national IEs, fill bits after a 12-octet UDH.
    let text = "Atenção: ação já é válida. ".repeat(12);
    let mut req = SubmitRequest::new("+351", text.clone());
    req.locking_shift = vec![Language::Portuguese];
    req.single_shift = vec![Language::Portuguese];
    req.concat_ref = Some(1000);
    let ps = parts(&req);
    assert!(ps.len() > 1);
    let udh = ps[0].tpdu.user_data().unwrap().udh.as_ref().unwrap();
    assert!(udh
        .ies
        .iter()
        .any(|i| i.id == IEI_SINGLE_SHIFT || i.id == IEI_LOCKING_SHIFT));
    assert_eq!(ps[0].tpdu.dcs().unwrap().alphabet, Alphabet::Gsm7);
    assert_eq!(reassemble(&ps), text);
}

#[test]
fn address_length_counts_digits_not_plus() {
    assert_eq!(
        hex_encode(&encode_address("+123", None).unwrap()),
        "039121F3"
    );
    assert_eq!(
        hex_encode(&encode_address("123", None).unwrap()),
        "038121F3"
    );
    // Alphanumeric sender.
    let a = encode_address("Info", None).unwrap();
    assert_eq!(a[1], 0xD0);
    let (addr, _) = serial_codec::sms::pdu::decode_address(&a).unwrap();
    assert_eq!(addr.value, "Info");
    assert!(encode_address("+123456789012345678901", None).is_err());
}

#[test]
fn binary_body_splits_by_octets() {
    let data: Vec<u8> = (0..=255u8).chain(0..44).collect(); // 300 octets
    let mut req = SubmitRequest::new("+1", "");
    req.body = Body::Binary(data.clone());
    let ps = parts(&req);
    assert_eq!(ps.len(), 3); // 134 octets per part with a 6-octet UDH
    let mut asm = Reassembler::new();
    let m = ps.iter().find_map(|p| asm.push_tpdu(&p.tpdu, 0)).unwrap();
    assert_eq!(m.payload, data);
    assert_eq!(m.text, None);
}

#[test]
fn submit_fields_round_trip() {
    let mut req = SubmitRequest::new("+447700900123", "hi");
    req.smsc = Some("+447700900000".into());
    req.message_ref = 42;
    req.status_report = true;
    req.reject_duplicates = true;
    req.class = Some(0);
    let ts = Timestamp {
        year: 2026,
        month: 11,
        day: 8,
        hour: 23,
        minute: 59,
        second: 1,
        tz_quarters: -14,
    };
    for v in [Validity::Relative(167), Validity::Absolute(ts)] {
        req.validity = Some(v);
        let p = &parts(&req)[0];
        let s = submit(p);
        assert_eq!(p.smsc.as_ref().unwrap().to_string(), "+447700900000");
        assert_eq!(s.destination.to_string(), "+447700900123");
        assert_eq!((s.mr, s.srr, s.rd, s.dcs.class), (42, true, true, Some(0)));
        assert_eq!(s.validity, Some(v));
        assert_eq!(p.tpdu.text(), Some("hi"));
    }
}

#[test]
fn timestamps_and_validity() {
    let b = [0x11, 0x10, 0x10, 0x21, 0x43, 0x65, 0x4A]; // 2011-01-01 12:34:56 -06:00
    let t = Timestamp::decode(&b).unwrap();
    assert_eq!(t.tz_quarters, -24);
    assert_eq!(t.to_string(), "2011-01-01T12:34:56-06:00");
    assert_eq!(t.unix_utc(), 1_293_906_896);
    assert_eq!(t.encode(), Some(b));
    // Invalid dates decode as None, never "now".
    assert!(Timestamp::decode(&[0x11, 0x20, 0x13, 0, 0, 0, 0]).is_none()); // 2011-02-31
    assert!(Timestamp::decode(&[0x00; 7]).is_none()); // month 0
    assert!(Timestamp::decode(&[0xFF; 7]).is_none());
    for vp in 0..=255u8 {
        assert_eq!(relative_from_minutes(relative_minutes(vp)), Some(vp));
    }
    assert_eq!(relative_from_minutes(63 * 7 * 1440 + 1), None);
}

#[test]
fn status_report_with_parameter_indicator() {
    // MR 7, RA +1234, SCTS, DT, ST 0, PI = PID|DCS|UDL, PID 0, DCS 0, UDL 2 "hi".
    let tpdu = [
        &[0x06, 0x07, 0x04, 0x91, 0x21, 0x43][..],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x65, 0x00],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x75, 0x00],
        &[0x00, 0x07, 0x00, 0x00, 0x02, 0xE8, 0x34],
    ]
    .concat();
    let Tpdu::StatusReport(r) = decode_tpdu(&tpdu).unwrap() else {
        panic!()
    };
    assert_eq!((r.mr, r.status, r.pi, r.pid), (7, 0, Some(7), Some(0)));
    assert_eq!(r.recipient.to_string(), "+1234");
    assert_eq!(
        r.discharge.unwrap().unix_utc() - r.scts.unwrap().unix_utc(),
        1
    );
    assert_eq!(r.user_data.unwrap().text.as_deref(), Some("hi"));
    // A missing PI is fine; so is a truncated optional tail (23.040 §9.2.3.27): the
    // mandatory fields stay, what could not be read is None.
    assert!(decode_tpdu(&tpdu[..21]).is_ok());
    let Tpdu::StatusReport(r) = decode_tpdu(&tpdu[..tpdu.len() - 1]).unwrap() else {
        panic!()
    };
    assert_eq!((r.mr, r.status, r.pi, r.pid), (7, 0, Some(7), Some(0)));
    assert!(r.user_data.is_none());
}

#[test]
fn status_report_ignores_unparseable_pi_tail() {
    let head = [
        &[0x06, 0x07, 0x04, 0x91, 0x21, 0x43][..],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x65, 0x00],
        &[0x62, 0x11, 0x80, 0x21, 0x43, 0x75, 0x00],
        &[0x00],
    ]
    .concat();
    // PI extension bit set to the end of the data; PI announcing PID/DCS/UDL that are
    // missing.
    for tail in [&[0xFF][..], &[0x87, 0x87], &[0x07], &[0x07, 0x00]] {
        let tpdu = [&head[..], tail].concat();
        let Tpdu::StatusReport(r) = decode_tpdu(&tpdu).unwrap() else {
            panic!()
        };
        assert_eq!((r.mr, r.status, r.pi), (7, 0, Some(tail[0])), "{tail:02X?}");
        assert_eq!(r.recipient.to_string(), "+1234");
        assert!(r.scts.is_some() && r.discharge.is_some());
        assert!(r.dcs.is_none() && r.user_data.is_none());
    }
}

#[test]
fn reassembler_is_memory_bounded() {
    let key = |reference| Key {
        kind: Kind::Deliver,
        address: "+1".into(),
        reference,
        ref_16bit: true,
        total: 255,
    };
    let f = || Fragment {
        alphabet: Alphabet::Gsm7,
        tables: Tables::DEFAULT,
        payload: vec![0x41; 153],
    };
    let mut asm = Reassembler::new();
    assert_eq!(
        asm.max_pending(),
        serial_codec::sms::concat::DEFAULT_MAX_PENDING
    );
    for r in 0..10_000u16 {
        asm.push(key(r), 1, f(), u64::from(r));
        assert!(asm.pending() <= asm.max_pending());
    }
    assert_eq!(asm.pending(), asm.max_pending());

    // Oldest first: with a cap of 2, a third message evicts the one touched least
    // recently, even if it started first.
    let k2 = |reference| Key {
        total: 2,
        ..key(reference)
    };
    let mut asm = Reassembler::with_max_pending(2);
    asm.push(k2(1), 1, f(), 1);
    asm.push(k2(2), 1, f(), 2);
    asm.push(k2(1), 1, f(), 3); // refresh 1: now 2 is the oldest
    asm.push(k2(3), 1, f(), 4); // evicts 2
    assert_eq!(asm.pending(), 2);
    assert!(asm.push(k2(2), 2, f(), 5).is_none()); // 2 restarts (evicting 1)…
    assert!(asm.push(k2(3), 2, f(), 6).is_some()); // …3 is intact
    assert!(asm.push(k2(1), 2, f(), 7).is_none()); // 1 was evicted
                                                   // Equal timestamps: arrival order decides.
    let mut asm = Reassembler::with_max_pending(1);
    asm.push(k2(1), 1, f(), 0);
    asm.push(k2(2), 1, f(), 0);
    assert!(asm.push(k2(2), 2, f(), 0).is_some());
    assert_eq!(Reassembler::with_max_pending(0).max_pending(), 1);
}

#[test]
fn reassembler_separates_8bit_and_16bit_refs() {
    let text = "x".repeat(200);
    let mut req = SubmitRequest::new("+15551234567", text.clone());
    req.concat_ref = Some(12);
    let p8 = parts(&req);
    req.concat_16bit = true;
    let p16 = parts(&req);
    assert_eq!((p8.len(), p16.len()), (2, 2));
    let mut asm = Reassembler::new();
    assert!(asm.push_tpdu(&p8[0].tpdu, 0).is_none());
    assert!(asm.push_tpdu(&p16[1].tpdu, 0).is_none()); // not part 2 of the 8-bit one
    assert_eq!(asm.pending(), 2);
    let m = asm.push_tpdu(&p8[1].tpdu, 0).unwrap();
    assert_eq!(m.text.as_deref(), Some(text.as_str()));
    assert!(!m.key.unwrap().ref_16bit);
}

#[test]
fn multipart_needs_a_concat_ref() {
    use serial_codec::sms::pdu::PduError;
    let mut req = SubmitRequest::new("+15551234567", "x".repeat(161));
    assert_eq!(encode_submit(&req), Err(PduError::MissingConcatRef));
    // One part needs none.
    assert!(encode_submit(&SubmitRequest::new("+15551234567", "x".repeat(160))).is_ok());
    req.concat_ref = Some(0);
    assert_eq!(encode_submit(&req).unwrap().len(), 2);
}

#[test]
fn empty_destination_is_rejected() {
    use serial_codec::sms::pdu::PduError;
    for to in ["", "+"] {
        assert_eq!(encode_address(to, None), Err(PduError::Invalid("address")));
        assert_eq!(
            encode_submit(&SubmitRequest::new(to, "hi")),
            Err(PduError::Invalid("address"))
        );
    }
    assert_eq!(
        encode_address("", Some(0xD0)),
        Err(PduError::Invalid("address"))
    );
}

#[test]
fn udh_encode_rejects_lengths_over_255() {
    use serial_codec::sms::pdu::{Ie, PduError, Udh};
    let ie = |n| Ie {
        id: 0x70,
        data: vec![0; n],
    };
    let ok = Udh { ies: vec![ie(253)] }.encode().unwrap();
    assert_eq!((ok[0], ok[2], ok.len()), (255, 253, 256));
    assert_eq!(
        Udh { ies: vec![ie(300)] }.encode(),
        Err(PduError::Invalid("UDH length"))
    );
    assert_eq!(
        Udh {
            ies: vec![ie(200), ie(100)]
        }
        .encode(),
        Err(PduError::Invalid("UDH length"))
    );
}

#[test]
fn reassembler_keys_and_expiry() {
    let mut asm = Reassembler::new();
    let key = |kind, address: &str| Key {
        kind,
        address: address.into(),
        reference: 1,
        ref_16bit: false,
        total: 2,
    };
    let f = |s: &str| Fragment {
        alphabet: Alphabet::Gsm7,
        tables: Tables::DEFAULT,
        payload: gsm7::encode(s, Tables::DEFAULT).unwrap(),
    };
    assert!(asm.push(key(Kind::Deliver, "+1"), 1, f("a"), 10).is_none());
    assert!(asm.push(key(Kind::Submit, "+1"), 2, f("x"), 10).is_none());
    assert!(asm.push(key(Kind::Deliver, "+2"), 2, f("y"), 10).is_none());
    assert!(asm.push(key(Kind::Deliver, "+1"), 3, f("z"), 10).is_none()); // seq > total
    assert_eq!(asm.pending(), 3);
    assert_eq!(asm.expire_before(10).len(), 0);
    assert!(asm.push(key(Kind::Deliver, "+2"), 1, f("w"), 20).is_some());
    assert_eq!(asm.expire_before(11).len(), 2);
    assert_eq!(asm.pending(), 0);
}

#[test]
fn ussd_cr_rule() {
    for text in ["", "1234567", "12345678", "1234567\r", "abc"] {
        let packed = ussd::encode(text).unwrap();
        assert_eq!(ussd::decode_gsm7(&packed), text, "{text:?}");
    }
    assert_eq!(
        hex_encode(&ussd::encode("1234567").unwrap()),
        "31D98C56B3DD1A"
    );
}

proptest! {
    #[test]
    fn decoders_never_panic(bytes in proptest::collection::vec(any::<u8>(), 0..400)) {
        let _ = decode(&bytes);
        let _ = decode_tpdu(&bytes);
        let _ = decode_hex(&hex_encode(&bytes));
        let _ = serial_codec::sms::pdu::decode_address(&bytes);
        let _ = serial_codec::sms::pdu::decode_smsc(&bytes);
        let _ = serial_codec::sms::pdu::Udh::parse(&bytes);
        for udhi in [false, true] {
            for dcs in [0x00, 0x04, 0x08, 0x24, 0xF0] {
                if let Some((&udl, ud)) = bytes.split_first() {
                    let _ = serial_codec::sms::pdu::decode_user_data(
                        udl, ud, udhi, &serial_codec::sms::pdu::Dcs::decode(dcs));
                }
            }
        }
        let _ = gsm7::decode(&bytes, Tables::new(Language::Turkish, Language::Portuguese));
        let _ = gsm7::unpack(&bytes, bytes.first().copied().unwrap_or(0), 500);
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
        let _ = decode_hex(&s);
        let _ = ussd::parse_cusd(&s);
        let line = format!("+CUSD: {m},{s},{dcs}");
        if let Some(c) = ussd::parse_cusd(&line) {
            let _ = c.text();
            let _ = c.text_packed_hex();
        }
        let line = format!("+cusd: {m},\"{s}\",{dcs}OK");
        if let Some(c) = ussd::parse_cusd(&line) {
            for cs in [ussd::Cscs::Gsm, ussd::Cscs::Ucs2, ussd::Cscs::Hex] {
                let _ = c.text_as(cs);
            }
            let _ = c.text_packed_hex();
        }
    }

    #[test]
    fn any_text_round_trips(s in "\\PC{0,400}", r in any::<u16>(), turkish in any::<bool>()) {
        let mut req = SubmitRequest::new("+15550001111", s.clone());
        req.concat_ref = Some(r);
        if turkish {
            req.single_shift = vec![Language::Turkish];
            req.locking_shift = vec![Language::Turkish];
        }
        let ps = parts(&req);
        let text = if ps.len() == 1 {
            ps[0].tpdu.text().unwrap().to_string()
        } else {
            reassemble(&ps)
        };
        prop_assert_eq!(text, s);
    }
}
