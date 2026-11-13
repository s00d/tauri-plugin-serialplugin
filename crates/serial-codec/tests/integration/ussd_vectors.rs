//! USSD vectors (data only; sources and licenses in `tests/fixtures/ussd/README.md`).
//! Entries in `DISAGREE` must fail their check.

use serde_json::Value;
use serial_codec::sms::{hex_decode, hex_encode};
use serial_codec::ussd::{self, parse_cusd, CbsDcs, Cscs, CusdStatus};
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

#[test]
fn cusd_line_forms() {
    let c = parse_cusd(r#"+cusd: 1,"Menu: 1, 2, \"three\"",15OK"#).unwrap();
    assert_eq!(c.status, CusdStatus::ActionRequired);
    assert_eq!(c.raw.as_deref(), Some(r#"Menu: 1, 2, "three""#));
    assert_eq!(c.dcs, Some(15));
    // Unescaped inner quotes: the closing quote is the last one.
    let c = parse_cusd(r#"+CUSD: 0,"He said "hi", ok",15"#).unwrap();
    assert_eq!(c.raw.as_deref(), Some(r#"He said "hi", ok"#));
    // Unquoted with DCS, and trailing OK after a space.
    let c = parse_cusd("+CUSD: 2,Balance 5,00 EUR,15 OK").unwrap();
    assert_eq!(c.raw.as_deref(), Some("Balance 5,00 EUR"));
    assert_eq!(c.dcs, Some(15));
    assert_eq!(
        parse_cusd("+CUSD: 4").unwrap().status,
        CusdStatus::NotSupported
    );
    assert_eq!(parse_cusd("+CUSD: 5").unwrap().status, CusdStatus::Timeout);
    assert_eq!(
        parse_cusd("+CUSD:3").unwrap().status,
        CusdStatus::OtherClient
    );
    assert!(parse_cusd("+CUSD: x").is_none());
    assert!(parse_cusd("+CMTI: \"SM\",1").is_none());
    // DCS 72 (0x48) = UCS2 hex.
    let c = parse_cusd(r#"+CUSD: 0,"041F0440043804320435044200210020",72"#).unwrap();
    assert_eq!(c.text().as_deref(), Some("Привет! "));
    // Packed GSM 7-bit hex with DCS 15 ("*115*5#" in USSD packing): firmware quirk,
    // decoded only on request.
    let c = parse_cusd(r#"+CUSD: 0,"AA58ACA6AA8D1A",15"#).unwrap();
    assert_eq!(c.text().as_deref(), Some("AA58ACA6AA8D1A"));
    assert_eq!(c.text_packed_hex().as_deref(), Some("*115*5#"));
    // Plain text with a GSM DCS stays as sent.
    let c = parse_cusd(r#"+CUSD: 0,"Totsiens",15"#).unwrap();
    assert_eq!(c.text().as_deref(), Some("Totsiens"));
    // Language-prefixed UCS2 (DCS 0x11): two GSM 7-bit characters, then UCS2.
    let t = ussd::decode(&hex_decode("61390041").unwrap(), 0x11).unwrap();
    assert_eq!((t.language.as_deref(), t.text.as_str()), (Some("ar"), "A"));
    // Language-prefixed GSM 7-bit (DCS 0x10): "en" CR, then the text.
    let t = ussd::decode(&ussd::encode("en\rHi").unwrap(), 0x10).unwrap();
    assert_eq!((t.language.as_deref(), t.text.as_str()), (Some("en"), "Hi"));
}

/// 27.007 §7.15: with a GSM 7-bit DCS `<str>` is in the TE character set; a hex-looking
/// plain reply is not reinterpreted.
#[test]
fn cusd_text_follows_cscs() {
    let t = |line: &str, cs: Cscs| parse_cusd(line).unwrap().text_as(cs);
    for (line, plain) in [
        (r#"+CUSD: 0,"0031",15"#, "0031"),
        (r#"+CUSD: 0,"0031",0"#, "0031"),
        (r#"+CUSD: 0,"1234",15"#, "1234"),
        (r#"+CUSD: 0,"00310032",15"#, "00310032"),
        (r#"+CUSD: 0,"ABCD",15"#, "ABCD"),
        (r#"+CUSD: 0,"12345678",15"#, "12345678"),
        (r#"+CUSD: 0,"4869""#, "4869"),
    ] {
        assert_eq!(
            parse_cusd(line).unwrap().text().as_deref(),
            Some(plain),
            "{line}"
        );
        assert_eq!(t(line, Cscs::Ira).as_deref(), Some(plain), "{line}");
    }
    assert_eq!(
        t(r#"+CUSD: 0,"00310032",15"#, Cscs::Ucs2).as_deref(),
        Some("12")
    );
    assert_eq!(t(r#"+CUSD: 0,"4869",15"#, Cscs::Hex).as_deref(), Some("Hi"));
    assert_eq!(
        t(r#"+CUSD: 0,"48656C6C6F",15"#, Cscs::Hex).as_deref(),
        Some("Hello")
    );
    assert_eq!(t(r#"+CUSD: 0,"Hello",15"#, Cscs::Ucs2), None);
    // UCS2 DCS: hex octets whatever +CSCS is.
    assert_eq!(
        t(r#"+CUSD: 0,"00480069",72"#, Cscs::Gsm).as_deref(),
        Some("Hi")
    );
    // The packed-hex opt-in needs hex.
    assert_eq!(
        parse_cusd(r#"+CUSD: 0,"Hello",15"#)
            .unwrap()
            .text_packed_hex(),
        None
    );
}

#[test]
fn cusd_unclosed_quote_keeps_ok_and_bad_dcs_is_reported() {
    let c = parse_cusd(r#"+CUSD: 0,"1234 OK"#).unwrap();
    assert_eq!(c.raw.as_deref(), Some("1234 OK"));
    // A glued OK after a closed string or the DCS is still dropped.
    let c = parse_cusd(r#"+CUSD: 0,"1234"OK"#).unwrap();
    assert_eq!((c.raw.as_deref(), c.dcs), (Some("1234"), None));
    let c = parse_cusd(r#"+CUSD: 0,"Hi",300"#).unwrap();
    assert_eq!(c.dcs, None);
    assert_eq!(c.invalid_dcs.as_deref(), Some("300"));
    let c = parse_cusd("+CUSD: 0,Hi,300 OK").unwrap();
    assert_eq!(
        (c.raw.as_deref(), c.invalid_dcs.as_deref()),
        (Some("Hi"), Some("300"))
    );
    let c = parse_cusd(r#"+CUSD: 0,"Hi",x"#).unwrap();
    assert_eq!(c.invalid_dcs.as_deref(), Some("x"));
    let c = parse_cusd(r#"+CUSD: 0,"Hi",15"#).unwrap();
    assert_eq!((c.dcs, c.invalid_dcs), (Some(15), None));
}

#[test]
fn cbs_dcs_language() {
    assert_eq!(CbsDcs::decode(0x00).language(), Some("de"));
    assert_eq!(CbsDcs::decode(0x01).language(), Some("en"));
    assert_eq!(CbsDcs::decode(0x0E).language(), Some("pl"));
    assert_eq!(CbsDcs::decode(0x0F).language(), None);
    assert_eq!(CbsDcs::decode(0x20).language(), Some("cs"));
    assert_eq!(CbsDcs::decode(0x23).language(), Some("ru"));
    assert_eq!(CbsDcs::decode(0x24).language(), Some("is"));
    assert_eq!(CbsDcs::decode(0x25).language(), None);
    assert_eq!(CbsDcs::decode(0x10).language(), None);
    assert_eq!(CbsDcs::decode(0x48).language(), None);
    let t = ussd::decode(&ussd::encode("Hi").unwrap(), 0x01).unwrap();
    assert_eq!((t.language.as_deref(), t.text.as_str()), (Some("en"), "Hi"));
    // The 0x10 prefix still wins.
    let t = ussd::decode(&ussd::encode("fr\rSalut").unwrap(), 0x10).unwrap();
    assert_eq!(t.language.as_deref(), Some("fr"));
}

#[test]
fn cbs_dcs_groups() {
    use serial_codec::sms::pdu::Alphabet::*;
    let d = |raw| {
        let d = CbsDcs::decode(raw);
        (d.alphabet, d.reserved)
    };
    assert_eq!(d(0x01), (Gsm7, false));
    assert_eq!(d(0x20), (Gsm7, false));
    assert_eq!(d(0x24), (Gsm7, false));
    assert_eq!(d(0x25), (Gsm7, true));
    assert_eq!(d(0x40), (Gsm7, false));
    assert_eq!(d(0x44), (Bit8, false));
    assert_eq!(d(0x48), (Ucs2, false));
    assert_eq!(d(0x4C), (Gsm7, true));
    assert_eq!(d(0x90), (Gsm7, false));
    assert_eq!(d(0x94), (Bit8, false));
    assert_eq!(d(0x98), (Ucs2, false));
    assert_eq!(d(0xF0), (Gsm7, false));
    assert_eq!(d(0xF4), (Bit8, false));
}

#[test]
fn gsm7_text_without_language_prefix_keeps_a_cr_after_two_chars() {
    let packed = ussd::encode("ab\rcd").unwrap();
    let t = ussd::decode(&packed, 0x0F).unwrap();
    assert_eq!((t.text.as_str(), t.language), ("ab\rcd", None));
}

#[test]
fn unpack_keeps_two_crs_off_the_octet_boundary() {
    // `A CR CR`: 21 bits in 3 octets; the second CR is data, not the extra CR.
    let septets = [0x41, 0x0D, 0x0D];
    let packed = ussd::pack_septets(&septets, 0);
    assert_eq!(packed.len(), 3);
    assert_eq!(ussd::unpack_septets(&packed, 0), septets);
}

#[test]
fn cusd_unquoted_text_and_bare_ok() {
    let c = parse_cusd("+CUSD: 0,abc,def").unwrap();
    assert_eq!(
        (c.raw.as_deref(), c.dcs, c.invalid_dcs),
        (Some("abc,def"), None, None)
    );
    let c = parse_cusd("+CUSD: 0,abc,").unwrap();
    assert_eq!((c.raw.as_deref(), c.dcs), (Some("abc,"), None));
    let c = parse_cusd("+CUSD: 0,abc,15").unwrap();
    assert_eq!((c.raw.as_deref(), c.dcs), (Some("abc"), Some(15)));
    let c = parse_cusd("+CUSD: 2 OK").unwrap();
    assert_eq!((c.status, c.raw), (CusdStatus::Terminated, None));
}
