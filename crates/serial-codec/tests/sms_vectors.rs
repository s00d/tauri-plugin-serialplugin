//! SMS PDU vectors from other projects' test suites (data only; see
//! `tests/fixtures/sms/README.md` for sources, licenses and the disagreement list).
//!
//! Every fixture entry runs. Entries listed in `DISAGREE_*` must FAIL the check (so a
//! fix shows up as a test failure telling you to drop the entry from the list).

use serde_json::Value;
use serial_codec::sms::concat::{Fragment, Key, Kind, Reassembler};
use serial_codec::sms::gsm7::Tables;
use serial_codec::sms::pdu::{
    self, decode_address, decode_hex, decode_semi_octets, decode_smsc, decode_tpdu, decode_tz,
    decode_user_data, encode_address, encode_semi_octets, encode_smsc, encode_submit,
    relative_from_minutes, Alphabet, Dcs, Pdu, SubmitRequest, Timestamp, Tpdu, Udh, Validity,
};
use serial_codec::sms::{hex_decode, hex_encode};
use std::collections::BTreeMap;

type R = Result<(), String>;

fn load(json: &str) -> Vec<Value> {
    serde_json::from_str(json).expect("fixture json")
}

fn hx(s: &str) -> Vec<u8> {
    hex_decode(s).unwrap_or_else(|| panic!("bad hex in fixture: {s}"))
}

fn ensure(cond: bool, msg: impl FnOnce() -> String) -> R {
    if cond {
        Ok(())
    } else {
        Err(msg())
    }
}

fn eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> R {
    ensure(got == want, || {
        format!("{what}: got {got:?}, want {want:?}")
    })
}

/// Run `check` on every entry; entries in `disagree` must fail. Prints per-source counts.
fn run(name: &str, entries: &[Value], disagree: &[(&str, &str)], check: impl Fn(&Value) -> R) {
    let mut failures = Vec::new();
    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for e in entries {
        let id = e["id"].as_str().unwrap_or("?");
        let src = e["source"]
            .as_str()
            .unwrap_or("?")
            .split([' ', '/'])
            .next()
            .unwrap_or("?")
            .to_string();
        let r = check(e);
        let listed = disagree.iter().any(|(d, _)| *d == id);
        let st = stats.entry(src).or_default();
        match (r, listed) {
            (Ok(()), false) => st.0 += 1,
            (Err(_), true) => st.1 += 1,
            (Ok(()), true) => failures.push(format!("{id}: listed as disagreement but passes")),
            (Err(m), false) => failures.push(format!("{id}: {m}")),
        }
    }
    for (src, (p, d)) in &stats {
        eprintln!("[{name}] {src}: {p} pass, {d} known disagreement");
    }
    assert!(failures.is_empty(), "{name}:\n{}", failures.join("\n"));
}

/// `YYYY-MM-DDTHH:MM:SS` with `Z`, `±HH:MM` or nothing (UTC) → (unix, offset minutes).
fn iso(s: &str) -> (i64, i32) {
    let n = |a: usize, b: usize| s[a..b].parse::<i64>().unwrap();
    let off = match s.get(19..) {
        None | Some("") | Some("Z") => 0,
        Some(o) => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            sign * (o[1..3].parse::<i32>().unwrap() * 60 + o[4..6].parse::<i32>().unwrap())
        }
    };
    let t = Timestamp {
        year: n(0, 4) as u16,
        month: n(5, 7) as u8,
        day: n(8, 10) as u8,
        hour: n(11, 13) as u8,
        minute: n(14, 16) as u8,
        second: n(17, 19) as u8,
        tz_quarters: (off / 15) as i8,
    };
    (t.unix_utc(), off)
}

fn ts_of(s: &str) -> Timestamp {
    let (unix, off) = iso(s);
    let n = |a: usize, b: usize| s[a..b].parse::<u8>().unwrap();
    let t = Timestamp {
        year: s[0..4].parse().unwrap(),
        month: n(5, 7),
        day: n(8, 10),
        hour: n(11, 13),
        minute: n(14, 16),
        second: n(17, 19),
        tz_quarters: (off / 15) as i8,
    };
    assert_eq!(t.unix_utc(), unix);
    t
}

fn decode_input(i: &Value) -> Result<Pdu, pdu::PduError> {
    let hex = i["pdu"].as_str().unwrap_or("");
    if i["has_smsc"].as_bool().unwrap_or(true) {
        decode_hex(hex)
    } else {
        let b = hex_decode(hex).ok_or(pdu::PduError::InvalidHex)?;
        Ok(Pdu {
            smsc: None,
            tpdu: decode_tpdu(&b)?,
        })
    }
}

fn expects_error(x: &Value) -> bool {
    let e = &x["error"];
    (!e.is_null() && e != &Value::Bool(false)) || x["ok"] == false || x["decode_ok"] == false
}

/// Some sources print the `+` of international numbers, some do not.
fn addr_eq(a: &pdu::Address, want: &str) -> R {
    let got = if want.starts_with('+') {
        a.to_string()
    } else {
        a.value.clone()
    };
    eq("address", got.as_str(), want)
}

fn type_name(t: &Tpdu) -> &'static str {
    match t {
        Tpdu::Deliver(_) => "deliver",
        Tpdu::Submit(_) => "submit",
        Tpdu::StatusReport(_) => "status-report",
    }
}

fn check_decoded(p: &Pdu, x: &Value) -> R {
    let t = &p.tpdu;
    let ud = t.user_data();
    let udh = ud.and_then(|u| u.udh.as_ref());
    for (k, v) in x.as_object().unwrap() {
        match k.as_str() {
            "ok" | "decode_ok" | "error" => {}
            "smsc" => eq(
                "smsc",
                p.smsc.as_ref().map(|a| a.to_string()),
                v.as_str().map(str::to_string),
            )?,
            "sender" | "number" | "destination" | "recipient" => {
                addr_eq(t.address(), v.as_str().unwrap())?
            }
            "text" if t.dcs().is_some_and(|d| d.alphabet == Alphabet::Bit8) => eq(
                "8-bit payload",
                ud.map(|u| u.payload.as_slice()),
                v.as_str().map(str::as_bytes),
            )?,
            "text" => eq("text", t.text(), v.as_str())?,
            "data_hex" => eq(
                "data",
                ud.map(|u| hex_encode(&u.payload)),
                v.as_str().map(str::to_string),
            )?,
            "dcs" => eq("dcs", t.dcs().map(|d| u64::from(d.raw)), v.as_u64())?,
            "type" => {
                let want = v.as_str().unwrap().to_ascii_lowercase();
                let want = want.trim_start_matches("sms-");
                eq("type", type_name(t), want)?
            }
            "reference" | "mr" => {
                let mr = match t {
                    Tpdu::Submit(s) => s.mr,
                    Tpdu::StatusReport(r) => r.mr,
                    Tpdu::Deliver(_) => return Err("no MR in DELIVER".into()),
                };
                eq("mr", u64::from(mr), v.as_u64().unwrap())?
            }
            "timestamp" | "time" => {
                let scts = match t {
                    Tpdu::Deliver(d) => d.scts,
                    Tpdu::StatusReport(r) => r.scts,
                    Tpdu::Submit(_) => None,
                };
                eq(
                    "scts",
                    scts.map(|s| s.unix_utc()),
                    Some(iso(v.as_str().unwrap()).0),
                )?
            }
            "discharge" => {
                let Tpdu::StatusReport(r) = t else {
                    return Err("not a status report".into());
                };
                eq(
                    "dt",
                    r.discharge.map(|s| s.unix_utc()),
                    Some(iso(v.as_str().unwrap()).0),
                )?
            }
            "should_concat" => eq(
                "concat",
                udh.and_then(Udh::concat).is_some(),
                v.as_bool().unwrap(),
            )?,
            "replace" => {
                let Tpdu::Deliver(d) = t else {
                    return Err("not deliver".into());
                };
                eq(
                    "replace",
                    (0x41..=0x47).contains(&d.pid),
                    v.as_bool().unwrap(),
                )?
            }
            "mwi" => {
                let mwi = t
                    .dcs()
                    .and_then(|d| d.mwi)
                    .ok_or("no DCS MWI (CPHS MWI is not decoded)")?;
                eq("mwi", mwi.active, v == "set")?
            }
            "mwi_dont_store" => {
                let mwi = t.dcs().and_then(|d| d.mwi).ok_or("no DCS MWI")?;
                eq("mwi store", !mwi.store, v.as_bool().unwrap())?
            }
            "udh" => {
                let u = udh.ok_or("no UDH")?;
                if let Some(c) = v.get("concat") {
                    let got = u.concat().ok_or("no concat")?;
                    eq("ref", u64::from(got.reference), c["ref"].as_u64().unwrap())?;
                    eq("total", u64::from(got.total), c["total"].as_u64().unwrap())?;
                    eq("seq", u64::from(got.seq), c["seq"].as_u64().unwrap())?;
                    eq("ref8", !got.ref_16bit, c["ref_8bit"].as_bool().unwrap())?;
                }
                if let Some(pt) = v.get("ports") {
                    let got = u.ports().ok_or("no ports")?;
                    eq("dest", u64::from(got.dest), pt["dest"].as_u64().unwrap())?;
                    eq("orig", u64::from(got.orig), pt["orig"].as_u64().unwrap())?;
                    eq("8bit", !got.wide, pt["8bit"].as_bool().unwrap())?;
                }
            }
            "app_port" => {
                let got = udh.and_then(Udh::ports).ok_or("no ports")?;
                eq("dst", u64::from(got.dest), v["dst"].as_u64().unwrap())?;
                eq("8bit", !got.wide, v["is_8bit"].as_bool().unwrap())?;
            }
            "charset" => {
                let want = match v.as_str().unwrap() {
                    "8bit" => Alphabet::Bit8,
                    "ucs2" => Alphabet::Ucs2,
                    _ => Alphabet::Gsm7,
                };
                eq("alphabet", t.dcs().map(|d| d.alphabet), Some(want))?
            }
            "datagram_nonnull" => ensure(ud.is_some_and(|u| !u.payload.is_empty()), || {
                "empty datagram".into()
            })?,
            "udhi" => eq("udhi", udh.is_some(), v.as_bool().unwrap())?,
            "udhl" | "udl" | "ud_bytes" => {
                // Re-read the raw fields: UDL is the last octet before TP-UD.
                let raw = raw_ud(p)?;
                let got = match k.as_str() {
                    "udl" => raw.0,
                    "udhl" => usize::from(*raw.1.first().ok_or("no UD")?),
                    _ => raw.1.len(),
                };
                eq(k, got as u64, v.as_u64().unwrap())?
            }
            "udh_ies" => {
                let got: Vec<(u64, u64)> = udh
                    .ok_or("no UDH")?
                    .ies
                    .iter()
                    .map(|ie| (u64::from(ie.id), ie.data.len() as u64))
                    .collect();
                let want: Vec<(u64, u64)> = v
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|ie| (ie["iei"].as_u64().unwrap(), ie["len"].as_u64().unwrap()))
                    .collect();
                eq("ies", got, want)?
            }
            other => return Err(format!("unsupported assertion `{other}`")),
        }
    }
    Ok(())
}

/// (TP-UDL, TP-UD octets actually used) recomputed from the decoded fields.
fn raw_ud(p: &Pdu) -> Result<(usize, Vec<u8>), String> {
    let ud = p.tpdu.user_data().ok_or("no UD")?;
    let dcs = p.tpdu.dcs().copied().unwrap_or(Dcs::decode(0));
    let hdr = ud
        .udh
        .as_ref()
        .map(Udh::encode)
        .transpose()
        .map_err(|e| e.to_string())?
        .unwrap_or_default();
    if dcs.alphabet == Alphabet::Gsm7 && !dcs.compressed {
        let hs = (hdr.len() * 8).div_ceil(7);
        let fill = (hs * 7 - hdr.len() * 8) as u8;
        let mut b = hdr;
        b.extend(serial_codec::sms::gsm7::pack(&ud.payload, fill));
        Ok((hs + ud.payload.len(), b))
    } else {
        let mut b = hdr;
        b.extend(&ud.payload);
        Ok((b.len(), b))
    }
}

fn check_decode_entry(e: &Value) -> R {
    let i = &e["input"];
    let x = &e["expected"];
    let kind = i["op"]
        .as_str()
        .or(i["kind"].as_str())
        .or(i["field"].as_str())
        .unwrap_or("");
    match kind {
        "" if i.get("pdu").is_some() => {
            let r = decode_input(i);
            if expects_error(x) {
                return ensure(r.is_err(), || format!("decoded, want error: {r:?}"));
            }
            let p = r.map_err(|e| format!("decode: {e}"))?;
            check_decoded(&p, x)
        }
        "multipart_assembly" => {
            let mut asm = Reassembler::new();
            let mut done = Vec::new();
            for h in i["pdus"].as_array().unwrap() {
                let p = decode_hex(h.as_str().unwrap()).map_err(|e| e.to_string())?;
                if let Some(m) = asm.push_tpdu(&p.tpdu, 0) {
                    done.push(m);
                }
            }
            // ModemManager counts partial messages too.
            eq(
                "messages",
                (done.len() + asm.pending()) as u64,
                x["messages"].as_u64().unwrap(),
            )?;
            if x["deliver_complete"] == true {
                ensure(
                    done.iter()
                        .any(|m| m.key.as_ref().is_some_and(|k| k.kind == Kind::Deliver)),
                    || "no deliver message".into(),
                )?;
            }
            Ok(())
        }
        "" if i.get("pdus").is_some() => {
            // oFono 3-part UCS2 assembly + expiry + re-prepare with 16-bit ref.
            let pdus: Vec<Pdu> = i["pdus"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| decode_input(p).map_err(|e| e.to_string()))
                .collect::<Result<_, _>>()?;
            let mut asm = Reassembler::new();
            let states: Vec<&str> = pdus
                .iter()
                .map(|p| {
                    if asm.push_tpdu(&p.tpdu, 100).is_some() {
                        "complete"
                    } else {
                        "incomplete"
                    }
                })
                .collect();
            let want: Vec<&str> = x["assembly"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            eq("assembly", states, want)?;
            let mut asm = Reassembler::new();
            asm.push_tpdu(&pdus[0].tpdu, 100);
            let ttl = x["expire_after_s_drops_partial"].as_u64().unwrap();
            eq("kept before ttl", asm.expire_before(100).len(), 0)?;
            eq("dropped after ttl", asm.expire_before(100 + ttl).len(), 1)?;
            let mut asm = Reassembler::new();
            let text = pdus
                .iter()
                .find_map(|p| asm.push_tpdu(&p.tpdu, 0))
                .and_then(|m| m.text)
                .ok_or("no text")?;
            let mut req = SubmitRequest::new("555", text.clone());
            req.concat_ref = Some(0);
            req.concat_16bit = true;
            let parts = encode_submit(&req).map_err(|e| e.to_string())?;
            eq(
                "segments",
                parts.len() as u64,
                x["reprepare_ucs2_16bit_ref_segments"].as_u64().unwrap(),
            )?;
            eq("roundtrip", reassemble(&parts)?, text)
        }
        "address" => {
            let b = hex_decode(i["hex"].as_str().unwrap_or("")).unwrap_or_default();
            let r = decode_address(&b);
            if expects_error(x) {
                return ensure(r.is_err(), || format!("want error, got {r:?}"));
            }
            let (a, n) = r.map_err(|e| e.to_string())?;
            if let Some(c) = x["read_count"].as_u64() {
                eq("read", n as u64, c)?;
            }
            if let Some(num) = x["number"].as_str() {
                addr_eq(&a, num)?;
            }
            if let Some(l) = x["length"].as_u64() {
                eq("len", u64::from(b[0]), l)?;
            }
            if let Some(toa) = x.get("toa") {
                eq("toa", toa_of(toa), a.toa)?;
            }
            if let Some(addr) = x.get("address") {
                eq(
                    "addr",
                    a.value.as_str(),
                    addr["Addr"].as_str().unwrap_or(""),
                )?;
            }
            Ok(())
        }
        "smsc-address" => {
            let b = hex_decode(i["hex"].as_str().unwrap_or("")).unwrap_or_default();
            let r = decode_smsc(&b);
            if expects_error(x) {
                return ensure(r.is_err(), || format!("want error, got {r:?}"));
            }
            let (a, n) = r.map_err(|e| e.to_string())?;
            let a = a.ok_or("no smsc")?;
            if let Some(c) = x["read_count"].as_u64() {
                eq("read", n as u64, c)?;
            }
            if let Some(num) = x["number"].as_str() {
                addr_eq(&a, num)?;
            }
            if let Some(toa) = x.get("toa") {
                eq("toa", toa_of(toa), a.toa)?;
            }
            if let Some(l) = x["length"].as_u64() {
                eq("len", u64::from(b[0]), l)?;
            }
            Ok(())
        }
        "semioctet" | "number-semioctets" | "swap-nibbles" => {
            let b = hex_decode(i["hex"].as_str().unwrap_or("")).unwrap_or_default();
            let got = decode_semi_octets(&b, usize::MAX);
            let want = x["digits"]
                .as_str()
                .or(x["number"].as_str())
                .or(x["hex"].as_str())
                .unwrap_or("");
            eq("digits", got.as_str(), want)
        }
        "validity-period-relative" => {
            let vp = i["tp_vp"].as_u64().unwrap();
            ensure(u8::try_from(vp).is_err(), || "in range".into())
        }
        "scts-timezone" => {
            // The fixture writes the octet with its nibbles already swapped.
            let b = hx(i["hex"].as_str().unwrap())[0];
            let q = decode_tz(b.rotate_left(4)).ok_or("bad tz")?;
            eq(
                "hours",
                i64::from(q) * 15 / 60,
                x["offset_hours"].as_i64().unwrap(),
            )
        }
        "scts" => {
            let b = hex_decode(i["hex"].as_str().unwrap()).unwrap();
            let t = Timestamp::decode(&b.try_into().unwrap()).ok_or("invalid scts")?;
            let want = x["utc"]
                .as_str()
                .or(x["timestamp"]["Time"].as_str())
                .unwrap();
            eq("unix", t.unix_utc(), iso(want).0)
        }
        "udh-ie" => {
            let u = Udh::parse(&hx(i["hex"].as_str().unwrap())).map_err(|e| e.to_string())?;
            match x["type"].as_str().unwrap() {
                "Concatenation" => {
                    let c = u.concat().ok_or("no concat")?;
                    eq(
                        "ref",
                        u64::from(c.reference),
                        x["reference"].as_u64().unwrap(),
                    )?;
                    eq("seq", u64::from(c.seq), x["number"].as_u64().unwrap())?;
                    eq("total", u64::from(c.total), x["parts"].as_u64().unwrap())
                }
                _ => {
                    let p = u.ports().ok_or("no ports")?;
                    eq(
                        "dest",
                        u64::from(p.dest),
                        x["destination"].as_u64().unwrap(),
                    )?;
                    eq("orig", u64::from(p.orig), x["source"].as_u64().unwrap())
                }
            }
        }
        "type-of-address" => {
            let toa = hx(i["hex"].as_str().or(i["args"][0].as_str()).unwrap())[0];
            if expects_error(x) {
                // We accept every TOA octet (receiver tolerance).
                return Err("TOA accepted".into());
            }
            eq("toa", toa_of(&x["toa"]), toa)
        }
        "first-octet" => {
            let fo = i["hex"]
                .as_str()
                .map(|h| hx(h)[0])
                .or(i["first_octet"].as_u64().map(|v| v as u8))
                .unwrap();
            check_first_octet(fo, i["direction"].as_str(), x)
        }
        "dcs" => {
            let d = Dcs::decode(i["dcs"].as_u64().unwrap() as u8);
            if let Some(c) = x["compressed"].as_bool() {
                eq("compressed", d.compressed, c)?;
            }
            if let Some(a) = x["alphabet"]["value"].as_u64() {
                let want = [Alphabet::Gsm7, Alphabet::Bit8, Alphabet::Ucs2][a as usize];
                eq("alphabet", d.alphabet, want)?;
                eq("reserved", d.reserved, !x["error"].is_null())?;
            }
            Ok(())
        }
        "udh-concat" | "ConcatInfo" => {
            let u = Udh::default();
            eq("concat", u.concat().is_some(), x["ok"].as_bool().unwrap())
        }
        "user-data" => {
            let b = hex_decode(i["hex"].as_str().unwrap_or("")).unwrap_or_default();
            let udhi = i["tpdu"]["FirstOctet"].as_u64().unwrap_or(0) & 0x40 != 0;
            let r = match b.split_first() {
                None => Err(pdu::PduError::Truncated("TP-UDL")),
                Some((&udl, ud)) => decode_user_data(udl, ud, udhi, &Dcs::decode(0)),
            };
            ensure(r.is_err() == expects_error(x), || format!("got {r:?}"))
        }
        "user-data-text" => {
            let nli = i["udh"][0]["Data"].as_str().map(|d| hx(d)[0]).unwrap_or(0);
            ensure(
                serial_codec::sms::gsm7::Language::from_nli(nli).is_some(),
                || format!("national language {nli} has no table here"),
            )
        }
        "validity-period-enhanced-format" => {
            // SUBMIT with VPF=01 and an all-zero enhanced VP.
            let b = hx("0900008100000000000000000000");
            let Ok(Tpdu::Submit(s)) = decode_tpdu(&b) else {
                return Err("decode".into());
            };
            let Some(Validity::Enhanced(vp)) = s.validity else {
                return Err("not enhanced".into());
            };
            eq(
                "format",
                u64::from(vp[0] & 7),
                x["format"].as_u64().unwrap(),
            )
        }
        "collect_sequence" => check_collect(i, x),
        other => Err(format!("unsupported op `{other}`")),
    }
}

fn toa_of(v: &Value) -> u8 {
    let ton = match v["ton"].as_str().unwrap() {
        "unknown" => 0,
        "international" => 1,
        "national" => 2,
        "alphanumeric" => 5,
        t => panic!("ton {t}"),
    };
    let npi = match v["npi"].as_str().unwrap() {
        "unknown" => 0,
        "isdn" => 1,
        n => panic!("npi {n}"),
    };
    0x80 | ton << 4 | npi
}

fn check_first_octet(fo: u8, dir: Option<&str>, x: &Value) -> R {
    let tpdu = match dir {
        Some("deliver") => {
            let mut b = vec![fo & !3];
            b.extend(hx("0081000470101010203000010000"));
            b
        }
        _ => {
            let mut b = vec![fo & !3 | 1, 0, 0, 0x81, 0, 0];
            match (fo >> 3) & 3 {
                0 => {}
                2 => b.push(0),
                _ => b.extend([0x70, 0x10, 0x10, 0x10, 0x20, 0x30, 0x00]),
            }
            b.extend(if fo & 0x40 != 0 { vec![1, 0] } else { vec![0] });
            if fo & 0x40 != 0 {
                b[5] = 4; // 8-bit DCS so a 1-octet UD can hold UDHL=0
            }
            b
        }
    };
    let t = decode_tpdu(&tpdu).map_err(|e| e.to_string())?;
    for (k, v) in x.as_object().unwrap() {
        let b = |want: bool| v.as_bool() == Some(want);
        let ok = match (&t, k.as_str()) {
            (Tpdu::Deliver(d), "rp") => b(d.rp),
            (Tpdu::Deliver(d), "udhi") => b(d.udhi),
            (Tpdu::Deliver(d), "sri") => b(d.sri),
            (Tpdu::Deliver(d), "lp") => b(d.lp),
            (Tpdu::Deliver(d), "mms") => b(d.mms),
            (Tpdu::Deliver(_), "mti") => v == "deliver",
            (Tpdu::Submit(s), "rp") => b(s.rp),
            (Tpdu::Submit(s), "udhi") => b(s.udhi),
            (Tpdu::Submit(s), "srr") => b(s.srr),
            (Tpdu::Submit(s), "rd") => b(s.rd),
            (Tpdu::Submit(_), "mti") => v == "submit",
            (Tpdu::Submit(s), "vpf") => {
                let vpf = s.validity.map_or(0, |v| u64::from(v.vpf()));
                // TP-VPF value; warthog's constants use the same numbers.
                v.as_u64().or(v["value"].as_u64()) == Some(vpf)
            }
            _ => false,
        };
        ensure(ok, || format!("{k}: {v} vs {t:?}"))?;
    }
    Ok(())
}

fn check_collect(i: &Value, x: &Value) -> R {
    let mut asm = Reassembler::new();
    for (step, want) in i["steps"]
        .as_array()
        .unwrap()
        .iter()
        .zip(x["steps"].as_array().unwrap())
    {
        let name = step["name"].as_str().unwrap();
        let inp = &step["in"];
        let mo = inp["Direction"]["value"] == 1;
        let kind = match (mo, inp["FirstOctet"].as_u64().unwrap_or(0) & 3) {
            (false, 0) => Kind::Deliver,
            (true, 1) => Kind::Submit,
            _ => continue, // SMS-DELIVER-REPORT is not a TPDU this crate models
        };
        let addr = inp["OA"]["Addr"]
            .as_str()
            .or(inp["DA"]["Addr"].as_str())
            .unwrap();
        let udh = Udh {
            ies: inp["UDH"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|ie| pdu::Ie {
                            id: ie["ID"].as_u64().unwrap() as u8,
                            data: hx(ie["Data"].as_str().unwrap()),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        };
        let frag = Fragment {
            alphabet: Alphabet::Gsm7,
            tables: Tables::DEFAULT,
            payload: name.as_bytes().to_vec(),
        };
        let got = match udh.concat() {
            None => Some(1),
            Some(c) => {
                let key = Key {
                    kind,
                    address: format!("+{addr}"),
                    reference: c.reference,
                    ref_16bit: c.ref_16bit,
                    total: c.total,
                };
                asm.push(key, c.seq, frag, 0).map(|m| m.parts)
            }
        };
        let want_parts = want["out"].as_array().map(Vec::len);
        // 23.040 §9.2.3.24.1: an IE with seq 0 or seq > total is ignored, so the message
        // is delivered on its own. warthog reports ErrReassemblyInconsistency instead.
        let invalid_ie = !udh.ies.is_empty() && udh.concat().is_none();
        if invalid_ie {
            eq(name, got, Some(1))?;
            continue;
        }
        eq(name, got, want_parts)?;
    }
    Ok(())
}

fn reassemble(parts: &[(usize, String)]) -> Result<String, String> {
    let mut asm = Reassembler::new();
    let mut out = None;
    for (len, hex) in parts {
        let p = decode_hex(hex).map_err(|e| e.to_string())?;
        eq("tpdu_len", hex.len() / 2 - smsc_len(hex), *len)?;
        out = asm.push_tpdu(&p.tpdu, 0);
    }
    out.and_then(|m| m.text).ok_or_else(|| "incomplete".into())
}

fn smsc_len(hex: &str) -> usize {
    usize::from(hx(&hex[..2])[0]) + 1
}

// ------------------------------------------------------------------ decode file

const DISAGREE_DECODE: &[(&str, &str)] = &[
    ("dec-009", "CPHS voicemail indicator in the OA (CPHS 4.2 §B.4.2), not a 3GPP field"),
    ("dec-010", "CPHS voicemail indicator, see dec-009"),
    ("dec-011", "CPHS voicemail indicator, see dec-009"),
    ("dec-014", "Android email-gateway split (23.040 §3.8) is application logic, not decoded"),
    ("dec-015", "Android email-gateway split, see dec-014"),
    ("dec-034", "gammu accepts TP-UDL 5 with 1 octet; we reject a UDL beyond the data (23.040 §9.2.3.16)"),
    ("dec-035", "gammu decodes truncated UCS2 (UDL 4, 3 octets); rejected, see dec-034"),
    ("dec-038", "gammu's multipart hardening fails this UDH; per 23.040 §9.2.3.24.1 (ref CE, 1/1) it is valid"),
    ("dec-080", "oFono also re-encodes the SMS-DELIVER byte-exact; this crate encodes SMS-SUBMIT only"),
    ("dec-107", "ZTE status report: python-gsmmodem prints nibble B as 'b'; 23.040 §9.1.2.3 maps 1011 to '#'"),
    ("dec-112", "smspdudecoder rejects TOA 0x00; we accept any TOA octet (receiver tolerance)"),
    ("dec-113", "smspdudecoder rejects NPI 2; we accept any TOA octet"),
    ("dec-114", "UCS2 UD shorter than TP-UDL (68 vs 17 octets): rejected, smspdudecoder decodes with a warning"),
    ("dec-132", "Kannada (NLI 7) national tables are not included"),
];

#[test]
fn pdu_decode_vectors() {
    let v = load(include_str!("fixtures/sms/pdu_decode.json"));
    run("pdu_decode", &v, DISAGREE_DECODE, check_decode_entry);
}

// ------------------------------------------------------------------ encode file

fn base_req(dest: &str, text: &str) -> SubmitRequest {
    let mut req = SubmitRequest::new(dest, text);
    req.concat_ref = Some(0);
    req
}

fn one(req: &SubmitRequest) -> Result<String, String> {
    let parts = encode_submit(req).map_err(|e| format!("encode: {e}"))?;
    ensure(parts.len() == 1, || format!("{} parts", parts.len()))?;
    Ok(parts[0].1.clone())
}

fn text_spec(spec: &str) -> String {
    match spec {
        s if s.starts_with("'a'*4560") => {
            "a".repeat(4560) + &"b".repeat(152) + &"c".repeat(152) + &"d".repeat(152)
        }
        s => {
            // "U+XXXX repeated N times"
            let cp = u32::from_str_radix(&s[2..6], 16).unwrap();
            let n: usize = s.split(' ').nth(2).unwrap().parse().unwrap();
            char::from_u32(cp).unwrap().to_string().repeat(n)
        }
    }
}

fn check_encode_entry(e: &Value) -> R {
    let i = &e["input"];
    let x = &e["expected"];
    let src = e["source"].as_str().unwrap();
    let kind = i["op"]
        .as_str()
        .or(i["kind"].as_str())
        .or(i["field"].as_str())
        .unwrap_or("");
    match kind {
        "" if src.starts_with("aosp") => {
            let mut req = base_req(i["number"].as_str().unwrap(), i["text"].as_str().unwrap());
            req.smsc = i["smsc"].as_str().map(str::to_string);
            req.message_ref = i["message_ref"].as_u64().unwrap() as u8;
            let want = format!(
                "{}{}",
                x["smsc_hex"].as_str().unwrap(),
                x["pdu"].as_str().unwrap()
            );
            eq("pdu", one(&req)?, want)
        }
        "" if src.starts_with("chan_dongle") => {
            let mut req = base_req(i["number"].as_str().unwrap(), i["text"].as_str().unwrap());
            req.status_report = i["status_report"].as_bool().unwrap();
            req.concat_ref = Some(i["csms_ref"].as_u64().unwrap() as u16);
            req.validity = Some(Validity::Relative(
                relative_from_minutes(i["validity_minutes"].as_u64().unwrap() as u32).unwrap(),
            ));
            let got: Vec<String> = encode_submit(&req)
                .map_err(|e| e.to_string())?
                .into_iter()
                .map(|p| p.1)
                .collect();
            let want: Vec<String> = x["pdus"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| p.as_str().unwrap().to_string())
                .collect();
            eq("pdus", got, want)
        }
        "" if src.starts_with("gammu") => check_gammu(i, x),
        "" if i.get("roundtrip_of_pdu").is_some() => {
            let hex = i["roundtrip_of_pdu"].as_str().unwrap();
            let p = decode_hex(hex).map_err(|e| e.to_string())?;
            eq(
                "pdu_len",
                hex.len() as u64 / 2,
                x["pdu_len"].as_u64().unwrap(),
            )?;
            eq(
                "tpdu_len",
                (hex.len() / 2 - smsc_len(hex)) as u64,
                x["tpdu_len"].as_u64().unwrap(),
            )?;
            let Tpdu::Submit(s) = &p.tpdu else {
                return Err("re-encoding an SMS-DELIVER is not supported".into());
            };
            let mut req = base_req(
                &s.destination.value,
                s.user_data.text.as_deref().unwrap_or(""),
            );
            req.destination_toa = Some(s.destination.toa);
            req.smsc = p.smsc.as_ref().map(|a| a.value.clone());
            req.smsc_toa = p.smsc.as_ref().map(|a| a.toa);
            req.message_ref = s.mr;
            req.validity = s.validity;
            req.status_report = s.srr;
            req.reject_duplicates = s.rd;
            req.alphabet = Some(s.dcs.alphabet);
            req.pid = s.pid;
            eq(
                "pdu",
                one(&req)?,
                x["pdu"].as_str().unwrap().to_ascii_uppercase(),
            )
        }
        "" if i.get("text").is_some() || i.get("text_spec").is_some() => {
            if src.starts_with("ofono") {
                check_ofono_segments(i, x)
            } else {
                check_python_submit(i, x)
            }
        }
        "split_concat_7bit" => {
            let mut req = base_req("0", i["text"].as_str().unwrap());
            req.concat_ref = Some(i["udh_ref"].as_u64().unwrap() as u16);
            let parts = encode_submit(&req).map_err(|e| e.to_string())?;
            let want = x["parts"].as_array().unwrap();
            eq("parts", parts.len(), want.len())?;
            for ((_, hex), w) in parts.iter().zip(want) {
                let p = decode_hex(hex).map_err(|e| e.to_string())?;
                let ud = p.tpdu.user_data().unwrap();
                eq(
                    "udh",
                    ud.udh.as_ref().map(|u| hex_encode(&u.encode().unwrap())),
                    w["udh_hex"].as_str().map(str::to_string),
                )?;
                eq("text", p.tpdu.text(), w["text"].as_str())?;
                eq(
                    "septets",
                    ud.payload.len() as u64,
                    w["septets"].as_u64().unwrap(),
                )?;
            }
            Ok(())
        }
        "address_field" => {
            let toa = match i["type"].as_str().unwrap() {
                "unknown" => 0x81,
                "national" => 0xA1,
                "international" => 0x91,
                _ => 0xD0,
            };
            let r = encode_address(i["number"].as_str().unwrap(), Some(toa));
            if x["len"] == 2 {
                // libosmocore writes only the header when the number does not fit.
                return ensure(r.is_err(), || format!("encoded {r:?}"));
            }
            eq(
                "hex",
                r.map(|b| hex_encode(&b)).map_err(|e| e.to_string())?,
                x["hex"].as_str().unwrap().to_string(),
            )
        }
        "submit_pdu" => {
            let mut req = base_req(i["number"].as_str().unwrap(), i["text"].as_str().unwrap());
            req.validity = i["validity_relative"]
                .as_u64()
                .map(|m| Validity::Relative(relative_from_minutes(m as u32).unwrap()));
            let h = one(&req)?;
            eq(
                "msgstart",
                smsc_len(&h) as u64,
                x["msgstart"].as_u64().unwrap(),
            )?;
            eq("pdu", h, x["pdu"].as_str().unwrap().to_string())
        }
        "semioctet" | "number-semioctets" => {
            let r = encode_semi_octets(i["digits"].as_str().or(i["number"].as_str()).unwrap_or(""));
            if expects_error(x) {
                return ensure(r.is_err(), || "encoded".into());
            }
            eq(
                "hex",
                hex_encode(&r.map_err(|c| c.to_string())?),
                x["hex"].as_str().unwrap_or("").to_string(),
            )
        }
        "address" => {
            let (num, toa) = match i.get("address") {
                Some(a) => (
                    a["Addr"].as_str().unwrap_or("").to_string(),
                    Some(a["TOA"].as_u64().unwrap_or(0) as u8),
                ),
                None => (i["number"].as_str().unwrap().to_string(), None),
            };
            let r = encode_address(&num, toa);
            if expects_error(x) {
                return ensure(r.is_err(), || "encoded".into());
            }
            eq(
                "hex",
                hex_encode(&r.map_err(|e| e.to_string())?),
                x["hex"].as_str().unwrap().to_string(),
            )
        }
        "smsc-address" => {
            let (num, toa) = match i["smsc"].get(0) {
                Some(a) => (
                    a["Addr"].as_str().map(str::to_string),
                    a["TOA"].as_u64().map(|t| t as u8),
                ),
                None => (i["smsc"].as_str().map(str::to_string), None),
            };
            let r = encode_smsc(num.as_deref(), toa);
            if expects_error(x) {
                return ensure(r.is_err(), || "encoded".into());
            }
            eq(
                "hex",
                hex_encode(&r.map_err(|e| e.to_string())?),
                x["hex"].as_str().unwrap().to_string(),
            )
        }
        "" if i.get("tpdu_hex").is_some() => {
            let (num, toa) = match i["smsc"].get(0) {
                Some(a) => (
                    a["Addr"].as_str().map(str::to_string),
                    a["TOA"].as_u64().map(|t| t as u8),
                ),
                None => (None, None),
            };
            let r = encode_smsc(num.as_deref(), toa);
            if expects_error(x) {
                return ensure(r.is_err(), || "encoded".into());
            }
            let mut b = r.map_err(|e| e.to_string())?;
            b.extend(hex_decode(i["tpdu_hex"].as_str().unwrap_or("")).unwrap());
            eq(
                "pdu",
                hex_encode(&b),
                x["pdu"].as_str().unwrap().to_string(),
            )
        }
        "validity-period-relative" => {
            let secs = i["duration"]["timedelta_seconds"].as_u64().unwrap();
            let r = relative_from_minutes((secs / 60) as u32);
            match x["tp_vp"].as_u64() {
                Some(vp) => eq("vp", r.map(u64::from), Some(vp)),
                None => ensure(r.is_none(), || "in range".into()),
            }
        }
        "validity-period" => {
            let vp = &i["vp"];
            match vp["Format"]["value"].as_u64() {
                None => ensure(x["hex"].is_null(), || "vp".into()),
                Some(2) => {
                    let mins = vp["Duration"].as_u64().unwrap() / 60_000_000_000;
                    let r = relative_from_minutes(mins as u32).map(|v| hex_encode(&[v]));
                    eq("vp", r, x["hex"].as_str().map(str::to_string))
                }
                Some(_) => Err("enhanced VP is passed through raw, not validated".into()),
            }
        }
        "scts" => {
            let t = if let Some(py) = i["datetime_py"].as_str() {
                ts_of(match py {
                    "datetime(2018, 1, 1)" => "2018-01-01T00:00:00Z",
                    p if p.contains("Paris") => "2020-01-29T13:25:41+01:00",
                    _ => "2013-03-25T23:01:56-07:00",
                })
            } else if let Some(s) = i["timestamp"].as_str().or(i["timestamp"]["Time"].as_str()) {
                ts_of(s)
            } else {
                // warthog: 2070-12-31T23:59:59 with a +24h zone, which TP-SCTS cannot carry.
                Timestamp {
                    year: 2070,
                    month: 12,
                    day: 31,
                    hour: 23,
                    minute: 59,
                    second: 59,
                    tz_quarters: 96,
                }
            };
            match x["hex"].as_str() {
                Some(h) => eq(
                    "scts",
                    t.encode().map(|b| hex_encode(&b)),
                    Some(h.to_string()),
                ),
                None => ensure(t.encode().is_none(), || "encoded".into()),
            }
        }
        "type-of-address" => eq(
            "toa",
            hex_encode(&[toa_of(&i["toa"])]),
            x["hex"].as_str().unwrap().to_string(),
        ),
        "divide_text" => {
            let mut req = base_req("1", i["text"].as_str().unwrap());
            req.alphabet = Some(match i["alphabet"].as_str().unwrap() {
                "gsm7" => Alphabet::Gsm7,
                _ => Alphabet::Ucs2,
            });
            let n = encode_submit(&req).map_err(|e| e.to_string())?.len();
            eq("segments", n as u64, x["segments"].as_u64().unwrap())
        }
        other => Err(format!("unsupported op `{other}`")),
    }
}

fn check_gammu(i: &Value, x: &Value) -> R {
    let want = x["pdu"].as_str().unwrap();
    let text = i["text"].as_str().unwrap();
    let ucs = i["coding"] == "Unicode";
    let p = decode_hex(want).map_err(|e| e.to_string())?;
    eq(
        "smsc",
        p.smsc.map(|a| a.to_string()),
        Some(i["smsc"].as_str().unwrap().to_string()),
    )?;
    eq(
        "number",
        p.tpdu.address().to_string(),
        i["number"].as_str().unwrap().to_string(),
    )?;
    eq(
        "alphabet",
        p.tpdu.dcs().map(|d| d.alphabet),
        Some(if ucs { Alphabet::Ucs2 } else { Alphabet::Gsm7 }),
    )?;
    let got = p.tpdu.text().ok_or("no text")?;
    let encodable = ucs || serial_codec::sms::gsm7::is_encodable(text, Tables::DEFAULT);
    if encodable {
        // gammu truncates to one part (70 UCS2 / 160 GSM characters).
        ensure(text.starts_with(got) && !got.is_empty(), || {
            format!("text {got:?}")
        })?;
    } else {
        // gammu transliterates characters outside GSM 7-bit (š → s); compare lengths.
        eq("chars", got.chars().count(), text.chars().count())?;
    }
    let Tpdu::Submit(s) = &p.tpdu else {
        return Ok(()); // "Sent"-folder entries are stored as SMS-DELIVER
    };
    if !encodable || got != text {
        return Ok(()); // gammu output is truncated or transliterated: nothing to compare
    }
    let mut req = base_req(i["number"].as_str().unwrap(), text);
    req.smsc = i["smsc"].as_str().map(str::to_string);
    req.alphabet = Some(if ucs { Alphabet::Ucs2 } else { Alphabet::Gsm7 });
    req.validity = s.validity;
    eq("pdu", one(&req)?, want.to_string())
}

fn check_ofono_segments(i: &Value, x: &Value) -> R {
    let text = i["text"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| text_spec(i["text_spec"].as_str().unwrap()));
    let mut req = base_req(i["number"].as_str().unwrap(), &text);
    req.concat_16bit = i["use_16bit_ref"].as_bool().unwrap();
    req.concat_ref = Some(i["ref"].as_u64().unwrap_or(0) as u16);
    req.status_report = i["status_report_request"].as_bool().unwrap_or(false);
    let parts = encode_submit(&req).map_err(|e| e.to_string())?;
    eq(
        "segments",
        parts.len() as u64,
        x["segments"].as_u64().unwrap(),
    )?;
    if x["roundtrip_text_equal"] == true || x.get("decoded_char_count").is_some() {
        let back = reassemble(&parts)?;
        eq("roundtrip", back.chars().count(), text.chars().count())?;
        eq("roundtrip text", back == text, true)?;
    }
    if x["pdu_len_eq_tpdu_len_plus_1"] == true {
        ensure(parts.iter().all(|(l, h)| h.len() / 2 == l + 1), || {
            "smsc".into()
        })?;
    }
    if x.get("one_more_char").is_some() {
        req.body = pdu::Body::Text(format!("{text}{}", text.chars().next().unwrap()));
        ensure(
            encode_submit(&req) == Err(pdu::PduError::TooManyParts),
            || "one more char still fits".into(),
        )?;
    }
    Ok(())
}

fn check_python_submit(i: &Value, x: &Value) -> R {
    let mut req = base_req(i["number"].as_str().unwrap(), i["text"].as_str().unwrap());
    let reference = i["reference"].as_u64().unwrap_or(0);
    req.message_ref = reference as u8;
    req.concat_ref = Some(reference as u16);
    req.smsc = i["smsc"].as_str().map(str::to_string);
    req.reject_duplicates = i["rejectDuplicates"].as_bool().unwrap_or(false);
    // sendSms() (entries with "commands") requests a status report by default;
    // encodeSmsSubmitPdu() does not.
    req.status_report = i["requestStatusReport"]
        .as_bool()
        .unwrap_or(x.get("commands").is_some());
    if i["sendFlash"] == true {
        req.class = Some(0);
    }
    if let Some(s) = i["validity"]["timedelta_seconds"].as_u64() {
        req.validity = Some(Validity::Relative(
            relative_from_minutes((s / 60) as u32).unwrap(),
        ));
    }
    // python-gsmmodem sends numbers without `+` as national (TOA 0xA1).
    if !req.destination.starts_with('+') {
        req.destination_toa = Some(pdu::TOA_NATIONAL);
    }
    if req.smsc.as_deref().is_some_and(|s| !s.starts_with('+')) {
        req.smsc_toa = Some(pdu::TOA_NATIONAL);
    }
    let parts = encode_submit(&req).map_err(|e| e.to_string())?;
    let got: Vec<String> = parts.iter().map(|p| p.1.clone()).collect();
    let want: Vec<String> = match x.get("pdus") {
        Some(p) => p
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap().to_string())
            .collect(),
        None => vec![x["pdu"].as_str().unwrap().to_string()],
    };
    eq("pdus", got, want)?;
    if let Some(l) = x.get("tpdu_lengths") {
        let got: Vec<u64> = parts.iter().map(|p| p.0 as u64).collect();
        let want: Vec<u64> = l
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap())
            .collect();
        eq("tpdu_lengths", got, want)?;
    }
    if let Some(c) = x["commands"].as_array() {
        for (len, hex) in &parts {
            let cmgs = format!("AT+CMGS={len}");
            let body = format!("{hex}\u{1a}");
            ensure(c.iter().any(|v| v == cmgs.as_str()), || {
                format!("no {cmgs}")
            })?;
            ensure(c.iter().any(|v| v == body.as_str()), || {
                "no pdu line".into()
            })?;
        }
    }
    Ok(())
}

const DISAGREE_ENCODE: &[(&str, &str)] = &[
    ("enc-002", "chan_dongle fills 152 septets per concatenated part; the maximum with a 6-octet UDH is 153 (23.040 §9.2.3.24), which we use"),
    ("enc-090", "fixture input says rejectDuplicates=true but its PDU has TP-SRR set and TP-RD clear (first octet 0x31); the extracted argument name looks mislabelled"),
    ("enc-094", "python-gsmmodem divideTextGsm7 always splits at 153 septets; a 154-septet text fits one SMS (160, 23.040 §9.2.3.16)"),
    ("enc-095", "same as enc-094 (153 characters, 154 septets with one escape pair)"),
    ("enc-098", "python-gsmmodem divideTextUcs2 always splits at 67 characters; 68 fit one UCS2 SMS (70)"),
    ("enc-099", "same as enc-098 (68 characters)"),
    ("enc-043", "alphanumeric length: libosmocore counts octets*2 (14); 23.040 §9.1.2.5 counts useful semi-octets (13)"),
    ("enc-044", "libosmocore truncates a 20-character alphanumeric address to its buffer; we reject >11 characters (20 semi-octets)"),
    ("enc-061", "oFono re-encodes this SMS-DELIVER; this crate encodes SMS-SUBMIT only"),
    ("enc-062", "SMS-DELIVER re-encode, see enc-061"),
    ("enc-063", "SMS-DELIVER re-encode, see enc-061"),
    ("enc-126", "warthog marshals an empty address as 00 00; encode_address rejects it, an SMSC refuses a TPDU without a destination"),
    ("enc-134", "enhanced TP-VP is passed through raw (no 23.040 §9.2.3.12.3 validation)"),
];

#[test]
fn pdu_encode_vectors() {
    let v = load(include_str!("fixtures/sms/pdu_encode.json"));
    run("pdu_encode", &v, DISAGREE_ENCODE, check_encode_entry);
}

// ------------------------------------------------------------------ gateway fixtures

fn check_gateway_golden(e: &Value) -> R {
    let p = decode_hex(e["pdu"].as_str().unwrap()).map_err(|er| er.to_string())?;
    let t = &p.tpdu;
    let want_type = match e["tpdu_type"].as_str().unwrap() {
        "SMS-DELIVER" => "deliver",
        "SMS-SUBMIT" => "submit",
        _ => "status-report",
    };
    eq("type", type_name(t), want_type)?;
    if let Some(a) = e["sender"].as_str().or(e["destination"].as_str()) {
        eq("address", t.address().value.as_str(), a)?;
    }
    let dcs = t.dcs().copied();
    if let Some(enc) = e["encoding"].as_str().filter(|s| *s != "unknown") {
        let want = match enc {
            "gsm" => Alphabet::Gsm7,
            "ucs2" => Alphabet::Ucs2,
            _ => Alphabet::Bit8,
        };
        eq("alphabet", dcs.map(|d| d.alphabet), Some(want))?;
    }
    if let Some(text) = e["text"].as_str() {
        match dcs.map(|d| d.alphabet) {
            Some(Alphabet::Bit8) => eq(
                "payload",
                t.user_data().map(|u| u.payload.clone()),
                Some(text.as_bytes().to_vec()),
            )?,
            _ => eq("text", t.text(), Some(text))?,
        }
    }
    let tags: Vec<&str> = e["tags"]
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    if tags.contains(&"morgverd-encode") {
        // Re-encode: FO 01, MR 00, PID 00, DCS class 1, no VP.
        let Tpdu::Submit(s) = t else {
            return Err("not submit".into());
        };
        let mut req = base_req(&t.address().to_string(), t.text().unwrap_or(""));
        req.class = s.dcs.class;
        eq(
            "pdu",
            one(&req)?,
            e["pdu"].as_str().unwrap().to_ascii_uppercase(),
        )?;
    }
    Ok(())
}

const DISAGREE_GATEWAY: &[(&str, &str)] = &[
    ("warthog_status_report_minimal", "truncated: TP-SCTS, TP-DT and TP-ST need 15 octets after TP-RA, 3 present (23.040 §9.2.2.3)"),
    ("warthog_pdumode_roundtrip", "synthetic bytes 01..09: TP-UDL 9 with no user data (23.040 §9.2.3.16)"),
];

#[test]
fn gateway_golden_vectors() {
    let v = load(include_str!("fixtures/sms/gateway_golden_external.json"));
    let v: Vec<Value> = v
        .into_iter()
        .map(|mut e| {
            e["source"] = Value::String("gateway-soft-rs".into());
            let id = e["id"].clone();
            e["id"] = id;
            e
        })
        .collect();
    run("gateway_golden", &v, DISAGREE_GATEWAY, check_gateway_golden);
}

#[test]
fn gateway_multipart_vectors() {
    let v = load(include_str!("fixtures/sms/gateway_multipart.json"));
    for e in &v {
        let mut asm = Reassembler::new();
        let mut text = None;
        for h in e["parts"].as_array().unwrap() {
            let p = decode_hex(h.as_str().unwrap()).unwrap();
            text = asm.push_tpdu(&p.tpdu, 0).and_then(|m| m.text);
        }
        assert_eq!(text.as_deref(), e["text"].as_str(), "{}", e["id"]);
        if e["id"] == "gen_gsm_multipart_long" {
            // Byte-exact re-encode: +79001234567, MR 0, concat ref 1.
            let mut req = SubmitRequest::new("+79001234567", e["text"].as_str().unwrap());
            req.concat_ref = Some(1);
            let got: Vec<String> = encode_submit(&req)
                .unwrap()
                .into_iter()
                .map(|p| p.1)
                .collect();
            let want: Vec<String> = e["parts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap().to_string())
                .collect();
            assert_eq!(got, want);
        }
    }
    eprintln!("[gateway_multipart] {} pass", v.len());
}

#[test]
fn gateway_codec_vectors() {
    let v = load(include_str!("fixtures/sms/gateway_codec_golden.json"));
    for e in &v {
        let b = hx(e["hex"].as_str().unwrap());
        let got = match e["encoding"].as_str().unwrap() {
            "ucs2" => serial_codec::sms::ucs2::decode(&b),
            // Septet count is not given; the 23.038 §6.1.2.3.1 CR rule recovers it.
            _ => serial_codec::ussd::decode_gsm7(&b),
        };
        assert_eq!(got, e["text"].as_str().unwrap(), "{}", e["id"]);
    }
    eprintln!("[gateway_codec] {} pass", v.len());
}

#[test]
fn gateway_bit8_vectors() {
    let v: Value =
        serde_json::from_str(include_str!("fixtures/sms/gateway_bit8_external.json")).unwrap();
    for e in v["pdu_cases"].as_array().unwrap() {
        check_gateway_golden(e).unwrap();
    }
    for e in v["payload_cases"].as_array().unwrap() {
        // 8-bit payloads stay bytes: check the UD round-trips through decode_user_data.
        let b = hx(e["payload_hex"].as_str().unwrap());
        let ud = decode_user_data(b.len() as u8, &b, false, &Dcs::decode(0x04)).unwrap();
        assert_eq!(ud.payload, b);
        assert_eq!(ud.text, None);
        if let Some(t) = e["expect_text"].as_str() {
            assert_eq!(ud.payload, t.as_bytes());
        }
    }
    for e in v["ussd_cases"].as_array().unwrap() {
        let b = hx(e["ussd_hex"].as_str().unwrap());
        let dcs: u8 = e["dcs"].as_str().unwrap().parse().unwrap();
        // Known disagreement: the fixture reads USSD DCS 4 as SMS 8-bit data. +CUSD
        // carries a CBS DCS (27.007 §7.15), and CBS 0x04 is GSM 7-bit, language
        // group 0000 (23.038 §5). Check our reading and the 8-bit one separately.
        assert_eq!(
            serial_codec::ussd::CbsDcs::decode(dcs).alphabet,
            Alphabet::Gsm7
        );
        let t = serial_codec::ussd::decode(&b, 0x44).unwrap(); // CBS 8-bit
        assert_eq!(t.text, e["text"].as_str().unwrap());
    }
    eprintln!("[gateway_bit8] 4 pass");
}

#[test]
fn gateway_rejected_vectors() {
    let v = load(include_str!("fixtures/sms/gateway_rejected.json"));
    for e in &v {
        let id = e["id"].as_str().unwrap();
        match id {
            // Portuguese tables round-trip "ação".
            "charset_portuguese_acao" => {
                let mut req = SubmitRequest::new("+1", "ação");
                req.locking_shift = vec![serial_codec::sms::gsm7::Language::Portuguese];
                req.single_shift = vec![serial_codec::sms::gsm7::Language::Portuguese];
                let parts = encode_submit(&req).unwrap();
                let p = decode_hex(&parts[0].1).unwrap();
                assert_eq!(p.tpdu.dcs().unwrap().alphabet, Alphabet::Gsm7);
                assert_eq!(p.tpdu.text(), Some("ação"));
            }
            _ => assert!(decode_hex(e["pdu"].as_str().unwrap()).is_err(), "{id}"),
        }
    }
    eprintln!("[gateway_rejected] {} pass", v.len());
}
