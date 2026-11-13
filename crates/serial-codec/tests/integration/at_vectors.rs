//! AT replies, URCs and +CME/+CMS codes from other projects' tests, used as data only:
//! ModemManager (GPL-2.0-or-later; error table LGPL-2.0-or-later), gammu and chan_dongle
//! (GPL-2.0-only), python-gsmmodem (LGPL-3.0-or-later), zephyr (Apache-2.0).
//! See `fixtures/README.md`. Each fixture keeps the source's `input`/`expected`; `codec`
//! is what serial-codec must do with it (whole-reply completion and line routing).

use serde::Deserialize;
use serde_json::Value;
use serial_codec::at::{
    at_final_line_complete, at_intermediate_line_complete, classify_final_line, parse_at_response,
    urc_lines_from_drain, AtParseStatus, AtResultFormat as F, ExchangeDemux, ExchangeMatch,
};
use std::collections::HashMap;

#[derive(Deserialize)]
struct Exchange {
    command: String,
    raw: String,
    matched: Option<ExchangeMatch>,
    body: Vec<String>,
    urc: Vec<String>,
    #[serde(default)]
    solicited_prefixes: Vec<String>,
    #[serde(default)]
    completion: Option<String>,
}

#[derive(Deserialize)]
struct Reply {
    name: String,
    source: String,
    codec: Vec<Exchange>,
}

#[derive(Deserialize)]
struct UrcCodec {
    lines: Vec<String>,
}

#[derive(Deserialize)]
struct Urc {
    name: String,
    source: String,
    codec: UrcCodec,
}

fn replies() -> Vec<Reply> {
    serde_json::from_str(include_str!("../fixtures/at_replies.json")).unwrap()
}

fn urcs() -> Vec<Urc> {
    serde_json::from_str(include_str!("../fixtures/at_urc.json")).unwrap()
}

fn cme_cms() -> Vec<Value> {
    serde_json::from_str(include_str!("../fixtures/cme_cms.json")).unwrap()
}

/// What a V.250 verbose modem sends for these lines: `<CR><LF>text<CR><LF>`.
fn v250(lines: &[String]) -> String {
    lines.iter().map(|l| format!("\r\n{l}\r\n")).collect()
}

fn demux_live(command: &str, prefixes: &[String], raw: &[u8], byte_by_byte: bool) -> Vec<String> {
    let mut demux = ExchangeDemux::new(command, prefixes);
    if byte_by_byte {
        raw.iter()
            .flat_map(|b| demux.process_chunk(&[*b]))
            .collect()
    } else {
        demux.process_chunk(raw)
    }
}

// --- full replies ----------------------------------------------------------------------

#[test]
fn full_replies_complete_with_the_right_match() {
    let mut n = 0;
    for r in replies() {
        for ex in &r.codec {
            let raw = ex.raw.as_bytes();
            let got = match ex.completion.as_deref() {
                Some("intermediate") => at_intermediate_line_complete(raw, F::Verbose),
                _ => at_final_line_complete(raw, F::Verbose),
            };
            assert_eq!(got, ex.matched, "{} ({}): {:?}", r.name, r.source, ex.raw);
            n += 1;
        }
    }
    assert!(n > 150, "{n}");
}

#[test]
fn partial_replies_do_not_complete_before_the_final_line() {
    for r in replies() {
        for ex in r
            .codec
            .iter()
            .filter(|ex| ex.urc.is_empty() && ex.matched.is_some() && ex.completion.is_none())
        {
            let end = ex.raw.trim_end().len();
            for k in 0..end {
                let part = &ex.raw.as_bytes()[..k];
                assert_eq!(
                    at_final_line_complete(part, F::Verbose),
                    None,
                    "{} ({}) after {k} bytes: {:?}",
                    r.name,
                    r.source,
                    &ex.raw[..k]
                );
            }
        }
    }
}

#[test]
fn full_replies_split_into_body_and_urc() {
    for r in replies() {
        for ex in r.codec.iter().filter(|ex| ex.completion.is_none()) {
            let Some(m) = &ex.matched else { continue };
            let p = parse_at_response(
                ex.raw.as_bytes(),
                &ex.command,
                &ex.solicited_prefixes,
                F::Verbose,
            );
            let ctx = format!("{} ({}): {:?}", r.name, r.source, ex.raw);
            assert_eq!(p.status, m.to_parse_status(), "{ctx}");
            assert_eq!(p.solicited_body, ex.body, "{ctx}");
            assert_eq!(p.urc_lines, ex.urc, "{ctx}");
        }
    }
}

#[test]
fn demux_emits_exactly_the_urcs_of_each_reply() {
    for r in replies() {
        for ex in &r.codec {
            for byte_by_byte in [false, true] {
                let live = demux_live(
                    &ex.command,
                    &ex.solicited_prefixes,
                    ex.raw.as_bytes(),
                    byte_by_byte,
                );
                assert_eq!(
                    live, ex.urc,
                    "{} ({}) byte_by_byte={byte_by_byte}: {:?}",
                    r.name, r.source, ex.raw
                );
            }
        }
    }
}

// --- URCs ------------------------------------------------------------------------------

#[test]
fn urcs_are_urcs_in_a_drain() {
    for u in urcs() {
        let raw = v250(&u.codec.lines);
        assert_eq!(
            urc_lines_from_drain(raw.as_bytes(), &[]),
            u.codec.lines,
            "{} ({})",
            u.name,
            u.source
        );
    }
}

/// A URC before the echo, between echo and body, and after `OK` of an unrelated
/// command: never part of the body, never ends the exchange, emitted live once.
#[test]
fn urcs_around_and_inside_an_unrelated_reply() {
    let echo = "AT+CSQ\r\r\n";
    let body = "\r\n+CSQ: 20,99\r\n\r\nOK\r\n";
    for u in urcs() {
        let urc = v250(&u.codec.lines);
        for (place, raw) in [
            ("before echo", format!("{urc}{echo}{body}")),
            ("after echo", format!("{echo}{urc}{body}")),
            ("after OK", format!("{echo}{body}{urc}")),
            ("no echo (ATE0)", format!("{urc}{body}")),
        ] {
            let ctx = format!("{} ({}) {place}: {raw:?}", u.name, u.source);
            assert_eq!(
                at_final_line_complete(raw.as_bytes(), F::Verbose),
                Some(ExchangeMatch::Ok),
                "{ctx}"
            );
            let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
            assert_eq!(p.status, AtParseStatus::Ok, "{ctx}");
            assert_eq!(p.solicited_body, ["+CSQ: 20,99"], "{ctx}");
            assert_eq!(p.urc_lines, u.codec.lines, "{ctx}");
            for byte_by_byte in [false, true] {
                assert_eq!(
                    demux_live("AT+CSQ", &[], raw.as_bytes(), byte_by_byte),
                    u.codec.lines,
                    "{ctx} byte_by_byte={byte_by_byte}"
                );
            }
        }
    }
}

#[test]
fn composite_command_replies_are_body() {
    let raw = "AT+CSQ;+CREG?\r\r\n+CSQ: 20,99\r\n\r\n+CREG: 0,1\r\n\r\nOK\r\n";
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ;+CREG?", &[], F::Verbose);
    assert_eq!(p.solicited_body, ["+CSQ: 20,99", "+CREG: 0,1"]);
    assert!(p.urc_lines.is_empty());
    assert!(demux_live("AT+CSQ;+CREG?", &[], raw.as_bytes(), true).is_empty());
}

// --- +CME / +CMS -----------------------------------------------------------------------

fn kind_of(e: &Value) -> &str {
    e["input"]["kind"].as_str().unwrap_or_default()
}

/// ModemManager enum: (kind, name) → code, from its error header.
fn mm_codes() -> HashMap<(String, String), i64> {
    cme_cms()
        .iter()
        .filter(|e| e["name"] == "error_enum")
        .map(|e| {
            (
                (
                    kind_of(e).to_string(),
                    e["expected"]["name"].as_str().unwrap().to_string(),
                ),
                e["input"]["code"].as_i64().unwrap(),
            )
        })
        .collect()
}

fn error_code(line: &str) -> (Option<i32>, &'static str) {
    match classify_final_line(line) {
        Some(ExchangeMatch::CmeError { code }) => (code, "+CME ERROR"),
        Some(ExchangeMatch::CmsError { code }) => (code, "+CMS ERROR"),
        other => panic!("{line}: {other:?}"),
    }
}

/// GPRS / packet-domain codes of 27.007 §9.2.2 whose short text we map.
const GPRS_TEXT_CODES: &[i64] = &[103, 106, 107, 111, 112, 113, 132, 133, 134, 148, 149, 150];

#[test]
fn verbose_texts_map_to_the_27007_27005_codes() {
    let mut required = 0;
    for e in cme_cms().iter().filter(|e| e["name"] == "error_enum") {
        let kind = kind_of(e);
        let code = e["input"]["code"].as_i64().unwrap();
        // "Illegal MS (CS, GPRS, UMTS); Illegal UE (EPS, 5GS)." → "Illegal MS"
        let desc = e["expected"]["description"].as_str().unwrap();
        let text = desc.split(" (").next().unwrap().trim_end_matches('.');
        let line = format!("{kind}: {text}");
        let (got, got_kind) = error_code(&line);
        assert_eq!(got_kind, kind, "{line}");
        let must_map =
            code <= 63 || code == 100 || kind == "+CMS ERROR" || GPRS_TEXT_CODES.contains(&code);
        if must_map {
            assert_eq!(got.map(i64::from), Some(code), "{line} ({})", e["source"]);
            required += 1;
        } else if let Some(got) = got {
            assert_eq!(
                i64::from(got),
                code,
                "{line}: text maps to a different code"
            );
        }
        // The numeric form always passes the code through.
        assert_eq!(
            error_code(&format!("{kind}: {code}")).0.map(i64::from),
            Some(code)
        );
    }
    assert!(required >= 70, "{required}");
}

#[test]
fn modemmanager_normalized_texts_map_to_the_same_codes() {
    let codes = mm_codes();
    for e in cme_cms()
        .iter()
        .filter(|e| e["input"].get("text").is_some_and(Value::is_string))
    {
        let kind = kind_of(e);
        let text = e["input"]["text"].as_str().unwrap();
        let name = e["expected"]["name"].as_str().unwrap();
        let (got, _) = error_code(&format!("{kind}: {text}"));
        if name == "unknown" {
            // ModemManager folds unknown text into its generic "unknown"; we keep the
            // text and leave the code empty instead of inventing 100 / 500.
            assert_eq!(got, None, "{text}");
        } else {
            let want = codes[&(kind.to_string(), name.to_string())];
            assert_eq!(
                got.map(i64::from),
                Some(want),
                "{kind}: {text} ({})",
                e["source"]
            );
        }
    }
}

#[test]
fn out_of_table_numeric_codes_pass_through() {
    // ModemManager maps these to its generic "unknown"; the raw code is more useful.
    for e in cme_cms().iter().filter(|e| e["name"] == "for_code") {
        let (kind, code) = (kind_of(e), e["input"]["code"].as_i64().unwrap());
        assert_eq!(
            error_code(&format!("{kind}: {code}")).0.map(i64::from),
            Some(code)
        );
    }
}

#[test]
fn error_lines_from_python_gsmmodem() {
    for e in cme_cms()
        .iter()
        .filter(|e| e["source"].as_str().unwrap().starts_with("python-gsmmodem"))
    {
        let line = e["input"]["line"].as_str().unwrap();
        let got = classify_final_line(line);
        let want = if line == "ERROR" {
            ExchangeMatch::Error
        } else {
            let code = line.rsplit(' ').next().unwrap().parse().ok();
            if line.starts_with("+CME") {
                ExchangeMatch::CmeError { code }
            } else {
                ExchangeMatch::CmsError { code }
            }
        };
        assert_eq!(got, Some(want), "{line} ({})", e["source"]);
        if let Some(code) = e["expected"]["code"].as_i64() {
            assert!(line.ends_with(&code.to_string()));
        }
    }
}

/// gammu dispatch buffers: a doubled echo, then the error, without the trailing CRLF.
#[test]
fn gammu_dispatch_error_replies() {
    for e in cme_cms().iter().filter(|e| e["name"] == "at-dispatch") {
        let raw = e["input"]["response"].as_str().unwrap();
        let code: i32 = raw.rsplit(' ').next().unwrap().parse().unwrap();
        // Only complete lines are judged: without its CRLF the line may still grow.
        assert_eq!(at_final_line_complete(raw.as_bytes(), F::Verbose), None);
        let raw = format!("{raw}\r\n");
        let want = ExchangeMatch::CmeError { code: Some(code) };
        assert_eq!(
            at_final_line_complete(raw.as_bytes(), F::Verbose),
            Some(want),
            "{raw:?}"
        );
        let command = raw.split("\r\n").next().unwrap();
        let p = parse_at_response(raw.as_bytes(), command, &[], F::Verbose);
        assert_eq!(p.status, AtParseStatus::Cme);
    }
}
