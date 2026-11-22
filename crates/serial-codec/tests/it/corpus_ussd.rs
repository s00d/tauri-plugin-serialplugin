//! Corpus: USSD vectors (data only; sources and licenses in `tests/fixtures/ussd/README.md`).
//! Entries in `DISAGREE` must fail their check.

use serde_json::Value;
use serial_codec::sms::{hex_decode, hex_encode};
use serial_codec::ussd::{self, parse_cusd, Cscs, CusdStatus};
use std::collections::BTreeMap;

type R = Result<(), String>;

fn eq<T: PartialEq + std::fmt::Debug>(what: &str, got: T, want: T) -> R {
    if got == want {
        Ok(())
    } else {
        Err(format!("{what}: got {got:?}, want {want:?}"))
    }
}

fn check(e: &Value) -> R {
    let i = &e["input"];
    let x = &e["expected"];
    let src = e["source"].as_str().unwrap();
    match i["op"].as_str().unwrap_or("") {
        "pack_7bit_ussd" => {
            let text = i["text"].as_str().unwrap();
            let packed = ussd::encode(text).map_err(|c| c.to_string())?;
            eq(
                "hex",
                hex_encode(&packed),
                x["hex"].as_str().unwrap().to_string(),
            )?;
            eq(
                "decoded",
                ussd::decode_gsm7(&packed),
                x["decoded"].as_str().unwrap().to_string(),
            )
        }
        "pack7_ussd" => {
            let septets = hex_decode(i["septets"].as_str().unwrap_or("")).unwrap();
            let pad = i["pad_bits"].as_u64().unwrap() as u8;
            let want = x["packed"].as_str().unwrap_or("");
            eq(
                "pack",
                hex_encode(&ussd::pack_septets(&septets, pad)),
                want.to_string(),
            )?;
            eq(
                "unpack",
                ussd::unpack_septets(&hex_decode(want).unwrap(), pad),
                septets,
            )
        }
        "reply" => check_lines(i, x),
        _ if i.get("lines").is_some() => check_lines(i, x),
        _ if src.starts_with("chan_dongle") => {
            let c = parse_cusd(i["line"].as_str().unwrap()).ok_or("not parsed")?;
            eq(
                "type",
                c.status,
                CusdStatus::from_code(x["type"].as_u64().unwrap() as u8),
            )?;
            eq(
                "str",
                c.raw.unwrap_or_default(),
                x["cusd"].as_str().unwrap().to_string(),
            )?;
            eq(
                "dcs",
                c.dcs.map_or(-1, i64::from),
                x["dcs"].as_i64().unwrap(),
            )
        }
        _ if src.starts_with("gammu") => {
            let c = parse_cusd(i["line"].as_str().unwrap()).ok_or("not parsed")?;
            // `at_charset` is the +CSCS gammu ran with; Huawei firmware sends packed
            // septets in hex (non-standard), which needs the explicit opt-in.
            let t = if i["at_charset"] == "UCS2" {
                c.text_as(Cscs::Ucs2)
            } else if e["vendor"] == "Huawei" {
                c.text_packed_hex()
            } else {
                c.text()
            }
            .ok_or("no text")?;
            let raw = c.raw.as_deref().unwrap_or("");
            let hex = raw.len() >= 2 && raw.bytes().all(|b| b.is_ascii_hexdigit());
            if t.is_empty() || (hex && t == raw) {
                return Err(format!("not decoded: {t:?}"));
            }
            Ok(())
        }
        other => Err(format!("unsupported op `{other}`")),
    }
}

/// python-gsmmodem session tests: the `+CUSD` line carrying the expected message parses to
/// that message, and `<m>` gives the session state.
fn check_lines(i: &Value, x: &Value) -> R {
    let want = x["message"].as_str().unwrap();
    let hit = i["lines"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|l| parse_cusd(l.as_str()?))
        .find(|c| c.text().as_deref() == Some(want))
        .ok_or("no +CUSD line yields the message")?;
    if let Some(active) = x["session_active"].as_bool() {
        eq("session", hit.status.session_active(), active)?;
    }
    if let Some(cmd) = x["command"].as_str() {
        // AT+CUSD=1,"<code>",15 is what the library sends; check the code round-trips.
        eq(
            "command",
            cmd,
            format!("AT+CUSD=1,\"{}\",15", i["ussd"].as_str().unwrap()).as_str(),
        )?;
    }
    Ok(())
}

const DISAGREE: &[(&str, &str)] = &[(
    "us-014",
    "libosmocore keeps the CR added after a CR on an octet boundary; we drop it (23.038 §6.1.2.3.1: CR CR ≡ CR). The packed bytes match",
)];

#[test]
fn ussd_vectors() {
    let v: Vec<Value> = serde_json::from_str(include_str!("../fixtures/ussd/ussd.json")).unwrap();
    let mut failures = Vec::new();
    let mut stats: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for e in &v {
        let id = e["id"].as_str().unwrap();
        let src = e["source"]
            .as_str()
            .unwrap()
            .split('/')
            .next()
            .unwrap()
            .to_string();
        let listed = DISAGREE.iter().any(|(d, _)| *d == id);
        let st = stats.entry(src).or_default();
        match (check(e), listed) {
            (Ok(()), false) => st.0 += 1,
            (Err(_), true) => st.1 += 1,
            (Ok(()), true) => failures.push(format!("{id}: listed as disagreement but passes")),
            (Err(m), false) => failures.push(format!("{id}: {m}")),
        }
    }
    for (src, (p, d)) in &stats {
        eprintln!("[ussd] {src}: {p} pass, {d} known disagreement");
    }
    assert!(failures.is_empty(), "ussd:\n{}", failures.join("\n"));
}
