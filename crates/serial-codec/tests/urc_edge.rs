//! Hand-written edge cases for `serial_codec::urc` (lines written from 3GPP 27.007 / 27.005),
//! plus a property test that arbitrary lines never panic.

use proptest::prelude::*;
use serial_codec::urc::*;

#[test]
fn registration_forms_and_extras() {
    // Response with cause fields (27.007 7.2: ...,<AcT>,<cause_type>,<reject_cause>)
    let r = Registration::parse("+creg: 3,3,\"00C3\",\"0000A13B\",7,0,15").unwrap();
    assert_eq!(r.kind, RegKind::Creg);
    assert_eq!(r.n, Some(3));
    assert_eq!(r.stat, RegStatus::Denied);
    assert_eq!(
        (r.lac, r.ci, r.act),
        (Some(0xC3), Some(0xA13B), Some(AccessTech::Eutran))
    );
    assert_eq!((r.cause_type, r.reject_cause), (Some(0), Some(15)));
    // +CGREG URC with <rac> after <AcT>
    let r = Registration::parse("+CGREG: 5,\"0502\",\"0404736D\",2,\"3F\"").unwrap();
    assert_eq!((r.n, r.stat, r.rac), (None, RegStatus::Roaming, Some(0x3F)));
    // +C5GREG: 36-bit NR cell id, NSSAI
    let r =
        Registration::parse("+C5GREG: 2,1,\"00AB12\",\"F12345678\",11,4,\"01.000001\"").unwrap();
    assert_eq!(r.ci, Some(0xF_1234_5678));
    assert_eq!(r.allowed_nssai.as_deref(), Some("01.000001"));
    assert_eq!(r.act, Some(AccessTech::Nr5gcn));
    // Unknown stat / AcT kept
    let r = Registration::parse("+CEREG: 42,\"1\",\"2\",99").unwrap();
    assert_eq!(
        (r.stat, r.act),
        (RegStatus::Other(42), Some(AccessTech::Other(99)))
    );
    // Not registration
    assert_eq!(Registration::parse("+CREG:"), None);
    assert_eq!(Registration::parse("+CREG: x"), None);
    assert_eq!(Registration::parse("+CREGS: 1"), None);
    assert_eq!(
        Registration::parse("+CREG: 256"),
        None,
        "stat out of u8 range"
    );
}

#[test]
fn signal() {
    let c = Csq::parse("+CSQ: 99,99").unwrap();
    assert_eq!((c.rssi_dbm(), c.ber), (None, 99));
    assert_eq!(Csq::parse("+CSQ: 0,0").unwrap().rssi_dbm(), Some(-113));
    assert_eq!(Csq::parse("+CSQ: 31,0").unwrap().rssi_dbm(), Some(-51));
    assert_eq!(Csq::parse("+CSQ: 18"), None);
    let e = Cesq::parse("+CESQ: 99,99,255,255,34,97,255,255,255").unwrap();
    assert_eq!(
        (e.rsrq_db(), e.rsrp_dbm(), e.ss_sinr),
        (Some(-3.0), Some(-44.0), Some(255))
    );
    assert_eq!(
        Cesq::parse("+CESQ: 99,99,0,49,255,255").unwrap().ecno_db(),
        Some(0.0)
    );
}

#[test]
fn pin_and_calls() {
    assert_eq!(Cpin::parse("+CPIN: READY"), Some(Cpin::Ready));
    assert_eq!(Cpin::parse("+cpin: \"SIM PUK2\""), Some(Cpin::SimPuk2));
    assert_eq!(
        Cpin::parse("+CPIN: BLOCKED"),
        Some(Cpin::Other("BLOCKED".into()))
    );
    assert_eq!(Cpin::parse("+CPIN: "), None);

    let c = CallerId::parse_clip("+CLIP: \"+10000000000\",145,,,\"Alice, \\22A\\22\",0").unwrap();
    assert_eq!(c.number, "+10000000000");
    assert_eq!(c.alpha.as_deref(), Some("Alice, \"A\""));
    assert_eq!(c.cli_validity, Some(0));
    let c = CallerId::parse_clip("+CLIP: \"\",128,,,,2").unwrap();
    assert_eq!((c.number.as_str(), c.cli_validity), ("", Some(2)));
    let c = CallerId::parse_colp("+COLP: \"+10000000000\",145,\"\",,\"Bob\"").unwrap();
    assert_eq!((c.alpha.as_deref(), c.cli_validity), (Some("Bob"), None));

    assert_eq!(CallWaiting::parse("+CCWA: 1,1"), None, "query response");
    let w = CallWaiting::parse("+CCWA: \"+10000000000\",145,1,\"Eve\",0").unwrap();
    assert_eq!((w.alpha.as_deref(), w.cli_validity), (Some("Eve"), Some(0)));

    let l = Clcc::parse("+CLCC: 2,0,2,0,1,\"+10000000000\",145,\"Bob\"").unwrap();
    assert_eq!(
        (l.id, l.dir, l.stat, l.mpty),
        (2, CallDirection::MobileOriginated, CallState::Dialing, true)
    );
    assert_eq!(l.alpha.as_deref(), Some("Bob"));
    let l = Clcc::parse("+CLCC: 1,1,0,1,0").unwrap();
    assert_eq!(l.number, None);
    assert_eq!(Clcc::parse("+CLCC: 1,7,0,0,0"), None, "bad dir");
}

#[test]
fn indicators() {
    assert_eq!(
        Ciev::parse("+CIEV: 2,5"),
        Some(Ciev {
            index: Some(2),
            name: None,
            value: 5
        })
    );
    assert_eq!(
        Ciev::parse("+CIEV: \"MESSAGE\",1"),
        Some(Ciev {
            index: None,
            name: Some("MESSAGE".into()),
            value: 1
        })
    );
    assert_eq!(Ciev::parse("+CIEV: 2"), None);
    assert_eq!(
        CindValues::parse("+CIND: 5,99,1,0,0,0,0")
            .unwrap()
            .values
            .len(),
        7
    );
    assert_eq!(CindValues::parse("+CIND: (\"x\",(0-1))"), None);
    let r = CindRanges::parse("+CIND: (\"signal\",(0-5)),(\"roam\",(0,1))").unwrap();
    assert_eq!(r.indicators[1].values, vec![0, 1]);
    assert_eq!(CindRanges::parse("+CIND: 1,2"), None);
}

#[test]
fn sms() {
    let h = MessageHeader::parse_cmgl("+CMGL: 3,\"STO UNSENT\",\"+10000000000\",,").unwrap();
    assert_eq!(
        (h.index, h.stat, h.mode),
        (Some(3), MessageStatus::StoredUnsent, SmsMode::Text)
    );
    assert_eq!(h.scts, None);
    let h = MessageHeader::parse_cmgl("+CMGL: 1,0,\"Alpha\",23").unwrap();
    assert_eq!((h.alpha.as_deref(), h.length), (Some("Alpha"), Some(23)));
    assert_eq!(
        MessageHeader::parse_cmgl("+CMGL: 1,9,,23"),
        None,
        "bad stat"
    );
    let h = MessageHeader::parse_cmgr(
        "+CMGR: \"REC UNREAD\",\"+10000000000\",,\"24/01/02,03:04:05+08\",145,4,0,0,\"+10000000009\",145,5",
    )
    .unwrap();
    assert_eq!((h.address_type, h.length), (Some(145), Some(5)));

    let m = Cmt::parse("+CMT: ,24").unwrap();
    assert_eq!((m.mode, m.length, m.alpha), (SmsMode::Pdu, Some(24), None));
    let m = Cmt::parse("+CMT: \"+10000000000\",,\"24/01/02,03:04:05+08\"").unwrap();
    assert_eq!(
        (m.mode, m.address.as_deref()),
        (SmsMode::Text, Some("+10000000000"))
    );
    let d = Cds::parse(
        "+CDS: 6,183,\"+10000000000\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0",
    )
    .unwrap();
    assert_eq!((d.mode, d.mr, d.st), (SmsMode::Text, Some(183), Some(0)));
    assert_eq!(Cds::parse("+CDS: 6,1"), None);

    let r = MessageReference::parse_cmss("+CMSS: 12,\"24/01/02,03:04:05+08\"").unwrap();
    assert_eq!((r.mr, r.ack.is_some()), (12, true));
    assert_eq!(MessageReference::parse_cmgs("+CMGS: 256"), None);

    let s = CpmsStatus::parse("+CPMS: 3,30,3,30,3,30").unwrap();
    assert_eq!(s.storages.len(), 3);
    assert_eq!(s.storages[0].mem, None);
    assert_eq!(CpmsStatus::parse("+CPMS: \"SM\",1"), None);
    let s = CpmsSupport::parse("+CPMS: (\"SM\",\"ME\"),\"SM\"").unwrap();
    assert_eq!((s.mem2, s.mem3.len()), (vec!["SM".to_string()], 0));

    let n = Cnmi::parse("+CNMI: 2,1,0,0,0").unwrap();
    assert_eq!((n.mode, n.mt), (2, Some(1)));
    let n = CnmiSupport::parse("+CNMI: (0-2),(0,1,3),(0),(0,1),(0,1)").unwrap();
    assert_eq!(n.mt, vec![0, 1, 3]);
}

#[test]
fn operators() {
    let c = Cops::parse("+COPS: 0").unwrap();
    assert_eq!((c.mode, c.oper), (0, None));
    let c = Cops::parse("+COPS: 1,2,\"00101\",13").unwrap();
    assert_eq!(c.act, Some(AccessTech::EutraNrDual));
    let l = CopsList::parse(
        "+COPS: (2,\"Op A\",\"A\",\"00101\",7),(3,\"Op, B\",\"B\",\"00102\",0),,(0-4),(0-2)",
    )
    .unwrap();
    assert_eq!(l.operators.len(), 2);
    assert_eq!(l.operators[1].stat, OperatorStatus::Forbidden);
    assert_eq!(l.operators[1].long.as_deref(), Some("Op, B"));
    assert_eq!(CopsList::parse("+COPS: 0,0,\"X\""), None);
    assert_eq!(Cops::parse("+COPS: (1,\"a\",\"b\",\"c\")"), None);
}

#[test]
fn cgev_other_and_modify() {
    let c = Cgev::parse("+CGEV: NW MODIFY 1,2,0").unwrap();
    assert_eq!(
        c.event,
        CgevEvent::NwModify {
            cid: 1,
            change_reason: 2,
            event_type: 0
        }
    );
    let c = Cgev::parse("+CGEV: ME PDN ACT 1,0,2").unwrap();
    assert_eq!(
        c.event,
        CgevEvent::MePdnAct {
            cid: 1,
            reason: Some(0),
            cid_other: Some(2)
        }
    );
    let c = Cgev::parse("+CGEV: SOMETHING NEW 1").unwrap();
    assert_eq!(
        (c.event, c.raw.as_str()),
        (CgevEvent::Other, "SOMETHING NEW 1")
    );
    assert_eq!(
        Cgev::parse("+CGEV: NW ACT 1").unwrap().event,
        CgevEvent::Other
    );
}

#[test]
fn dispatcher_and_serde() {
    assert!(matches!(parse_any("+CREG: 1"), Some(Urc::Registration(_))));
    assert!(matches!(parse_any("+CGEV: ME DETACH"), Some(Urc::Cgev(_))));
    assert!(matches!(parse_any("+CMTI: \"SM\",3"), Some(Urc::Cmti(_))));
    assert!(matches!(parse_any("+CDSI: \"SR\",3"), Some(Urc::Cdsi(_))));
    assert!(matches!(parse_any("+CMT: ,24"), Some(Urc::Cmt(_))));
    assert!(matches!(parse_any("+CDS: 24"), Some(Urc::Cds(_))));
    assert!(matches!(parse_any("+CIEV: 1,1"), Some(Urc::Ciev(_))));
    assert!(matches!(parse_any("+COLP: \"1\",129"), Some(Urc::Colp(_))));
    assert!(matches!(
        parse_any("+CCWA: \"1\",129,1"),
        Some(Urc::Ccwa(_))
    ));
    assert!(matches!(parse_any("+clip: \"1\",129"), Some(Urc::Clip(_))));
    for l in [
        "",
        "OK",
        "RING",
        "+CUSD: 0,\"x\",15",
        "+CSQ: 1,2",
        "+CMTI",
        "é+",
    ] {
        assert_eq!(parse_any(l), None, "{l}");
    }

    let v = serde_json::to_value(parse_any("+CEREG: 5,\"00C3\",\"0000A13B\",7").unwrap()).unwrap();
    assert_eq!(v["urc"], "registration");
    assert_eq!(v["data"]["stat"], "roaming");
    assert_eq!(v["data"]["act"], "eutran");
    assert_eq!(v["data"]["rejectCause"], serde_json::Value::Null);
    let v = serde_json::to_value(Cgev::parse("+CGEV: NW ACT 1,2,1").unwrap()).unwrap();
    assert_eq!(v["event"]["event"], "nwAct");
    assert_eq!(v["event"]["pCid"], 1);
    let v = serde_json::to_value(CallerId::parse_clip("+CLIP: \"1\",129").unwrap()).unwrap();
    assert_eq!(v["type"], 129);
}

fn all_parsers(l: &str) {
    let _ = parse_any(l);
    let _ = Registration::parse(l);
    let _ = Csq::parse(l);
    let _ = Cesq::parse(l);
    let _ = Cpin::parse(l);
    let _ = Clcc::parse(l);
    let _ = CindValues::parse(l);
    let _ = CindRanges::parse(l);
    let _ = MessageHeader::parse_cmgl(l);
    let _ = MessageHeader::parse_cmgr(l);
    let _ = MessageReference::parse_cmgs(l);
    let _ = CpmsStatus::parse(l);
    let _ = CpmsSupport::parse(l);
    let _ = Cnmi::parse(l);
    let _ = CnmiSupport::parse(l);
    let _ = Cops::parse(l);
    let _ = CopsList::parse(l);
    let _ = Cgev::parse_event(l);
    let _ = serial_codec::args::split(l);
    let _ = serial_codec::args::parse_values(l);
}

const PREFIXES: [&str; 20] = [
    "+CREG", "+CGREG", "+CEREG", "+C5GREG", "+CSQ", "+CESQ", "+CPIN", "+CLIP", "+CCWA", "+COLP",
    "+CLCC", "+CIEV", "+CIND", "+CMTI", "+CMGL", "+CMGR", "+CMT", "+CDS", "+CPMS", "+COPS",
];

proptest! {
    #[test]
    fn never_panics(s in "\\PC{0,80}") {
        all_parsers(&s);
    }

    #[test]
    fn never_panics_with_prefix(
        p in 0usize..PREFIXES.len(),
        body in "[0-9A-Fa-f\",()\\\\ \\-é]{0,60}",
    ) {
        all_parsers(&format!("{}: {body}", PREFIXES[p]));
        all_parsers(&format!("+CGEV: NW {body}"));
        all_parsers(&format!("+CNMI: {body}"));
        all_parsers(&format!("+CMGS: {body}"));
    }
}

// --- audit regressions --------------------------------------------------------------

/// 27.005 §3.4.3: a stored SUBMIT (`STO UNSENT` / `STO SENT`) has `<da>` and no `<scts>`.
#[test]
fn cmgr_text_stored_submit_has_no_scts() {
    let h = MessageHeader::parse_cmgr(
        "+CMGR: \"STO UNSENT\",\"+10000000001\",,145,17,0,0,167,\"+10000000002\",145,5",
    )
    .unwrap();
    assert_eq!(h.address.as_deref(), Some("+10000000001"));
    assert_eq!(h.scts, None);
    assert_eq!(h.address_type, Some(145));
    assert_eq!(h.length, Some(5));
    let h = MessageHeader::parse_cmgr("+CMGR: \"STO SENT\",\"+10000000001\"").unwrap();
    assert_eq!((h.scts, h.address_type), (None, None));
    // DELIVER keeps its layout.
    let h = MessageHeader::parse_cmgr(
        "+CMGR: \"REC READ\",\"+10000000001\",,\"24/01/02,03:04:05+08\",145,4,0,0,\"+10000000009\",145,5",
    )
    .unwrap();
    assert_eq!(h.scts.as_deref(), Some("24/01/02,03:04:05+08"));
    assert_eq!(h.address_type, Some(145));
}

/// Text-mode status reports (`<stat>,<fo>,<mr>,…`) are not decoded: no garbage header.
#[test]
fn cmgr_cmgl_text_status_report_is_not_a_message_header() {
    assert_eq!(
        MessageHeader::parse_cmgr(
            "+CMGR: \"REC READ\",6,12,\"+10000000001\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0"
        ),
        None
    );
    assert_eq!(
        MessageHeader::parse_cmgl(
            "+CMGL: 3,\"REC READ\",6,12,\"+10000000001\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0"
        ),
        None
    );
}

/// `AT+CLIP?` / `AT+COLP?` answer `<n>,<m>`: not a caller id.
#[test]
fn clip_colp_read_responses_are_not_urcs() {
    assert_eq!(parse_any("+CLIP: 1,1"), None);
    assert_eq!(parse_any("+COLP: 0,2"), None);
    assert_eq!(CallerId::parse_clip("+CLIP: 1,1"), None);
    assert!(matches!(
        parse_any("+CLIP: \"+10000000001\",145,,,,0"),
        Some(Urc::Clip(_))
    ));
}

/// Only 27.007 behaviour: a tuple closed before `<AcT>` gets no AcT from the stray field.
#[test]
fn cops_list_has_no_vendor_tuple_quirk() {
    let c = CopsList::parse("+COPS: (2,\"A\",\"A\",\"310260\"),2),(1,\"B\",\"B\",\"310410\",0)")
        .unwrap();
    assert_eq!(c.operators.len(), 2);
    assert_eq!(c.operators[0].act, None);
    assert_eq!(c.operators[1].act, Some(AccessTech::Gsm));
}

/// The ModemManager `+CDS` vector carried a realistic mobile number in its PDU.
#[test]
fn fixtures_carry_no_realistic_numbers() {
    for f in [
        "at_urc.json",
        "urc/at_urc.json",
        "at_replies.json",
        "urc/at_responses.json",
    ] {
        let path = format!("{}/tests/fixtures/{f}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(path).unwrap();
        assert!(!text.contains("098136395339F6"), "{f}");
        // A bank's real service number in a stored SMS text.
        assert!(!text.contains("0860003422"), "{f}");
        assert!(!text.contains("Standard Bank"), "{f}");
    }
}
