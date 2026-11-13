//! URC / information-line parsers: lines built from typed values (any field count the
//! command allows, omitted optional fields, any prefix case) must parse back to those
//! values, through the typed parser and through `parse_any`; arbitrary lines never
//! panic; stories cover fixed layouts and rejections.

use crate::common::*;
use proptest::option::of;
use serial_codec::urc::*;

/// `<prefix>: <fields>` with the prefix in any case and optional spacing / CRLF.
fn line(prefix: &str, style: u8, fields: &[String]) -> String {
    let p = match style % 3 {
        0 => prefix.to_string(),
        1 => prefix.to_ascii_lowercase(),
        _ => prefix
            .chars()
            .enumerate()
            .map(|(i, c)| {
                if i % 2 == 0 {
                    c.to_ascii_lowercase()
                } else {
                    c
                }
            })
            .collect(),
    };
    let body = fields.join(",");
    match style % 4 {
        0 => format!("{p}: {body}"),
        1 => format!("{p}:{body}"),
        2 => format!("\r\n{p}: {body}\r\n"),
        _ => format!("{p} : {body} "),
    }
}

fn q(s: &str) -> String {
    format!("\"{s}\"")
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or(String::new(), ToString::to_string)
}

fn qopt(v: &Option<String>) -> String {
    v.as_ref().map_or(String::new(), |s| q(s))
}

/// Quoted text without quotes or backslashes (both are escapes inside AT strings).
fn qtext() -> impl Strategy<Value = String> {
    "[A-Za-z0-9 ,.+()-]{1,12}"
}

fn number() -> impl Strategy<Value = String> {
    "\\+?[0-9]{1,15}"
}

/// Keep the first `n` of the optional fields; later ones are absent (`None`).
fn keep<T>(n: usize, i: usize, v: Option<T>) -> Option<T> {
    if i < n {
        v
    } else {
        None
    }
}

// ------------------------------------------------------------------ registration

fn registration() -> impl Strategy<Value = (String, Registration)> {
    (
        select(vec![
            ("+CREG", RegKind::Creg),
            ("+CGREG", RegKind::Cgreg),
            ("+CEREG", RegKind::Cereg),
            ("+C5GREG", RegKind::C5greg),
        ]),
        of(0u8..6),
        any::<u8>(),
        (
            of(any::<u16>()),
            of(0u64..1 << 36),
            of(any::<u8>()),
            of(any::<u8>()),
        ),
        (of(qtext()), of(any::<u8>()), of(any::<u32>())),
        0usize..8,
        any::<u8>(),
    )
        .prop_filter(
            "response form never has 3 fields",
            |(_, n, _, _, _, k, _)| n.is_none() || *k != 1,
        )
        .prop_map(
            |((prefix, kind), n, stat, (lac, ci, act, rac), (nssai, ct, rc), k, style)| {
                // The response form needs a 1-2 digit <stat> to be told apart.
                let stat = if n.is_some() { stat % 100 } else { stat };
                let extra = match kind {
                    RegKind::Cgreg => 1,
                    RegKind::C5greg => 2,
                    _ => 0,
                };
                let k = k.min(5 + extra);
                let (lac, ci, act) = (keep(k, 0, lac), keep(k, 1, ci), keep(k, 2, act));
                let mut f = vec![stat.to_string()];
                if let Some(n) = n {
                    f.insert(0, n.to_string());
                }
                let mut tail = vec![
                    lac.map_or(String::new(), |v| q(&format!("{v:04X}"))),
                    ci.map_or(String::new(), |v| q(&format!("{v:08X}"))),
                    opt(&act),
                ];
                let mut want = Registration {
                    kind,
                    n,
                    stat: RegStatus::from_code(stat),
                    lac: lac.map(u32::from),
                    ci,
                    act: act.map(AccessTech::from_code),
                    rac: None,
                    allowed_nssai: None,
                    cause_type: None,
                    reject_cause: None,
                };
                match kind {
                    RegKind::Cgreg => {
                        let rac = keep(k, 3, rac);
                        tail.push(rac.map_or(String::new(), |v| q(&format!("{v:02X}"))));
                        want.rac = rac;
                    }
                    RegKind::C5greg => {
                        let nssai = keep(k, 4, nssai);
                        tail.push("2".into());
                        tail.push(qopt(&nssai));
                        want.allowed_nssai = nssai;
                    }
                    _ => {}
                }
                let (ct, rc) = (keep(k, 3 + extra, ct), keep(k, 4 + extra, rc));
                tail.push(opt(&ct));
                tail.push(opt(&rc));
                want.cause_type = ct;
                want.reject_cause = rc;
                tail.truncate(k);
                f.extend(tail);
                (line(prefix, style, &f), want)
            },
        )
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn registration_lines((l, want) in registration()) {
        prop_assert_eq!(Registration::parse(&l), Some(want.clone()));
        let registered = matches!(want.stat, RegStatus::Home | RegStatus::Roaming
            | RegStatus::HomeSmsOnly | RegStatus::RoamingSmsOnly
            | RegStatus::HomeCsfbNotPreferred | RegStatus::RoamingCsfbNotPreferred);
        prop_assert_eq!(want.stat.is_registered(), registered);
        prop_assert_eq!(parse_any(&l), Some(Urc::Registration(want)));
    }
}

#[test]
fn reg_status_and_access_tech_codes() {
    for v in 0..=255u8 {
        let s = RegStatus::from_code(v);
        assert_eq!(s.is_registered(), [1, 5, 6, 7, 9, 10].contains(&v), "{v}");
        assert_eq!(s == RegStatus::Other(v), v > 11, "{v}");
        assert_eq!(
            AccessTech::from_code(v) == AccessTech::Other(v),
            v > 13,
            "{v}"
        );
    }
}

// ------------------------------------------------------------------ signal

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn csq_lines(rssi in any::<u8>(), ber in any::<u8>(), style in any::<u8>()) {
        let c = Csq::parse(&line("+CSQ", style, &[rssi.to_string(), ber.to_string()])).unwrap();
        prop_assert_eq!(c, Csq { rssi, ber });
        prop_assert_eq!(c.rssi_dbm(), (rssi <= 31).then(|| -113 + 2 * i32::from(rssi)));
    }

    #[test]
    fn cesq_lines(v in any::<[u8; 9]>(), k in 6usize..=9, style in any::<u8>()) {
        let f: Vec<String> = v[..k].iter().map(u8::to_string).collect();
        let c = Cesq::parse(&line("+CESQ", style, &f)).unwrap();
        let o = |i: usize| (i < k).then_some(v[i]);
        prop_assert_eq!(c, Cesq {
            rxlev: v[0], ber: v[1], rscp: v[2], ecno: v[3], rsrq: v[4], rsrp: v[5],
            ss_rsrq: o(6), ss_rsrp: o(7), ss_sinr: o(8),
        });
        let f = |x: u8| f64::from(x);
        prop_assert_eq!(c.rxlev_dbm(), (v[0] <= 63).then(|| -111.0 + f(v[0])));
        prop_assert_eq!(c.rscp_dbm(), (v[2] <= 96).then(|| -121.0 + f(v[2])));
        prop_assert_eq!(c.ecno_db(), (v[3] <= 49).then(|| -24.5 + f(v[3]) / 2.0));
        prop_assert_eq!(c.rsrq_db(), (v[4] <= 34).then(|| -20.0 + f(v[4]) / 2.0));
        prop_assert_eq!(c.rsrp_dbm(), (v[5] <= 97).then(|| -141.0 + f(v[5])));
    }
}

// ------------------------------------------------------------------ PIN, calls

fn cpin_codes() -> Vec<(&'static str, Cpin)> {
    use Cpin::*;
    vec![
        ("READY", Ready),
        ("SIM PIN", SimPin),
        ("SIM PUK", SimPuk),
        ("PH-SIM PIN", PhSimPin),
        ("PH-FSIM PIN", PhFsimPin),
        ("PH-FSIM PUK", PhFsimPuk),
        ("SIM PIN2", SimPin2),
        ("SIM PUK2", SimPuk2),
        ("PH-NET PIN", PhNetPin),
        ("PH-NET PUK", PhNetPuk),
        ("PH-NETSUB PIN", PhNetsubPin),
        ("PH-NETSUB PUK", PhNetsubPuk),
        ("PH-SP PIN", PhSpPin),
        ("PH-SP PUK", PhSpPuk),
        ("PH-CORP PIN", PhCorpPin),
        ("PH-CORP PUK", PhCorpPuk),
    ]
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn cpin_lines(i in any::<prop::sample::Index>(), lower in any::<bool>(), quoted in any::<bool>(),
                  other in "[A-Z][A-Z0-9 ]{0,10}[A-Z0-9]", style in any::<u8>()) {
        let codes = cpin_codes();
        let (code, want) = codes[i.index(codes.len())].clone();
        let code = if lower { code.to_lowercase() } else { code.to_string() };
        let field = if quoted { q(&code) } else { code };
        prop_assert_eq!(Cpin::parse(&line("+CPIN", style, &[field])), Some(want));
        if !codes.iter().any(|(c, _)| c.eq_ignore_ascii_case(&other)) {
            prop_assert_eq!(Cpin::parse(&line("+CPIN", style, std::slice::from_ref(&other))), Some(Cpin::Other(other)));
        }
    }

    /// `+CLIP` / `+COLP`: quoted number, type, then optional subaddress, its type,
    /// alpha and (CLIP only) CLI validity.
    #[test]
    fn caller_id_lines(
        number in number(),
        ty in any::<u8>(),
        (subaddr, satype, alpha, validity) in (of(qtext()), of(any::<u8>()), of(qtext()), of(0u8..3)),
        k in 0usize..=4,
        style in any::<u8>(),
    ) {
        let (subaddr, satype, alpha, validity) =
            (keep(k, 0, subaddr), keep(k, 1, satype), keep(k, 2, alpha), keep(k, 3, validity));
        let mut f = vec![q(&number), ty.to_string(), qopt(&subaddr), opt(&satype), qopt(&alpha), opt(&validity)];
        f.truncate(2 + k);
        let want = CallerId { number, number_type: Some(ty), subaddr, satype, alpha, cli_validity: validity };
        let clip = line("+CLIP", style, &f);
        prop_assert_eq!(CallerId::parse_clip(&clip), Some(want.clone()));
        prop_assert_eq!(parse_any(&clip), Some(Urc::Clip(want.clone())));
        let colp = line("+COLP", style, &f);
        let want = CallerId { cli_validity: None, ..want };
        prop_assert_eq!(CallerId::parse_colp(&colp), Some(want.clone()));
        prop_assert_eq!(parse_any(&colp), Some(Urc::Colp(want)));
    }

    #[test]
    fn call_waiting_lines(
        number in number(),
        (ty, class) in (any::<u8>(), any::<u8>()),
        (alpha, validity, subaddr, satype, priority) in (of(qtext()), of(0u8..3), of(qtext()), of(any::<u8>()), of(any::<u8>())),
        k in 0usize..=5,
        style in any::<u8>(),
    ) {
        let (alpha, validity, subaddr, satype, priority) = (keep(k, 0, alpha), keep(k, 1, validity),
            keep(k, 2, subaddr), keep(k, 3, satype), keep(k, 4, priority));
        let mut f = vec![q(&number), ty.to_string(), class.to_string(), qopt(&alpha), opt(&validity),
            qopt(&subaddr), opt(&satype), opt(&priority)];
        f.truncate(3 + k);
        let want = CallWaiting { number, number_type: Some(ty), class: Some(class), alpha,
            cli_validity: validity, subaddr, satype, priority };
        let l = line("+CCWA", style, &f);
        prop_assert_eq!(CallWaiting::parse(&l), Some(want.clone()));
        prop_assert_eq!(parse_any(&l), Some(Urc::Ccwa(want)));
    }

    #[test]
    fn clcc_lines(
        (id, dir, stat, mode, mpty) in (any::<u32>(), 0u8..2, any::<u8>(), any::<u8>(), 0u8..2),
        caller in of((number(), any::<u8>(), of(qtext()), of(any::<u8>()), of(0u8..3))),
        k in 0usize..=3,
        style in any::<u8>(),
    ) {
        let mut f = vec![id.to_string(), dir.to_string(), stat.to_string(), mode.to_string(), mpty.to_string()];
        let mut want = Clcc {
            id,
            dir: if dir == 0 { CallDirection::MobileOriginated } else { CallDirection::MobileTerminated },
            stat: match stat {
                0 => CallState::Active, 1 => CallState::Held, 2 => CallState::Dialing,
                3 => CallState::Alerting, 4 => CallState::Incoming, 5 => CallState::Waiting,
                v => CallState::Other(v),
            },
            mode, mpty: mpty == 1, number: None, number_type: None, alpha: None, priority: None, cli_validity: None,
        };
        if let Some((n, ty, alpha, priority, validity)) = caller {
            let (alpha, priority, validity) = (keep(k, 0, alpha), keep(k, 1, priority), keep(k, 2, validity));
            let mut extra = vec![q(&n), ty.to_string(), qopt(&alpha), opt(&priority), opt(&validity)];
            extra.truncate(2 + k);
            f.extend(extra);
            want.number = Some(n);
            want.number_type = Some(ty);
            (want.alpha, want.priority, want.cli_validity) = (alpha, priority, validity);
        }
        prop_assert_eq!(Clcc::parse(&line("+CLCC", style, &f)), Some(want));
    }

    #[test]
    fn ciev_lines(index in prop_oneof![any::<u32>().prop_map(Ok), "[a-z]{1,8}".prop_map(Err)],
                  value in any::<i64>(), quoted in any::<bool>(), style in any::<u8>()) {
        let (ind, want) = match index {
            Ok(i) => (i.to_string(), Ciev { index: Some(i), name: None, value }),
            Err(n) => (if quoted { q(&n) } else { n.clone() }, Ciev { index: None, name: Some(n), value }),
        };
        let l = line("+CIEV", style, &[ind, value.to_string()]);
        prop_assert_eq!(Ciev::parse(&l), Some(want.clone()));
        prop_assert_eq!(parse_any(&l), Some(Urc::Ciev(want)));
    }
}

// ------------------------------------------------------------------ SMS

fn mem() -> impl Strategy<Value = String> {
    select(vec!["SM", "ME", "MT", "SR", "BM"]).prop_map(String::from)
}

fn scts() -> impl Strategy<Value = String> {
    (
        0u8..100,
        1u8..13,
        1u8..29,
        0u8..24,
        0u8..60,
        0u8..60,
        -48i8..=48,
    )
        .prop_map(|(y, mo, d, h, mi, s, z)| {
            format!("{y:02}/{mo:02}/{d:02},{h:02}:{mi:02}:{s:02}{z:+03}")
        })
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn message_indication_lines(mem in mem(), index in any::<u32>(), quoted in any::<bool>(), style in any::<u8>()) {
        let m = if quoted { q(&mem) } else { mem.clone() };
        let want = MessageIndication { mem, index };
        let f = [m, index.to_string()];
        let cmti = line("+CMTI", style, &f);
        prop_assert_eq!(MessageIndication::parse_cmti(&cmti), Some(want.clone()));
        prop_assert_eq!(parse_any(&cmti), Some(Urc::Cmti(want.clone())));
        let cdsi = line("+CDSI", style, &f);
        prop_assert_eq!(MessageIndication::parse_cdsi(&cdsi), Some(want.clone()));
        prop_assert_eq!(parse_any(&cdsi), Some(Urc::Cdsi(want)));
    }

    /// `+CDS`: PDU form `<length>`, text form `<fo>,<mr>,[<ra>],[<tora>],<scts>,<dt>,<st>`.
    #[test]
    fn cds_lines(
        len in any::<u32>(),
        (fo, mr, st) in (any::<u8>(), any::<u8>(), any::<u8>()),
        (ra, tora) in (of(number()), of(any::<u8>())),
        (sc, dt) in (scts(), scts()),
        style in any::<u8>(),
    ) {
        let pdu = Cds { mode: SmsMode::Pdu, length: Some(len), fo: None, mr: None, ra: None, tora: None,
            scts: None, dt: None, st: None };
        let l = line("+CDS", style, &[len.to_string()]);
        prop_assert_eq!(Cds::parse(&l), Some(pdu.clone()));
        prop_assert_eq!(parse_any(&l), Some(Urc::Cds(pdu)));
        let f = [fo.to_string(), mr.to_string(), qopt(&ra), opt(&tora), q(&sc), q(&dt), st.to_string()];
        let text = Cds { mode: SmsMode::Text, length: None, fo: Some(fo), mr: Some(mr), ra, tora,
            scts: Some(sc), dt: Some(dt), st: Some(st) };
        prop_assert_eq!(Cds::parse(&line("+CDS", style, &f)), Some(text));
    }

    /// `+CMT`: PDU form `[<alpha>],<length>`; text form `<oa>,[<alpha>],<scts>[,…,<length>]`.
    #[test]
    fn cmt_lines(
        alpha in of(qtext()),
        len in any::<u32>(),
        oa in number(),
        sc in scts(),
        detail in of((any::<u8>(), any::<[u8; 3]>(), number(), any::<u8>(), any::<u32>())),
        style in any::<u8>(),
    ) {
        let pdu = Cmt { mode: SmsMode::Pdu, alpha: alpha.clone(), length: Some(len), address: None,
            address_type: None, scts: None };
        let l = line("+CMT", style, &[qopt(&alpha), len.to_string()]);
        prop_assert_eq!(Cmt::parse(&l), Some(pdu.clone()));
        prop_assert_eq!(parse_any(&l), Some(Urc::Cmt(pdu)));
        let mut f = vec![q(&oa), qopt(&alpha), q(&sc)];
        let mut text = Cmt { mode: SmsMode::Text, alpha, length: None, address: Some(oa),
            address_type: None, scts: Some(sc) };
        if let Some((tooa, [fo, pid, dcs], sca, tosca, n)) = detail {
            f.extend([tooa.to_string(), fo.to_string(), pid.to_string(), dcs.to_string(), q(&sca),
                tosca.to_string(), n.to_string()]);
            text.address_type = Some(tooa);
            text.length = Some(n);
        }
        prop_assert_eq!(Cmt::parse(&line("+CMT", style, &f)), Some(text));
    }

    /// `+CMGL` / `+CMGR` headers in PDU mode, `<stat>` as a number or (text) a name.
    #[test]
    fn message_header_lines(stat in 0usize..5, index in any::<u32>(), alpha in of(qtext()), len in any::<u32>(),
                            name_case in any::<bool>(), style in any::<u8>()) {
        let all = [MessageStatus::ReceivedUnread, MessageStatus::ReceivedRead, MessageStatus::StoredUnsent,
            MessageStatus::StoredSent, MessageStatus::All];
        let names = ["REC UNREAD", "REC READ", "STO UNSENT", "STO SENT", "ALL"];
        let pdu = MessageHeader { mode: SmsMode::Pdu, index: Some(index), stat: all[stat], alpha: alpha.clone(),
            length: Some(len), address: None, address_type: None, scts: None };
        let f = [index.to_string(), stat.to_string(), qopt(&alpha), len.to_string()];
        prop_assert_eq!(MessageHeader::parse_cmgl(&line("+CMGL", style, &f)), Some(pdu.clone()));
        prop_assert_eq!(MessageHeader::parse_cmgr(&line("+CMGR", style, &f[1..])), Some(MessageHeader { index: None, ..pdu }));
        let name = if name_case { names[stat].to_lowercase() } else { names[stat].to_string() };
        let text = MessageHeader { mode: SmsMode::Text, index: Some(index), stat: all[stat], alpha: alpha.clone(),
            length: None, address: Some("+1".into()), address_type: None, scts: None };
        let f = [index.to_string(), q(&name), q("+1"), qopt(&alpha)];
        prop_assert_eq!(MessageHeader::parse_cmgl(&line("+CMGL", style, &f)), Some(text));
    }

    #[test]
    fn message_reference_lines(mr in any::<u8>(), ack in of(scts()), style in any::<u8>()) {
        let mut f = vec![mr.to_string()];
        f.extend(ack.iter().map(|a| q(a)));
        let want = MessageReference { mr, ack };
        prop_assert_eq!(MessageReference::parse_cmgs(&line("+CMGS", style, &f)), Some(want.clone()));
        prop_assert_eq!(MessageReference::parse_cmss(&line("+CMSS", style, &f)), Some(want));
    }

    /// `+CPMS?` (`<mem>,<used>,<total>` per storage) and the set response (counts only).
    #[test]
    fn cpms_lines(st in vec((mem(), any::<u32>(), any::<u32>()), 1..=3), with_mem in any::<bool>(), style in any::<u8>()) {
        let f: Vec<String> = st.iter().flat_map(|(m, u, t)| {
            let mut v = vec![u.to_string(), t.to_string()];
            if with_mem { v.insert(0, q(m)); }
            v
        }).collect();
        let storages = st.into_iter().map(|(m, used, total)| StorageUsage {
            mem: with_mem.then_some(m), used, total,
        }).collect();
        prop_assert_eq!(CpmsStatus::parse(&line("+CPMS", style, &f)), Some(CpmsStatus { storages }));
    }

    #[test]
    fn cnmi_lines(v in any::<[u8; 5]>(), k in 1usize..=5, style in any::<u8>()) {
        let f: Vec<String> = v[..k].iter().map(u8::to_string).collect();
        let o = |i: usize| (i < k).then_some(v[i]);
        prop_assert_eq!(Cnmi::parse(&line("+CNMI", style, &f)),
            Some(Cnmi { mode: v[0], mt: o(1), bm: o(2), ds: o(3), bfr: o(4) }));
    }
}

// ------------------------------------------------------------------ operators

fn operator() -> impl Strategy<Value = (String, OperatorEntry)> {
    (any::<u8>(), qtext(), qtext(), "[0-9]{5,6}", of(any::<u8>())).prop_map(
        |(stat, long, short, numeric, act)| {
            let mut f = vec![stat.to_string(), q(&long), q(&short), q(&numeric)];
            f.extend(act.map(|a| a.to_string()));
            let entry = OperatorEntry {
                stat: match stat {
                    0 => OperatorStatus::Unknown,
                    1 => OperatorStatus::Available,
                    2 => OperatorStatus::Current,
                    3 => OperatorStatus::Forbidden,
                    v => OperatorStatus::Other(v),
                },
                long: Some(long),
                short: Some(short),
                numeric: Some(numeric),
                act: act.map(AccessTech::from_code),
            };
            (format!("({})", f.join(",")), entry)
        },
    )
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn cops_lines(mode in any::<u8>(), format in any::<u8>(), oper in qtext(), act in of(any::<u8>()),
                  k in 0usize..=3, style in any::<u8>()) {
        let mut f = vec![mode.to_string(), format.to_string(), q(&oper), opt(&act)];
        f.truncate(1 + k);
        let want = Cops {
            mode,
            format: keep(k, 0, Some(format)),
            oper: keep(k, 1, Some(oper)),
            act: keep(k, 2, act).map(AccessTech::from_code),
        };
        prop_assert_eq!(Cops::parse(&line("+COPS", style, &f)), Some(want));
    }

    #[test]
    fn cops_list_lines(ops in vec(operator(), 1..5), lists in any::<bool>(), style in any::<u8>()) {
        let mut f: Vec<String> = ops.iter().map(|(s, _)| s.clone()).collect();
        if lists {
            f.extend(["".into(), "(0-4)".into(), "(0-2)".into()]);
        }
        let l = line("+COPS", style, &f);
        let want = CopsList { operators: ops.into_iter().map(|(_, e)| e).collect() };
        prop_assert_eq!(CopsList::parse(&l), Some(want));
        prop_assert_eq!(Cops::parse(&l), None);
    }
}

// ------------------------------------------------------------------ +CGEV

fn cgev() -> impl Strategy<Value = (String, CgevEvent)> {
    use CgevEvent::*;
    let ty = || select(vec!["IP", "IPV6", "PPP"]).prop_map(String::from);
    let addr = || "[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}\\.[0-9]{1,3}";
    let n = || 0u32..20;
    prop_oneof![
        (ty(), addr()).prop_map(|(t, a)| (
            format!("REJECT {t},\"{a}\""),
            Reject {
                pdp_type: t,
                pdp_addr: a
            }
        )),
        (
            select(vec!["NW REACT", "NW DEACT", "ME DEACT"]),
            ty(),
            addr(),
            of(n())
        )
            .prop_map(|(w, t, a, c)| {
                let text = format!(
                    "{w} {t},\"{a}\"{}",
                    c.map_or(String::new(), |c| format!(",{c}"))
                );
                let (pdp_type, pdp_addr, cid) = (t, a, c);
                let e = match w {
                    "NW REACT" => NwReact {
                        pdp_type,
                        pdp_addr,
                        cid,
                    },
                    "NW DEACT" => NwDeactPdp {
                        pdp_type,
                        pdp_addr,
                        cid,
                    },
                    _ => MeDeactPdp {
                        pdp_type,
                        pdp_addr,
                        cid,
                    },
                };
                (text, e)
            }),
        (
            select(vec!["NW PDN ACT", "NW PDN DEACT", "ME PDN DEACT"]),
            n()
        )
            .prop_map(|(w, cid)| {
                let e = match w {
                    "NW PDN ACT" => NwPdnAct { cid },
                    "NW PDN DEACT" => NwPdnDeact { cid },
                    _ => MePdnDeact { cid },
                };
                (format!("{w} {cid}"), e)
            }),
        (n(), of((n(), of(n())))).prop_map(|(cid, more)| {
            let (reason, cid_other) = more.map_or((None, None), |(r, o)| (Some(r), o));
            let mut text = format!("ME PDN ACT {cid}");
            for v in [reason, cid_other].into_iter().flatten() {
                text += &format!(",{v}");
            }
            (
                text,
                MePdnAct {
                    cid,
                    reason,
                    cid_other,
                },
            )
        }),
        (
            select(vec![
                "NW ACT",
                "ME ACT",
                "NW DEACT",
                "ME DEACT",
                "NW MODIFY",
                "ME MODIFY"
            ]),
            n(),
            n(),
            n()
        )
            .prop_map(|(w, a, b, c)| {
                let e = match w {
                    "NW ACT" => NwAct {
                        p_cid: a,
                        cid: b,
                        event_type: c,
                    },
                    "ME ACT" => MeAct {
                        p_cid: a,
                        cid: b,
                        event_type: c,
                    },
                    "NW DEACT" => NwDeact {
                        p_cid: a,
                        cid: b,
                        event_type: c,
                    },
                    "ME DEACT" => MeDeact {
                        p_cid: a,
                        cid: b,
                        event_type: c,
                    },
                    "NW MODIFY" => NwModify {
                        cid: a,
                        change_reason: b,
                        event_type: c,
                    },
                    _ => MeModify {
                        cid: a,
                        change_reason: b,
                        event_type: c,
                    },
                };
                (format!("{w} {a},{b},{c}"), e)
            }),
        select(vec![("NW DETACH", NwDetach), ("ME DETACH", MeDetach)])
            .prop_map(|(w, e)| (w.to_string(), e)),
        (any::<bool>(), select(vec!["A", "B", "CG", "CC"])).prop_map(|(nw, c)| {
            let class = c.to_string();
            if nw {
                (format!("NW CLASS {c}"), NwClass { class })
            } else {
                (format!("ME CLASS {c}"), MeClass { class })
            }
        }),
        "[A-Z]{3,8} [0-9]"
            .prop_filter("not a keyword", |s| !s.starts_with("NW")
                && !s.starts_with("ME")
                && !s.starts_with("REJECT"))
            .prop_map(|s| (s, Other)),
    ]
}

proptest! {
    #![proptest_config(cases(512))]

    #[test]
    fn cgev_lines((text, event) in cgev(), lower in any::<bool>(), style in any::<u8>()) {
        // Keywords match in any case; arguments are kept as sent.
        const KEYS: [&str; 10] = ["NW", "ME", "PDN", "ACT", "DEACT", "REACT", "MODIFY", "DETACH", "CLASS", "REJECT"];
        let text = if lower {
            let mut keys = true;
            text.split(' ')
                .map(|w| {
                    keys &= KEYS.contains(&w);
                    if keys { w.to_lowercase() } else { w.to_string() }
                })
                .collect::<Vec<_>>()
                .join(" ")
        } else {
            text
        };
        let want = Cgev { event, raw: text.trim().to_string() };
        let l = line("+CGEV", style, &[text]);
        prop_assert_eq!(Cgev::parse(&l), Some(want.clone()));
        prop_assert_eq!(parse_any(&l), Some(Urc::Cgev(want)));
    }
}

// ------------------------------------------------------------------ robustness

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
    #![proptest_config(cases(256))]

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

// ------------------------------------------------------------------ stories

/// Given a text-mode `+CMGR` of a stored SUBMIT (`STO UNSENT` / `STO SENT`) and of a DELIVER
/// When the headers are parsed
/// Then the SUBMIT has `<da>` and no `<scts>` (27.005 §3.4.3), the DELIVER keeps its layout
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

/// Given text-mode `+CMGR` / `+CMGL` lines of a status report (`<stat>,<fo>,<mr>,…`)
/// When they are parsed as message headers
/// Then no header comes out instead of a garbage one
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

/// Given the `AT+CLIP?` / `AT+COLP?` read responses `<n>,<m>`
/// When they are parsed
/// Then they are no caller id; a quoted number is
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

/// Given a `+COPS=?` list where a tuple closes before `<AcT>` and a stray field follows
/// When it is parsed
/// Then the stray field gives no AcT (27.007 only)
#[test]
fn cops_list_has_no_vendor_tuple_quirk() {
    let c = CopsList::parse("+COPS: (2,\"A\",\"A\",\"310260\"),2),(1,\"B\",\"B\",\"310410\",0)")
        .unwrap();
    assert_eq!(c.operators.len(), 2);
    assert_eq!(c.operators[0].act, None);
    assert_eq!(c.operators[1].act, Some(AccessTech::Gsm));
}

/// Given the fixture files
/// When they are scanned
/// Then no realistic phone number or bank text is in them
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

/// Given lines that are not the parser's command or miss required fields
/// When they are parsed
/// Then the parser returns `None`
#[test]
fn malformed_lines_are_rejected() {
    assert_eq!(Registration::parse("+CREG:"), None);
    assert_eq!(Registration::parse("+CREG: x"), None);
    assert_eq!(Registration::parse("+CREGS: 1"), None);
    assert_eq!(Registration::parse("+CREG: 256"), None);
    assert_eq!(Csq::parse("+CSQ: 18"), None);
    assert_eq!(Cpin::parse("+CPIN: "), None);
    assert_eq!(CallWaiting::parse("+CCWA: 1,1"), None);
    assert_eq!(Clcc::parse("+CLCC: 1,7,0,0,0"), None);
    assert_eq!(Ciev::parse("+CIEV: 2"), None);
    assert_eq!(MessageHeader::parse_cmgl("+CMGL: 1,9,,23"), None);
    assert_eq!(Cds::parse("+CDS: 6,1"), None);
    assert_eq!(Cds::parse("+CDS: 6,183,,,\"24/01/02,03:04:05+08\""), None);
    assert_eq!(Cmt::parse("+CMT: \"+10000000000\",\"\""), None);
    assert_eq!(MessageReference::parse_cmgs("+CMGS: 256"), None);
    assert_eq!(CpmsStatus::parse("+CPMS: \"SM\",1"), None);
    assert_eq!(CnmiSupport::parse("+CNMI: 2,1,0,0,0"), None);
    assert_eq!(CopsList::parse("+COPS: 0,0,\"X\""), None);
    assert_eq!(Cops::parse("+COPS: (1,\"a\",\"b\",\"c\")"), None);
    assert_eq!(
        Cgev::parse("+CGEV: NW ACT 1").unwrap().event,
        CgevEvent::Other
    );
    // An unquoted number is a call-waiting URC only with the 3-field form.
    let c = CallWaiting::parse("+CCWA: 10000000000,129,1").unwrap();
    assert_eq!((c.number.as_str(), c.class), ("10000000000", Some(1)));
}

/// Given `+CIND` read and test responses
/// When they are parsed
/// Then values and ranges come out, and each parser refuses the other form
#[test]
fn cind_lines() {
    assert_eq!(
        CindValues::parse("+CIND: 5,99,1,0,0,0,0")
            .unwrap()
            .values
            .len(),
        7
    );
    assert_eq!(CindValues::parse("+CIND: (\"x\",(0-1))"), None);
    let r = CindRanges::parse("+CIND: (\"signal\",(0-5)),(\"roam\",(0,1))").unwrap();
    assert_eq!(
        (r.indicators[0].values.len(), r.indicators[1].values.clone()),
        (6, vec![0, 1])
    );
    assert_eq!(r.indicators[1].index, 2);
    assert_eq!(CindRanges::parse("+CIND: 1,2"), None);
}

/// Given text-mode `+CMGL` without `<scts>` and `+CMGR` of a DELIVER with all fields
/// When they are parsed
/// Then address type and length are read from their 27.005 positions
#[test]
fn cmgr_text_layouts() {
    let h = MessageHeader::parse_cmgl("+CMGL: 3,\"STO UNSENT\",\"+10000000000\",,").unwrap();
    assert_eq!(
        (h.index, h.stat, h.mode, h.scts),
        (Some(3), MessageStatus::StoredUnsent, SmsMode::Text, None)
    );
    let h = MessageHeader::parse_cmgr(
        "+CMGR: \"REC UNREAD\",\"+10000000000\",,\"24/01/02,03:04:05+08\",145,4,0,0,\"+10000000009\",145,5",
    )
    .unwrap();
    assert_eq!((h.address_type, h.length), (Some(145), Some(5)));
}

/// Given `+CPMS=?` / `+CNMI=?` test responses
/// When they are parsed
/// Then storage lists (a single storage without parentheses too) and ranges expand
#[test]
fn cpms_cnmi_support() {
    let s = CpmsSupport::parse("+CPMS: (\"SM\",\"ME\"),\"SM\"").unwrap();
    assert_eq!(
        (s.mem1.len(), s.mem2, s.mem3.len()),
        (2, vec!["SM".to_string()], 0)
    );
    let n = CnmiSupport::parse("+CNMI: (0-2),(0,1,3),(0),(0,1),(0,1)").unwrap();
    assert_eq!((n.mode, n.mt), (vec![0, 1, 2], vec![0, 1, 3]));
}

/// Given a `+COPS=?` entry with an empty short name
/// When it is parsed
/// Then that name is `None`
#[test]
fn cops_list_empty_name() {
    let l = CopsList::parse("+COPS: (1,\"Other\",\"\",\"25002\"),,(0,1),(0,2)").unwrap();
    assert_eq!(l.operators[0].short, None);
}

/// Given lines that are no supported URC, and parsed URCs
/// When they are dispatched and serialized
/// Then dispatch gives `None`, and the JSON shape is the frontend contract
#[test]
fn dispatcher_negatives_and_serde() {
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
