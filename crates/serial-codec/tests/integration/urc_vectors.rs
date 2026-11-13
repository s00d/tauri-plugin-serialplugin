//! Harvested vectors (see tests/fixtures/urc/README.md) run through `serial_codec::urc`.
//! Entries with `known_disagreement` must still fail; a fixed one fails the test so the mark is
//! removed.

use serde_json::Value;
use serial_codec::urc::*;
use std::collections::BTreeMap;

fn lines(e: &Value) -> Vec<String> {
    let i = &e["input"];
    let mut raw: Vec<String> = Vec::new();
    for k in ["line", "response", "buffer", "at_response"] {
        if let Some(s) = i[k].as_str() {
            raw.push(s.to_string());
        }
    }
    if let Some(a) = i["lines"].as_array() {
        raw.extend(a.iter().filter_map(|v| v.as_str()).map(str::to_string));
    }
    raw.iter()
        .flat_map(|s| s.split(['\r', '\n']))
        .map(str::trim)
        .filter(|l| !l.is_empty() && *l != "OK" && *l != ">" && !l.starts_with("AT"))
        .map(str::to_string)
        .collect()
}

fn mm_state(s: RegStatus) -> &'static str {
    use RegStatus::*;
    match s {
        NotRegistered => "idle",
        Home => "home",
        Searching => "searching",
        Denied => "denied",
        Unknown => "unknown",
        Roaming => "roaming",
        HomeSmsOnly => "home_sms_only",
        RoamingSmsOnly => "roaming_sms_only",
        EmergencyOnly => "emergency_only",
        HomeCsfbNotPreferred => "home_csfb_not_preferred",
        RoamingCsfbNotPreferred => "roaming_csfb_not_preferred",
        AttachedRlos => "attached_rlos",
        Other(_) => "other",
    }
}

/// ModemManager's access-technology names; `None` (absent) is "unknown".
fn mm_act(a: Option<AccessTech>) -> &'static str {
    use AccessTech::*;
    match a {
        None => "unknown",
        Some(Gsm | GsmCompact) => "gsm",
        Some(Utran) => "umts",
        Some(GsmEgprs) => "edge",
        Some(UtranHsdpa) => "hsdpa",
        Some(UtranHsupa) => "hsupa",
        Some(UtranHsdpaHsupa) => "hspa",
        Some(Eutran | Eutra5gcn) => "lte",
        Some(Nr5gcn) => "5gnr",
        Some(_) => "other",
    }
}

fn s(v: &Value) -> Option<&str> {
    v.as_str()
}

fn check_reg(e: &Value, ls: &[String]) -> Result<(), String> {
    let x = &e["expected"];
    let r = ls
        .iter()
        .rev()
        .find_map(|l| Registration::parse(l))
        .ok_or("no registration line parsed")?;
    if let Some(st) = x["gsm_reg_status"].as_u64() {
        // chan_dongle: raw stat and verbatim LAC/CI
        let hex = |k: &str| u64::from_str_radix(x[k].as_str().unwrap(), 16).unwrap();
        let got = (
            stat_code(r.stat),
            r.lac.map(u64::from),
            r.ci,
            r.stat.is_registered(),
        );
        let want = (
            st,
            Some(hex("lac")),
            Some(hex("ci")),
            x["gsm_reg"].as_bool().unwrap(),
        );
        return (got == want)
            .then_some(())
            .ok_or(format!("{got:?} != {want:?}"));
    }
    let got = (
        mm_state(r.stat),
        r.lac.unwrap_or(0) as u64,
        r.ci.unwrap_or(0),
        mm_act(r.act),
        r.kind == RegKind::Cgreg,
        r.kind == RegKind::Cereg,
        r.kind == RegKind::C5greg,
    );
    let want = (
        s(&x["state"]).unwrap(),
        x["lac"].as_u64().unwrap(),
        x["ci"].as_u64().unwrap(),
        s(&x["act"]).unwrap(),
        x["cgreg"].as_bool().unwrap(),
        x["cereg"].as_bool().unwrap(),
        x["c5greg"].as_bool().unwrap(),
    );
    (got == want)
        .then_some(())
        .ok_or(format!("{got:?} != {want:?}"))
}

fn stat_code(s: RegStatus) -> u64 {
    (0..=255u8).find(|&v| RegStatus::from_code(v) == s).unwrap() as u64
}

fn mm_cgev(ev: &CgevEvent) -> (&'static str, Value) {
    use serde_json::json;
    use CgevEvent::*;
    let c = |c: &Option<u32>| c.unwrap_or(0);
    match ev {
        Reject { pdp_type, pdp_addr } => (
            "reject",
            json!({"pdp_type": pdp_type, "pdp_addr": pdp_addr, "cid": 0}),
        ),
        NwReact {
            pdp_type,
            pdp_addr,
            cid,
        } => (
            "nw_react",
            json!({"pdp_type": pdp_type, "pdp_addr": pdp_addr, "cid": c(cid)}),
        ),
        NwDeactPdp {
            pdp_type,
            pdp_addr,
            cid,
        } => (
            "nw_deact_pdp",
            json!({"pdp_type": pdp_type, "pdp_addr": pdp_addr, "cid": c(cid)}),
        ),
        MeDeactPdp {
            pdp_type,
            pdp_addr,
            cid,
        } => (
            "me_deact_pdp",
            json!({"pdp_type": pdp_type, "pdp_addr": pdp_addr, "cid": c(cid)}),
        ),
        MePdnAct { cid, .. } => ("me_act_primary", json!({ "cid": cid })),
        MePdnDeact { cid } => ("me_deact_primary", json!({ "cid": cid })),
        NwPdnAct { cid } => ("nw_act_primary", json!({ "cid": cid })),
        NwPdnDeact { cid } => ("nw_deact_primary", json!({ "cid": cid })),
        MeAct {
            p_cid,
            cid,
            event_type,
        } => (
            "me_act_secondary",
            json!({"cid": cid, "parent_cid": p_cid, "event_type": event_type}),
        ),
        MeDeact {
            p_cid,
            cid,
            event_type,
        } => (
            "me_deact_secondary",
            json!({"cid": cid, "parent_cid": p_cid, "event_type": event_type}),
        ),
        NwAct {
            p_cid,
            cid,
            event_type,
        } => (
            "nw_act_secondary",
            json!({"cid": cid, "parent_cid": p_cid, "event_type": event_type}),
        ),
        NwDeact {
            p_cid,
            cid,
            event_type,
        } => (
            "nw_deact_secondary",
            json!({"cid": cid, "parent_cid": p_cid, "event_type": event_type}),
        ),
        NwDetach => ("nw_detach", json!({})),
        MeDetach => ("me_detach", json!({})),
        NwClass { .. } => ("nw_class", json!({})),
        MeClass { .. } => ("me_class", json!({})),
        _ => ("other", json!({})),
    }
}

fn eq<T: PartialEq + std::fmt::Debug>(got: T, want: T) -> Result<(), String> {
    (got == want)
        .then_some(())
        .ok_or(format!("{got:?} != {want:?}"))
}

/// `yy/MM/dd,hh:mm:ss±qq` (quarter hours) -> `20yy-MM-ddThh:mm:ss±hh:mm`.
fn scts_iso(s: &str) -> String {
    let (dt, tz) = s.split_at(s.len() - 3);
    let q: i32 = tz.parse().unwrap();
    let (d, t) = dt.split_once(',').unwrap();
    let d = d.replace('/', "-");
    let sign = if q < 0 { '-' } else { '+' };
    let m = q.abs() * 15;
    format!("20{d}T{t}{sign}{:02}:{:02}", m / 60, m % 60)
}

fn check(group: &str, e: &Value) -> Result<(), String> {
    let x = &e["expected"];
    let ls = lines(e);
    let first = ls.first().cloned().unwrap_or_default();
    match group {
        "+CREG" | "+CGREG" | "+CEREG" | "+C5GREG" | "+CREG?" | "+CGREG?" | "+CEREG?"
        | "+C5GREG?" => check_reg(e, &ls),
        "+CCWA" => {
            let c = CallWaiting::parse(&first).ok_or("no parse")?;
            eq(
                (c.number.as_str(), c.number_type, c.class),
                (
                    s(&x["number"]).unwrap(),
                    x["type"].as_u64().map(|v| v as u8),
                    x["class"].as_u64().map(|v| v as u8),
                ),
            )
        }
        "+CLIP" => {
            let c = CallerId::parse_clip(&first).ok_or("no parse")?;
            eq(
                (c.number.as_str(), c.number_type),
                (
                    s(&x["number"]).unwrap(),
                    x["type"].as_u64().map(|v| v as u8),
                ),
            )
        }
        "+CRING" | "RING" => {
            let clip = ls
                .iter()
                .find_map(|l| CallerId::parse_clip(l))
                .ok_or("no +CLIP")?;
            if let Some(n) = s(&x["number"]) {
                eq(clip.number.as_str(), n)?;
            }
            Ok(())
        }
        "+CMTI" | "+CDSI" => {
            let p = if group == "+CMTI" {
                MessageIndication::parse_cmti(&first)
            } else {
                MessageIndication::parse_cdsi(&first)
            };
            if x["index"].as_i64() == Some(-1) {
                return eq(p, None);
            }
            let p = p.ok_or("no parse")?;
            eq(p.index as u64, x["index"].as_u64().unwrap())?;
            if let Some(m) = s(&x["memory"]) {
                eq(p.mem.as_str(), m)?;
            }
            Ok(())
        }
        "+CDS" => {
            let c = Cds::parse(&first).ok_or("no parse")?;
            eq(c.length, x["length"].as_u64().map(|v| v as u32))?;
            eq(ls.get(1).map(String::as_str), s(&x["pdu"]))
        }
        "+CGEV" => {
            let c = if first.to_ascii_uppercase().starts_with("+CGEV") {
                Cgev::parse(&first).ok_or("no parse")?
            } else {
                Cgev::parse_event(&first)
            };
            let (ty, fields) = mm_cgev(&c.event);
            eq(ty, s(&x["type"]).unwrap())?;
            for (k, v) in x.as_object().unwrap() {
                if k != "type" {
                    eq(&fields[k], v)?;
                }
            }
            Ok(())
        }
        "+CESQ" => {
            let c = Cesq::parse(&first).ok_or("no parse")?;
            let raw = [c.rxlev, c.ber, c.rscp, c.ecno, c.rsrq, c.rsrp];
            let want: Vec<u8> = [
                "rxlev",
                "ber",
                "rscp_level",
                "ecn0_level",
                "rsrq_level",
                "rsrp_level",
            ]
            .iter()
            .map(|k| x[*k].as_u64().unwrap() as u8)
            .collect();
            eq(raw.to_vec(), want)?;
            let sig = &x["signal"];
            let close = |got: Option<f64>, want: &Value| match (got, want.as_f64()) {
                (Some(g), Some(w)) => (g - w).abs() < 0.1,
                (None, None) => true,
                _ => false,
            };
            let ok = close(c.rxlev_dbm(), &sig["gsm"]["rssi_dbm"])
                && (sig["umts"].is_null() == c.rscp_dbm().is_none())
                && close(c.rsrq_db(), &sig["lte"]["rsrq_db"])
                && close(c.rsrp_dbm(), &sig["lte"]["rsrp_dbm"]);
            ok.then_some(()).ok_or(format!("{c:?} vs {sig}"))
        }
        "+CIND=?" => {
            let c = CindRanges::parse(&first).ok_or("no parse")?;
            let got: Vec<Value> = c
                .indicators
                .iter()
                .map(|i| {
                    serde_json::json!({
                        "name": i.name.to_lowercase().replace(' ', ""),
                        "index": i.index,
                        "min": i.values.iter().min(),
                        "max": i.values.iter().max(),
                    })
                })
                .collect();
            eq(&Value::Array(got), &x["indicators"])
        }
        "+CLCC" => {
            if let Some(calls) = x["calls"].as_array() {
                let n = ls.iter().filter_map(|l| Clcc::parse(l)).count();
                return eq(n, calls.len());
            }
            let c = Clcc::parse(&first);
            if x["ret"] == -1 {
                return eq(c, None);
            }
            let c = c.ok_or("no parse")?;
            let mut want = x.clone();
            let stat = want.as_object_mut().unwrap().remove("stat");
            eq(stat, Some(4.into()))?;
            eq(c.stat, CallState::Incoming)?;
            let got = serde_json::json!({
                "ret": 0, "index": c.id, "dir": c.dir as u8, "mode": c.mode,
                "mpty": c.mpty as u8, "number": c.number, "toa": c.number_type,
            });
            eq(&got, &want)
        }
        "+CMGL" => {
            if x["ok"] == true {
                return MessageHeader::parse_cmgl(&first)
                    .map(|_| ())
                    .ok_or("no parse".into());
            }
            let msgs = x["messages"].as_array().unwrap();
            let mut got = Vec::new();
            for pair in ls.chunks(2) {
                let h = MessageHeader::parse_cmgl(&pair[0]).ok_or("no header")?;
                got.push((h, pair.get(1).cloned()));
            }
            eq(got.len(), msgs.len())?;
            for ((h, body), m) in got.iter().zip(msgs) {
                if let Some(pdu) = s(&m["pdu"]) {
                    eq(h.mode, SmsMode::Pdu)?;
                    eq(h.index.map(u64::from), m["index"].as_u64())?;
                    eq(h.stat as u64, m["status"].as_u64().unwrap())?;
                    eq(body.as_deref(), Some(pdu))?;
                } else {
                    eq(h.mode, SmsMode::Text)?;
                    eq(h.address.as_deref(), s(&m["number"]))?;
                    eq(h.stat as u64, m["status"]["value"].as_u64().unwrap())?;
                    eq(h.scts.as_deref().map(scts_iso).as_deref(), s(&m["time"]))?;
                    eq(body.as_deref(), s(&m["text"]))?;
                }
            }
            Ok(())
        }
        "+CMGR" => {
            let h = MessageHeader::parse_cmgr(&first).ok_or("no parse")?;
            if x["ok"] == true {
                return eq(h.mode, SmsMode::Text);
            }
            let pdu = s(&x["pdu"]).or(s(&e["input"]["pdu"])).unwrap();
            if let Some(st) = x["status"].as_u64() {
                eq(h.stat as u64, st)?;
            }
            eq(h.mode, SmsMode::Pdu)?;
            eq(ls.get(1).map(String::as_str), Some(pdu))
        }
        "+CMGS" => {
            let m = ls
                .iter()
                .find_map(|l| MessageReference::parse_cmgs(l))
                .ok_or("no +CMGS")?;
            eq(m.mr as u64, x["reference"].as_u64().unwrap())
        }
        "+CNMI=?" => CnmiSupport::parse(&first)
            .map(|_| ())
            .ok_or("no parse".into()),
        "+COPS=?" => {
            let c = CopsList::parse(&first).ok_or("no parse")?;
            let mut got: Vec<Value> = c
                .operators
                .iter()
                .map(|o| {
                    let stat = match o.stat {
                        OperatorStatus::Current => "current",
                        OperatorStatus::Available => "available",
                        OperatorStatus::Forbidden => "forbidden",
                        _ => "unknown",
                    };
                    // ModemManager defaults a missing <AcT> to GSM.
                    serde_json::json!({"status": stat, "long": o.long, "short": o.short,
                        "numeric": o.numeric, "act": mm_act(o.act.or(Some(AccessTech::Gsm)))})
                })
                .collect();
            let mut want = x["networks"].as_array().cloned().unwrap_or_default();
            let key = |v: &Value| v.to_string();
            got.sort_by_key(key);
            want.sort_by_key(key);
            eq(got, want)
        }
        "+COPS?" => {
            let c = Cops::parse(&first).ok_or("no parse")?;
            let op = s(&x["operator"]).or(s(&x["network_name"]));
            eq(c.oper.as_deref(), op)?;
            if let Some(m) = x["mode"].as_u64() {
                eq(
                    (c.mode as u64, c.format.map(u64::from), mm_act(c.act)),
                    (m, x["format"].as_u64(), s(&x["act"]).unwrap()),
                )?;
            }
            Ok(())
        }
        "+CPIN" => Cpin::parse(&first).map(|_| ()).ok_or("no parse".into()),
        "+CPIN?" => {
            let lock = match Cpin::parse(&first) {
                None => "unknown".to_string(),
                Some(p) => serde_json::to_value(&p)
                    .unwrap()
                    .as_str()
                    .unwrap()
                    .chars()
                    .flat_map(|c| {
                        if c.is_ascii_uppercase() {
                            vec!['_', c.to_ascii_lowercase()]
                        } else {
                            vec![c]
                        }
                    })
                    .collect(),
            };
            eq(lock.as_str(), s(&x["lock"]).unwrap())
        }
        "+CPMS=?" => {
            let c = CpmsSupport::parse(&first).ok_or("no parse")?;
            let lower = |v: &[String]| {
                let mut v: Vec<String> = v.iter().map(|m| m.to_lowercase()).collect();
                v.sort();
                v
            };
            if x["read"].is_object() {
                // gammu: ME/SM/SR availability for read (mem1) and save (mem2)
                for (k, list) in [("read", &c.mem1), ("save", &c.mem2)] {
                    for (mem, avail) in x[k].as_object().unwrap() {
                        eq(
                            (k, mem.as_str(), list.contains(mem)),
                            (k, mem.as_str(), avail.as_bool().unwrap()),
                        )?;
                    }
                }
                return Ok(());
            }
            for (k, list) in [("mem1", &c.mem1), ("mem2", &c.mem2), ("mem3", &c.mem3)] {
                let mut want: Vec<String> = x[k]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap().to_string())
                    .collect();
                want.sort();
                eq(lower(list), want)?;
            }
            Ok(())
        }
        "+CPMS?" => {
            let c = CpmsStatus::parse(&first).ok_or("no parse")?;
            let known = ["sm", "me", "mt", "sr", "bm", "ta"];
            let name = |i: usize| {
                let m = c.storages[i].mem.clone().unwrap().to_lowercase();
                if known.contains(&m.as_str()) {
                    m
                } else {
                    "unknown".into()
                }
            };
            eq(
                (name(0), name(1)),
                (s(&x["mem1"]).unwrap().into(), s(&x["mem2"]).unwrap().into()),
            )
        }
        "+CSQ" => {
            let c = ls.iter().find_map(|l| Csq::parse(l));
            match x["rssi"].as_u64() {
                Some(r) => eq(c.map(|c| c.rssi as u64), Some(r)),
                None => eq(c, None),
            }
        }
        _ => Err(format!("no checker for {group}")),
    }
}

fn run(file: &str) -> BTreeMap<String, (usize, usize, usize)> {
    let path = format!("{}/tests/fixtures/urc/{file}", env!("CARGO_MANIFEST_DIR"));
    let data: BTreeMap<String, Vec<Value>> =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let mut stats = BTreeMap::new();
    let mut errors = Vec::new();
    for (group, entries) in &data {
        for e in entries {
            let project = e["source"].as_str().unwrap().split('/').next().unwrap();
            let st = stats
                .entry(format!("{project} {group}"))
                .or_insert((0, 0, 0));
            let res = check(group, e);
            match (res, e.get("known_disagreement")) {
                (Ok(()), None) => st.0 += 1,
                (Err(_), Some(_)) => st.1 += 1,
                (Ok(()), Some(_)) => errors.push(format!(
                    "{group} {}: passes now, drop known_disagreement",
                    e["input"]
                )),
                (Err(m), None) => {
                    st.2 += 1;
                    errors.push(format!("{group} {} [{}]: {m}", e["input"], e["source"]))
                }
            }
        }
    }
    for (k, (p, d, f)) in &stats {
        println!("{file} {k}: pass {p}, known disagreement {d}, fail {f}");
    }
    assert!(errors.is_empty(), "{}", errors.join("\n"));
    stats
}

#[test]
fn urc_vectors() {
    run("at_urc.json");
}

#[test]
fn response_vectors() {
    run("at_responses.json");
}
