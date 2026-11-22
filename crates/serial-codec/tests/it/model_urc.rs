//! Model: information lines and URCs (27.007, 27.005).
//!
//! proptest builds lines from typed values: any number of the optional fields a command allows,
//! omitted ones in between, the prefix in any case and spacing. Every line is given to every
//! parser: the right one must return exactly the values, all others nothing, and `parse_any`
//! the same values for a URC. Lines with a required field missing or junk parse to nothing.
//! Every command is drawn in every run.

use crate::common::*;
use proptest::option::of;
use serial_codec::urc::*;

/// Everything a line can parse to.
#[derive(Debug, Clone, PartialEq)]
enum Parsed {
    Registration(Registration),
    Csq(Csq),
    Cesq(Cesq),
    Cpin(Cpin),
    Clip(CallerId),
    Colp(CallerId),
    Ccwa(CallWaiting),
    Clcc(Clcc),
    Ciev(Ciev),
    CindValues(CindValues),
    CindRanges(CindRanges),
    Cmti(MessageIndication),
    Cdsi(MessageIndication),
    Cmgl(MessageHeader),
    Cmgr(MessageHeader),
    Cmt(Cmt),
    Cds(Cds),
    Cmgs(MessageReference),
    Cmss(MessageReference),
    CpmsStatus(CpmsStatus),
    CpmsSupport(CpmsSupport),
    Cnmi(Cnmi),
    CnmiSupport(CnmiSupport),
    Cops(Cops),
    CopsList(CopsList),
    Cgev(Cgev),
}

/// Every parser's answer for one line.
fn parse_all(l: &str) -> Vec<Parsed> {
    use Parsed as P;
    [
        Registration::parse(l).map(P::Registration),
        Csq::parse(l).map(P::Csq),
        Cesq::parse(l).map(P::Cesq),
        Cpin::parse(l).map(P::Cpin),
        CallerId::parse_clip(l).map(P::Clip),
        CallerId::parse_colp(l).map(P::Colp),
        CallWaiting::parse(l).map(P::Ccwa),
        Clcc::parse(l).map(P::Clcc),
        Ciev::parse(l).map(P::Ciev),
        CindValues::parse(l).map(P::CindValues),
        CindRanges::parse(l).map(P::CindRanges),
        MessageIndication::parse_cmti(l).map(P::Cmti),
        MessageIndication::parse_cdsi(l).map(P::Cdsi),
        MessageHeader::parse_cmgl(l).map(P::Cmgl),
        MessageHeader::parse_cmgr(l).map(P::Cmgr),
        Cmt::parse(l).map(P::Cmt),
        Cds::parse(l).map(P::Cds),
        MessageReference::parse_cmgs(l).map(P::Cmgs),
        MessageReference::parse_cmss(l).map(P::Cmss),
        CpmsStatus::parse(l).map(P::CpmsStatus),
        CpmsSupport::parse(l).map(P::CpmsSupport),
        Cnmi::parse(l).map(P::Cnmi),
        CnmiSupport::parse(l).map(P::CnmiSupport),
        Cops::parse(l).map(P::Cops),
        CopsList::parse(l).map(P::CopsList),
        Cgev::parse(l).map(P::Cgev),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The URC `parse_any` must give for a parsed line, if it is one.
fn as_urc(p: &Parsed) -> Option<Urc> {
    Some(match p.clone() {
        Parsed::Registration(r) => Urc::Registration(r),
        Parsed::Clip(c) => Urc::Clip(c),
        Parsed::Colp(c) => Urc::Colp(c),
        Parsed::Ccwa(c) => Urc::Ccwa(c),
        Parsed::Ciev(c) => Urc::Ciev(c),
        Parsed::Cmti(m) => Urc::Cmti(m),
        Parsed::Cdsi(m) => Urc::Cdsi(m),
        Parsed::Cmt(c) => Urc::Cmt(c),
        Parsed::Cds(c) => Urc::Cds(c),
        Parsed::Cgev(c) => Urc::Cgev(c),
        _ => return None,
    })
}

type Line = (String, Parsed);

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

fn registration() -> impl Strategy<Value = Line> {
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
                (line(prefix, style, &f), Parsed::Registration(want))
            },
        )
}

// ------------------------------------------------------------------ signal, PIN, calls

fn csq() -> impl Strategy<Value = Line> {
    (any::<u8>(), any::<u8>(), any::<u8>()).prop_map(|(rssi, ber, style)| {
        let l = line("+CSQ", style, &[rssi.to_string(), ber.to_string()]);
        (l, Parsed::Csq(Csq { rssi, ber }))
    })
}

fn cesq() -> impl Strategy<Value = Line> {
    (any::<[u8; 9]>(), 6usize..=9, any::<u8>()).prop_map(|(v, k, style)| {
        let f: Vec<String> = v[..k].iter().map(u8::to_string).collect();
        let o = |i: usize| (i < k).then_some(v[i]);
        let want = Cesq {
            rxlev: v[0],
            ber: v[1],
            rscp: v[2],
            ecno: v[3],
            rsrq: v[4],
            rsrp: v[5],
            ss_rsrq: o(6),
            ss_rsrp: o(7),
            ss_sinr: o(8),
        };
        (line("+CESQ", style, &f), Parsed::Cesq(want))
    })
}

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

fn cpin() -> impl Strategy<Value = Line> {
    let known = (select(cpin_codes()), any::<bool>()).prop_map(|((code, want), lower)| {
        let code = if lower {
            code.to_lowercase()
        } else {
            code.to_string()
        };
        (code, want)
    });
    let other = "[A-Z][A-Z0-9 ]{0,10}[A-Z0-9]"
        .prop_filter("a known code", |s| {
            !cpin_codes().iter().any(|(c, _)| c == s)
        })
        .prop_map(|s| (s.clone(), Cpin::Other(s)));
    let code = prop_oneof![3 => known, 1 => other];
    (code, any::<bool>(), any::<u8>()).prop_map(|((code, want), quoted, style)| {
        let field = if quoted { q(&code) } else { code };
        (line("+CPIN", style, &[field]), Parsed::Cpin(want))
    })
}

/// `+CLIP` / `+COLP`: quoted number, type, then optional subaddress, its type, alpha and (CLIP
/// only) CLI validity.
fn caller_id() -> impl Strategy<Value = Line> {
    (
        any::<bool>(),
        number(),
        any::<u8>(),
        (of(qtext()), of(any::<u8>()), of(qtext()), of(0u8..3)),
        0usize..=4,
        any::<u8>(),
    )
        .prop_map(
            |(clip, number, ty, (subaddr, satype, alpha, validity), k, style)| {
                let (subaddr, satype, alpha, validity) = (
                    keep(k, 0, subaddr),
                    keep(k, 1, satype),
                    keep(k, 2, alpha),
                    keep(k, 3, validity),
                );
                let mut f = vec![
                    q(&number),
                    ty.to_string(),
                    qopt(&subaddr),
                    opt(&satype),
                    qopt(&alpha),
                    opt(&validity),
                ];
                f.truncate(2 + k);
                let want = CallerId {
                    number,
                    number_type: Some(ty),
                    subaddr,
                    satype,
                    alpha,
                    cli_validity: validity.filter(|_| clip),
                };
                if clip {
                    (line("+CLIP", style, &f), Parsed::Clip(want))
                } else {
                    (line("+COLP", style, &f), Parsed::Colp(want))
                }
            },
        )
}

/// The number is quoted, or not when the line has the three fields of the URC.
fn call_waiting() -> impl Strategy<Value = Line> {
    (
        number(),
        (any::<u8>(), any::<u8>()),
        (
            of(qtext()),
            of(0u8..3),
            of(qtext()),
            of(any::<u8>()),
            of(any::<u8>()),
        ),
        0usize..=5,
        any::<bool>(),
        any::<u8>(),
    )
        .prop_map(
            |(
                number,
                (ty, class),
                (alpha, validity, subaddr, satype, priority),
                k,
                quoted,
                style,
            )| {
                let want = CallWaiting {
                    number: number.clone(),
                    number_type: Some(ty),
                    class: Some(class),
                    alpha: keep(k, 0, alpha),
                    cli_validity: keep(k, 1, validity),
                    subaddr: keep(k, 2, subaddr),
                    satype: keep(k, 3, satype),
                    priority: keep(k, 4, priority),
                };
                let mut f = vec![
                    if quoted { q(&number) } else { number },
                    ty.to_string(),
                    class.to_string(),
                    qopt(&want.alpha),
                    opt(&want.cli_validity),
                    qopt(&want.subaddr),
                    opt(&want.satype),
                    opt(&want.priority),
                ];
                f.truncate(3 + k);
                (line("+CCWA", style, &f), Parsed::Ccwa(want))
            },
        )
}

fn clcc() -> impl Strategy<Value = Line> {
    (
        (any::<u32>(), 0u8..2, any::<u8>(), any::<u8>(), 0u8..2),
        of((
            number(),
            any::<u8>(),
            of(qtext()),
            of(any::<u8>()),
            of(0u8..3),
        )),
        0usize..=3,
        any::<u8>(),
    )
        .prop_map(|((id, dir, stat, mode, mpty), caller, k, style)| {
            let mut f: Vec<String> = [dir, stat, mode, mpty].iter().map(u8::to_string).collect();
            f.insert(0, id.to_string());
            let mut want = Clcc {
                id,
                dir: if dir == 0 {
                    CallDirection::MobileOriginated
                } else {
                    CallDirection::MobileTerminated
                },
                stat: match stat {
                    0 => CallState::Active,
                    1 => CallState::Held,
                    2 => CallState::Dialing,
                    3 => CallState::Alerting,
                    4 => CallState::Incoming,
                    5 => CallState::Waiting,
                    v => CallState::Other(v),
                },
                mode,
                mpty: mpty == 1,
                number: None,
                number_type: None,
                alpha: None,
                priority: None,
                cli_validity: None,
            };
            if let Some((n, ty, alpha, priority, validity)) = caller {
                want.alpha = keep(k, 0, alpha);
                want.priority = keep(k, 1, priority);
                want.cli_validity = keep(k, 2, validity);
                let mut extra = vec![
                    q(&n),
                    ty.to_string(),
                    qopt(&want.alpha),
                    opt(&want.priority),
                    opt(&want.cli_validity),
                ];
                extra.truncate(2 + k);
                f.extend(extra);
                want.number = Some(n);
                want.number_type = Some(ty);
            }
            (line("+CLCC", style, &f), Parsed::Clcc(want))
        })
}

// ------------------------------------------------------------------ indicators

fn ciev() -> impl Strategy<Value = Line> {
    let index = prop_oneof![any::<u32>().prop_map(Ok), "[a-z]{1,8}".prop_map(Err)];
    (index, any::<i64>(), any::<bool>(), any::<u8>()).prop_map(|(index, value, quoted, style)| {
        let (ind, index, name) = match index {
            Ok(i) => (i.to_string(), Some(i), None),
            Err(n) => (if quoted { q(&n) } else { n.clone() }, None, Some(n)),
        };
        let l = line("+CIEV", style, &[ind, value.to_string()]);
        (l, Parsed::Ciev(Ciev { index, name, value }))
    })
}

fn cind_values() -> impl Strategy<Value = Line> {
    (vec(any::<i64>(), 1..8), any::<u8>()).prop_map(|(values, style)| {
        let f: Vec<String> = values.iter().map(i64::to_string).collect();
        let want = Parsed::CindValues(CindValues { values });
        (line("+CIND", style, &f), want)
    })
}

/// A range list as written, and its values.
fn range_list() -> impl Strategy<Value = (String, Vec<u32>)> {
    prop_oneof![
        (0u32..10, 1u32..10).prop_map(|(a, n)| (format!("({a}-{})", a + n), (a..=a + n).collect())),
        vec(0u32..10, 1..4).prop_map(|v| {
            let text: Vec<String> = v.iter().map(u32::to_string).collect();
            (format!("({})", text.join(",")), v)
        }),
    ]
}

/// `("name",(values)),…`; an empty `()` some phones add counts as a position.
fn cind_ranges() -> impl Strategy<Value = Line> {
    let entry = of(("[a-z]{1,8}", range_list()));
    (vec(entry, 0..4), "[a-z]{1,8}", range_list(), any::<u8>()).prop_map(
        |(mut entries, name, range, style)| {
            entries.push(Some((name, range)));
            let mut indicators = Vec::new();
            let mut f = Vec::new();
            for (i, e) in entries.into_iter().enumerate() {
                let Some((name, (text, values))) = e else {
                    f.push("()".to_string());
                    continue;
                };
                f.push(format!("(\"{name}\",{text})"));
                indicators.push(IndicatorRange {
                    index: i as u32 + 1,
                    name,
                    values,
                });
            }
            let want = Parsed::CindRanges(CindRanges { indicators });
            (line("+CIND", style, &f), want)
        },
    )
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

fn indication() -> impl Strategy<Value = Line> {
    (
        any::<bool>(),
        mem(),
        any::<u32>(),
        any::<bool>(),
        any::<u8>(),
    )
        .prop_map(|(stored, mem, index, quoted, style)| {
            let f = [
                if quoted { q(&mem) } else { mem.clone() },
                index.to_string(),
            ];
            let want = MessageIndication { mem, index };
            if stored {
                (line("+CMTI", style, &f), Parsed::Cmti(want))
            } else {
                (line("+CDSI", style, &f), Parsed::Cdsi(want))
            }
        })
}

/// PDU form `<length>`, text form `<fo>,<mr>,[<ra>],[<tora>],<scts>,<dt>,<st>`.
fn cds() -> impl Strategy<Value = Line> {
    let pdu = any::<u32>().prop_map(|len| {
        let want = Cds {
            mode: SmsMode::Pdu,
            length: Some(len),
            fo: None,
            mr: None,
            ra: None,
            tora: None,
            scts: None,
            dt: None,
            st: None,
        };
        (vec![len.to_string()], want)
    });
    let text = (
        any::<[u8; 3]>(),
        of(number()),
        of(any::<u8>()),
        scts(),
        scts(),
    )
        .prop_map(|([fo, mr, st], ra, tora, sc, dt)| {
            let f = vec![
                fo.to_string(),
                mr.to_string(),
                qopt(&ra),
                opt(&tora),
                q(&sc),
                q(&dt),
                st.to_string(),
            ];
            let want = Cds {
                mode: SmsMode::Text,
                length: None,
                fo: Some(fo),
                mr: Some(mr),
                ra,
                tora,
                scts: Some(sc),
                dt: Some(dt),
                st: Some(st),
            };
            (f, want)
        });
    (prop_oneof![pdu, text], any::<u8>())
        .prop_map(|((f, want), style)| (line("+CDS", style, &f), Parsed::Cds(want)))
}

/// PDU form `[<alpha>],<length>`; text form `<oa>,[<alpha>],<scts>[,<tooa>,…,<length>]`.
fn cmt() -> impl Strategy<Value = Line> {
    let detail = (any::<[u8; 5]>(), number(), any::<u32>());
    let text = of((number(), scts(), of(detail)));
    (of(qtext()), any::<u32>(), text, any::<u8>()).prop_map(|(alpha, len, text, style)| {
        let mut f = vec![qopt(&alpha), len.to_string()];
        let mut want = Cmt {
            mode: SmsMode::Pdu,
            alpha,
            length: Some(len),
            address: None,
            address_type: None,
            scts: None,
        };
        if let Some((oa, sc, detail)) = text {
            f = vec![q(&oa), f.remove(0), q(&sc)];
            want = Cmt {
                mode: SmsMode::Text,
                length: None,
                address: Some(oa),
                scts: Some(sc),
                ..want
            };
            if let Some(([tooa, fo, pid, dcs, tosca], sca, n)) = detail {
                f.extend([tooa, fo, pid, dcs].iter().map(u8::to_string));
                f.extend([q(&sca), tosca.to_string(), n.to_string()]);
                want.address_type = Some(tooa);
                want.length = Some(n);
            }
        }
        (line("+CMT", style, &f), Parsed::Cmt(want))
    })
}

const STATS: [(&str, MessageStatus); 5] = [
    ("REC UNREAD", MessageStatus::ReceivedUnread),
    ("REC READ", MessageStatus::ReceivedRead),
    ("STO UNSENT", MessageStatus::StoredUnsent),
    ("STO SENT", MessageStatus::StoredSent),
    ("ALL", MessageStatus::All),
];

/// `+CMGL` / `+CMGR` in PDU mode (`<stat>` a number) and text mode (a name). Text-mode layouts
/// (27.005 §3.4.2, §3.4.3): CMGL `<oa/da>,[<alpha>],[<scts>][,<tooa/toda>,<length>]`, CMGR of a
/// received message `<oa>,[<alpha>],<scts>[,<tooa>,<fo>,<pid>,<dcs>,<sca>,<tosca>,<length>]`, of
/// a stored one `<da>,[<alpha>][,<toda>,<fo>,<pid>,<dcs>,[<vp>],<sca>,<tosca>,<length>]`.
fn header() -> impl Strategy<Value = Line> {
    (
        any::<bool>(),
        0usize..5,
        any::<u32>(),
        of(qtext()),
        of((
            number(),
            scts(),
            of((any::<[u8; 5]>(), of(any::<u8>()), number(), 0u32..200)),
        )),
        any::<bool>(),
        any::<u8>(),
        any::<u32>(),
    )
        .prop_map(|(list, stat, index, alpha, text, lower, style, len)| {
            let (name, status) = STATS[stat];
            let sent = !list && (stat == 2 || stat == 3);
            let mut want = MessageHeader {
                mode: SmsMode::Pdu,
                index: list.then_some(index),
                stat: status,
                alpha: alpha.clone(),
                length: Some(len),
                address: None,
                address_type: None,
                scts: None,
            };
            let mut f = vec![stat.to_string(), qopt(&alpha), len.to_string()];
            if let Some((address, sc, detail)) = text {
                let name = if lower {
                    name.to_lowercase()
                } else {
                    name.to_string()
                };
                f = vec![q(&name), q(&address), qopt(&alpha)];
                want.mode = SmsMode::Text;
                want.length = None;
                want.address = Some(address);
                if !sent {
                    f.push(q(&sc));
                    want.scts = Some(sc);
                }
                if let Some(([ty, fo, pid, dcs, tosca], vp, sca, n)) = detail {
                    f.push(ty.to_string());
                    if !list {
                        f.extend([fo, pid, dcs].iter().map(u8::to_string));
                        if sent {
                            f.push(opt(&vp));
                        }
                        f.extend([q(&sca), tosca.to_string()]);
                    }
                    f.push(n.to_string());
                    want.address_type = Some(ty);
                    want.length = Some(n);
                }
            }
            if list {
                f.insert(0, index.to_string());
                (line("+CMGL", style, &f), Parsed::Cmgl(want))
            } else {
                (line("+CMGR", style, &f), Parsed::Cmgr(want))
            }
        })
}

fn reference() -> impl Strategy<Value = Line> {
    (any::<bool>(), any::<u8>(), of(scts()), any::<u8>()).prop_map(|(sent, mr, ack, style)| {
        let mut f = vec![mr.to_string()];
        f.extend(ack.iter().map(|a| q(a)));
        let want = MessageReference { mr, ack };
        if sent {
            (line("+CMGS", style, &f), Parsed::Cmgs(want))
        } else {
            (line("+CMSS", style, &f), Parsed::Cmss(want))
        }
    })
}

/// `+CPMS?` (`<mem>,<used>,<total>` per storage) and the set response (counts only).
fn cpms_status() -> impl Strategy<Value = Line> {
    (
        vec((mem(), any::<u32>(), any::<u32>()), 1..=3),
        any::<bool>(),
        any::<u8>(),
    )
        .prop_map(|(st, with_mem, style)| {
            let mut f = Vec::new();
            let mut storages = Vec::new();
            for (mem, used, total) in st {
                f.extend(with_mem.then(|| q(&mem)));
                f.extend([used.to_string(), total.to_string()]);
                storages.push(StorageUsage {
                    mem: with_mem.then_some(mem),
                    used,
                    total,
                });
            }
            let want = Parsed::CpmsStatus(CpmsStatus { storages });
            (line("+CPMS", style, &f), want)
        })
}

/// `+CPMS=?`: storage lists; a single storage may come without brackets, but not in front.
fn cpms_support() -> impl Strategy<Value = Line> {
    let lists = vec((vec(mem(), 1..4), any::<bool>()), 1..=3);
    (lists, any::<u8>()).prop_map(|(mut lists, style)| {
        lists[0].1 = false;
        let f: Vec<String> = lists
            .iter()
            .map(|(l, plain)| {
                let quoted: Vec<String> = l.iter().map(|m| q(m)).collect();
                match (plain, l.len()) {
                    (true, 1) => quoted[0].clone(),
                    _ => format!("({})", quoted.join(",")),
                }
            })
            .collect();
        let mut mems = lists.into_iter().map(|l| l.0);
        let want = CpmsSupport {
            mem1: mems.next().unwrap_or_default(),
            mem2: mems.next().unwrap_or_default(),
            mem3: mems.next().unwrap_or_default(),
        };
        (line("+CPMS", style, &f), Parsed::CpmsSupport(want))
    })
}

fn cnmi() -> impl Strategy<Value = Line> {
    (any::<[u8; 5]>(), 1usize..=5, any::<u8>()).prop_map(|(v, k, style)| {
        let f: Vec<String> = v[..k].iter().map(u8::to_string).collect();
        let o = |i: usize| (i < k).then_some(v[i]);
        let want = Cnmi {
            mode: v[0],
            mt: o(1),
            bm: o(2),
            ds: o(3),
            bfr: o(4),
        };
        (line("+CNMI", style, &f), Parsed::Cnmi(want))
    })
}

fn cnmi_support() -> impl Strategy<Value = Line> {
    (vec(range_list(), 1..=5), any::<u8>()).prop_map(|(lists, style)| {
        let (f, values): (Vec<String>, Vec<Vec<u32>>) = lists.into_iter().unzip();
        let at = |i: usize| values.get(i).cloned().unwrap_or_default();
        let want = CnmiSupport {
            mode: at(0),
            mt: at(1),
            bm: at(2),
            ds: at(3),
            bfr: at(4),
        };
        (line("+CNMI", style, &f), Parsed::CnmiSupport(want))
    })
}

// ------------------------------------------------------------------ operators

fn cops() -> impl Strategy<Value = Line> {
    (
        any::<u8>(),
        any::<u8>(),
        qtext(),
        of(any::<u8>()),
        0usize..=3,
        any::<u8>(),
    )
        .prop_map(|(mode, format, oper, act, k, style)| {
            let mut f = vec![mode.to_string(), format.to_string(), q(&oper), opt(&act)];
            f.truncate(1 + k);
            let want = Cops {
                mode,
                format: keep(k, 0, Some(format)),
                oper: keep(k, 1, Some(oper)),
                act: keep(k, 2, act).map(AccessTech::from_code),
            };
            (line("+COPS", style, &f), Parsed::Cops(want))
        })
}

fn operator() -> impl Strategy<Value = (String, OperatorEntry)> {
    (
        any::<u8>(),
        of(qtext()),
        of(qtext()),
        "[0-9]{5,6}",
        of(any::<u8>()),
    )
        .prop_map(|(stat, long, short, numeric, act)| {
            // An empty name is none.
            let name = |n: &Option<String>| q(n.as_deref().unwrap_or(""));
            let mut f = vec![stat.to_string(), name(&long), name(&short), q(&numeric)];
            f.extend(act.map(|a| a.to_string()));
            let entry = OperatorEntry {
                stat: match stat {
                    0 => OperatorStatus::Unknown,
                    1 => OperatorStatus::Available,
                    2 => OperatorStatus::Current,
                    3 => OperatorStatus::Forbidden,
                    v => OperatorStatus::Other(v),
                },
                long,
                short,
                numeric: Some(numeric),
                act: act.map(AccessTech::from_code),
            };
            (format!("({})", f.join(",")), entry)
        })
}

/// Operators, and the mode / format lists behind them or not.
fn cops_list() -> impl Strategy<Value = Line> {
    (vec(operator(), 1..5), any::<bool>(), any::<u8>()).prop_map(|(ops, lists, style)| {
        let (mut f, operators): (Vec<String>, Vec<OperatorEntry>) = ops.into_iter().unzip();
        if lists {
            f.extend(["".into(), "(0-4)".into(), "(0-2)".into()]);
        }
        let want = Parsed::CopsList(CopsList { operators });
        (line("+COPS", style, &f), want)
    })
}

// ------------------------------------------------------------------ +CGEV

fn cgev_event() -> impl Strategy<Value = (String, CgevEvent)> {
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

/// Keywords match in any case; arguments are kept as sent.
fn cgev() -> impl Strategy<Value = Line> {
    const KEYS: [&str; 10] = [
        "NW", "ME", "PDN", "ACT", "DEACT", "REACT", "MODIFY", "DETACH", "CLASS", "REJECT",
    ];
    (cgev_event(), any::<bool>(), any::<u8>()).prop_map(|((text, event), lower, style)| {
        let mut keys = lower;
        let words: Vec<String> = text
            .split(' ')
            .map(|w| {
                keys &= KEYS.contains(&w);
                if keys {
                    w.to_lowercase()
                } else {
                    w.to_string()
                }
            })
            .collect();
        let raw = words.join(" ");
        let l = line("+CGEV", style, std::slice::from_ref(&raw));
        (l, Parsed::Cgev(Cgev { event, raw }))
    })
}

// ------------------------------------------------------------------ the model

/// One strategy per command and form.
fn kinds() -> Vec<BoxedStrategy<Line>> {
    vec![
        registration().boxed(),
        csq().boxed(),
        cesq().boxed(),
        cpin().boxed(),
        caller_id().boxed(),
        call_waiting().boxed(),
        clcc().boxed(),
        ciev().boxed(),
        cind_values().boxed(),
        cind_ranges().boxed(),
        indication().boxed(),
        cds().boxed(),
        cmt().boxed(),
        header().boxed(),
        reference().boxed(),
        cpms_status().boxed(),
        cpms_support().boxed(),
        cnmi().boxed(),
        cnmi_support().boxed(),
        cops().boxed(),
        cops_list().boxed(),
        cgev().boxed(),
    ]
}

/// Values the raw fields stand for (27.007 §8.5, §8.69, §7.2).
fn check_units(p: &Parsed) -> TestCaseResult {
    let f = |x: u8| f64::from(x);
    match p {
        Parsed::Csq(c) => {
            let want = (c.rssi <= 31).then(|| -113 + 2 * i32::from(c.rssi));
            prop_assert_eq!(c.rssi_dbm(), want);
        }
        Parsed::Cesq(c) => {
            prop_assert_eq!(c.rxlev_dbm(), (c.rxlev <= 63).then(|| -111.0 + f(c.rxlev)));
            prop_assert_eq!(c.rscp_dbm(), (c.rscp <= 96).then(|| -121.0 + f(c.rscp)));
            let ecno = (c.ecno <= 49).then(|| -24.5 + f(c.ecno) / 2.0);
            prop_assert_eq!(c.ecno_db(), ecno);
            let rsrq = (c.rsrq <= 34).then(|| -20.0 + f(c.rsrq) / 2.0);
            prop_assert_eq!(c.rsrq_db(), rsrq);
            prop_assert_eq!(c.rsrp_dbm(), (c.rsrp <= 97).then(|| -141.0 + f(c.rsrp)));
        }
        _ => {}
    }
    Ok(())
}

#[test]
fn lines_parse_to_their_values() {
    for kind in kinds() {
        run_model(192, kind, |(line, want)| {
            prop_assert_eq!(parse_all(&line), [want.clone()], "{}", line);
            prop_assert_eq!(parse_any(&line), as_urc(&want), "{}", line);
            check_units(&want)
        });
    }
}

#[test]
fn code_tables() {
    for v in 0..=255u8 {
        let s = RegStatus::from_code(v);
        assert_eq!(s.is_registered(), [1, 5, 6, 7, 9, 10].contains(&v), "{v}");
        assert_eq!(s == RegStatus::Other(v), v > 11, "{v}");
        let act = AccessTech::from_code(v);
        assert_eq!(act == AccessTech::Other(v), v > 13, "{v}");
    }
}

/// Prefix, the fields a line needs, and which of them must be numbers.
fn required() -> Vec<(&'static str, Vec<&'static str>)> {
    vec![
        ("+CREG", vec!["1"]),
        ("+C5GREG", vec!["1"]),
        ("+CSQ", vec!["18", "99"]),
        ("+CESQ", vec!["1", "2", "3", "4", "5", "6"]),
        ("+CLIP", vec!["\"+1\""]),
        ("+CLCC", vec!["1", "0", "4", "0", "0"]),
        ("+CIEV", vec!["\"signal\"", "5"]),
        ("+CMTI", vec!["\"SM\"", "3"]),
        ("+CDSI", vec!["\"SM\"", "3"]),
        ("+CMGL", vec!["1", "1", "", "23"]),
        ("+CMGR", vec!["1", "", "23"]),
        ("+CMT", vec!["", "23"]),
        ("+CDS", vec!["25"]),
        ("+CMGS", vec!["12"]),
        ("+CMSS", vec!["12"]),
        ("+CPMS", vec!["\"SM\"", "1", "20"]),
        ("+CNMI", vec!["2"]),
        ("+COPS", vec!["0"]),
    ]
}

/// A line with a required field cut off or replaced by junk, or of a longer command, is none of
/// the commands; so are forms that look like one but are not.
#[test]
fn broken_lines_are_refused() {
    let strategy = (
        select(required()),
        any::<prop::sample::Index>(),
        any::<bool>(),
        any::<u8>(),
    );
    run_model(512, strategy, |((prefix, fields), at, cut, style)| {
        let text = |f: &[String]| line(prefix, style, f);
        let good: Vec<String> = fields.iter().map(|f| f.to_string()).collect();
        prop_assert_eq!(parse_all(&text(&good)).len(), 1);
        let numbers: Vec<usize> = (0..good.len())
            .filter(|&i| good[i].parse::<u8>().is_ok())
            .collect();
        let mut bad = good.clone();
        if numbers.is_empty() {
            bad[0] = "x".into();
        } else if cut {
            // Cut in front of a required number; with nothing left the line is bare.
            bad.truncate(numbers[at.index(numbers.len())]);
        } else {
            bad[numbers[at.index(numbers.len())]] = "x".into();
        }
        prop_assert_eq!(parse_all(&text(&bad)), [], "{:?}", bad);
        let other = line(&format!("{prefix}S0"), style, &good);
        prop_assert_eq!(parse_all(&other), [], "{}", other);
        prop_assert_eq!(parse_any(&other), None);
        Ok(())
    });
    for l in [
        "+CREG: 256",
        "+CPIN: ",
        // `AT+CLIP?`, `AT+COLP?`, `AT+CCWA` read responses.
        "+CLIP: 1,1",
        "+COLP: 0,2",
        "+CCWA: 1,1",
        "+CLCC: 1,7,0,0,0",
        // A number without its type.
        "+CLCC: 1,0,4,0,0,\"1\"",
        "+CMGL: 1,9,,23",
        "+CMGR: \"REC READ\",6,12,\"+10000000001\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0",
        "+CMGL: 3,\"REC READ\",6,12,\"+10000000001\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0",
        "+CDS: 6,1",
        "+CDS: 6,183,,,\"24/01/02,03:04:05+08\"",
        "+CMT: \"+10000000000\",\"\"",
        "+CMGS: 256",
        "+CPMS: \"SM\",1",
        "+CIND: (\"x\")",
        "+CIND: (\"x\",(0-4294967295))",
    ] {
        assert_eq!(parse_all(l), [], "{l}");
        assert_eq!(parse_any(l), None, "{l}");
    }
    let short = Cgev::parse("+CGEV: NW ACT 1").unwrap();
    assert_eq!(
        (short.event, short.raw.as_str()),
        (CgevEvent::Other, "NW ACT 1")
    );
}

#[test]
fn any_line_parses_or_not() {
    let prefixes = [
        "+CREG",
        "+CGREG",
        "+CEREG",
        "+C5GREG",
        "+CSQ",
        "+CESQ",
        "+CPIN",
        "+CLIP",
        "+CCWA",
        "+COLP",
        "+CLCC",
        "+CIEV",
        "+CIND",
        "+CMTI",
        "+CMGL",
        "+CMGR",
        "+CMT",
        "+CDS",
        "+CPMS",
        "+COPS",
        "+CNMI",
        "+CMGS",
        "+CGEV: NW",
        "+CGEV: ME DEACT",
    ];
    let body = "[0-9A-Fa-f\",()\\\\ \\-é]{0,60}";
    let strategy = prop_oneof![
        1 => "\\PC{0,80}".prop_map(|s| s),
        2 => (select(prefixes.to_vec()), body).prop_map(|(p, b)| format!("{p}: {b}")),
        1 => (select(prefixes.to_vec()), body).prop_map(|(p, b)| format!("{p} {b}")),
    ];
    run_model(512, strategy, |line| {
        let parsed = parse_all(&line);
        let urcs = parsed.iter().filter_map(as_urc).count();
        prop_assert_eq!(parse_any(&line).is_some(), urcs == 1, "{}", line);
        let _ = Cgev::parse_event(&line);
        Ok(())
    });
}
