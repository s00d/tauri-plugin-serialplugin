//! AT reply handling: chunking independence for the demux and the completion check,
//! generated exchanges for [`parse_at_response`], "no input panics, output stays
//! bounded" for every parser, and stories for modem quirks found in the field (the
//! demux model is in `demux_model.rs`). Phone numbers and PDUs are fake.

use crate::common::*;
use serial_codec::at::{
    at_final_line_complete, at_intermediate_line_complete, classify_final_line,
    classify_numeric_final_line, derive_solicited_prefixes, parse_at_response, split_lines,
    urc_lines_from_drain, AtCompletion, AtParseStatus, AtResultFormat, ExchangeDemux,
    ExchangeMatch, ExchangeResponse, MAX_DEMUX_LINE,
};
use serial_codec::at::{
    classify_intermediate_line, classify_intermediate_line_with_format, is_ext_info_line,
    is_numeric_result_line,
};
use AtResultFormat as F;

const CMD: &str = "AT+CSQ";
const PDU: &str = "07910000000000F0040B910000000000F000004210101000000004D4F29C0E";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn cme(line: &str) -> Option<i32> {
    match classify_final_line(line) {
        Some(ExchangeMatch::CmeError { code } | ExchangeMatch::CmsError { code }) => code,
        other => panic!("{line}: {other:?}"),
    }
}

fn ok(buf: &[u8]) -> Option<ExchangeMatch> {
    at_final_line_complete(buf, F::Verbose)
}

fn format() -> impl Strategy<Value = AtResultFormat> {
    select(vec![AtResultFormat::Verbose, AtResultFormat::Numeric])
}

/// (line, match): V.250 / 27.007 / 27.005 final results.
pub(crate) fn finals() -> Vec<(String, ExchangeMatch)> {
    use ExchangeMatch as M;
    vec![
        ("OK".into(), M::Ok),
        ("ERROR".into(), M::Error),
        ("+CME ERROR: 10".into(), M::CmeError { code: Some(10) }),
        ("+CMS ERROR: 500".into(), M::CmsError { code: Some(500) }),
        (
            "+CME ERROR: SIM not inserted".into(),
            M::CmeError { code: Some(10) },
        ),
        ("NO CARRIER".into(), M::NoCarrier),
        ("BUSY".into(), M::Busy),
        ("SEND OK".into(), M::SendOk),
    ]
}

/// Byte soup from AT-ish pieces: lines, quotes, CR/LF, broken UTF-8.
fn soup() -> impl Strategy<Value = Vec<u8>> {
    let tokens: Vec<&[u8]> = vec![
        b"AT+CSQ",
        b"at",
        b"\r",
        b"\n",
        b"\r\n",
        b"OK",
        b"ok",
        b"ERROR",
        b"0",
        b"1",
        b"> ",
        b"+CUSD: 1,\"",
        b"\"",
        b",15",
        b"RING",
        b"+CMTI: \"SM\",1",
        b"+CMT: ,5",
        b"0791AB",
        b"+CSQ: 1,2",
        b"+CME ERROR: 3",
        b"CONNECT",
        b"xyz",
        b" ",
        "\u{e9}".as_bytes(),
        b"\xFF",
        b"\xC3",
        b"+CMGR: 0,,5",
        b"+CDS: 6",
        b"OK+CUSD: 2",
    ];
    vec(select(tokens), 0..60).prop_map(|t| t.concat())
}

proptest! {
    #![proptest_config(cases(256))]

    /// Live URCs and the phase do not depend on how the reply is split into reads.
    #[test]
    fn demux_chunking_independent(fmt in format(), data in soup(), sizes in vec(1usize..16, 1..5)) {
        let run = |parts: Vec<&[u8]>| {
            let mut d = ExchangeDemux::with_format(CMD, &[], fmt);
            let urcs: Vec<String> = parts.into_iter().flat_map(|c| d.process_chunk(c)).collect();
            (urcs, d.phase())
        };
        let whole = run(vec![&data]);
        prop_assert_eq!(run(chunks(&data, &sizes)), whole.clone());
        prop_assert_eq!(run(chunks(&data, &[1])), whole);
    }

    /// After each read the incremental completion check answers what the one-shot check
    /// answers for all bytes so far.
    #[test]
    fn incremental_completion_matches_one_shot(
        fmt in format(), intermediate in any::<bool>(), data in soup(), sizes in vec(1usize..16, 1..5),
    ) {
        let mut c = AtCompletion::new(intermediate, fmt);
        let mut seen = 0;
        for part in chunks(&data, &sizes) {
            seen += part.len();
            let one_shot = if intermediate {
                at_intermediate_line_complete(&data[..seen], fmt)
            } else {
                at_final_line_complete(&data[..seen], fmt)
            };
            prop_assert_eq!(c.push(part), one_shot, "after {} bytes", seen);
        }
    }

    /// No input panics; lines and URCs are made of input bytes.
    #[test]
    fn parsers_never_panic(data in prop_oneof![soup(), vec(any::<u8>(), 0..300)], fmt in format(), cmd in "[A-Za-z+^&;=?0-9 ]{0,16}") {
        let text = String::from_utf8_lossy(&data);
        let n = text.len();
        let p = parse_at_response(&data, &cmd, &["+X:".into()], fmt);
        prop_assert!(p.lines.iter().map(String::len).sum::<usize>() <= n);
        prop_assert!(p.solicited_body.len() + p.urc_lines.len() <= p.lines.len());
        prop_assert!(split_lines(&text).iter().all(|l| !l.is_empty() && l.len() <= n));
        prop_assert!(urc_lines_from_drain(&data, &[]).iter().map(String::len).sum::<usize>() <= n);
        let mut d = ExchangeDemux::with_format(&cmd, &[], fmt);
        prop_assert!(d.process_chunk(&data).iter().map(String::len).sum::<usize>() <= n);
        let _ = at_final_line_complete(&data, fmt);
        let _ = at_intermediate_line_complete(&data, fmt);
        let _ = derive_solicited_prefixes(&cmd);
        let _ = classify_final_line(&text);
    }
}

// ------------------------------------------------------------------ whole exchanges

/// A command, its reply lines and the URCs it may interleave.
fn command() -> impl Strategy<Value = (&'static str, Vec<&'static str>)> {
    select(vec![
        ("AT+CSQ", vec!["+CSQ: 20,99", "Manufacturer ACME"]),
        ("AT^SYSCFG?", vec!["^SYSCFG: 2,2,3FFFFFFF,1,1"]),
        ("AT+COPS?;+CREG?", vec!["+COPS: 0,0,\"Op\",7", "+CREG: 0,1"]),
        ("ATE0V1+CGMI", vec!["+CGMI: ACME", "ACME"]),
    ])
}

#[derive(Debug, Clone)]
enum Line {
    Body(usize),
    Urc(usize),
}

proptest! {
    #![proptest_config(cases(256))]

    /// A generated exchange (optional echo, reply lines with URCs between them, a final
    /// result, trailing URCs and an unterminated tail) parses into exactly those parts;
    /// the completion check sees the same final result.
    #[test]
    fn exchange_parses_into_its_parts(
        (cmd, body) in command(),
        echo in any::<bool>(),
        before in vec(0..3usize, 0..3),
        lines in vec(prop_oneof![(0..2usize).prop_map(Line::Body), (0..3usize).prop_map(Line::Urc)], 0..8),
        fin in 0..8usize,
        after in vec(0..3usize, 0..3),
        tail in "[A-Z+ ]{0,6}",
    ) {
        let urcs = ["+CMTI: \"SM\",3", "RING", "^MODE: 3,8"];
        let (final_line, matched) = finals()[fin].clone();
        let mut raw = String::new();
        let mut all = Vec::new();
        let mut want_body = Vec::new();
        let mut want_urc = Vec::new();
        for &u in &before {
            raw += &format!("{}\r\n", urcs[u]);
            all.push(urcs[u].to_string());
            want_urc.push(urcs[u].to_string());
        }
        if echo {
            raw += &format!("{cmd}\r\r\n");
            all.push(cmd.to_string());
        }
        for l in &lines {
            let (s, to) = match l {
                Line::Body(i) => (body[*i % body.len()], &mut want_body),
                Line::Urc(i) => (urcs[*i], &mut want_urc),
            };
            raw += &format!("{s}\r\n");
            all.push(s.to_string());
            to.push(s.to_string());
        }
        raw += &format!("\r\n{final_line}\r\n");
        all.push(final_line.clone());
        for &u in &after {
            raw += &format!("\r\n{}\r\n", urcs[u]);
            all.push(urcs[u].to_string());
            want_urc.push(urcs[u].to_string());
        }
        raw += &tail;
        let p = parse_at_response(raw.as_bytes(), cmd, &[], AtResultFormat::Verbose);
        prop_assert_eq!(&p.lines, &all);
        prop_assert_eq!(&p.solicited_body, &want_body);
        prop_assert_eq!(&p.urc_lines, &want_urc);
        prop_assert_eq!(p.final_line.as_deref(), Some(final_line.as_str()));
        prop_assert_eq!(p.status, matched.to_parse_status());
        prop_assert_eq!(at_final_line_complete(raw.as_bytes(), AtResultFormat::Verbose), Some(matched.clone()));
        let r = ExchangeResponse::from_raw(raw.clone().into_bytes(), matched.clone(), cmd, &[], AtResultFormat::Verbose);
        prop_assert_eq!((r.solicited_body, r.urc_lines, r.status), (want_body, want_urc, p.status));
    }

    /// Verbose result codes are case-insensitive; numeric error codes come through.
    #[test]
    fn final_results_classify(
        fin in 0..8usize,
        mask in vec(any::<bool>(), 32),
        code in 0i32..100_000,
        tail in "[a-zA-Z ]{0,10}",
    ) {
        let (line, want) = finals()[fin].clone();
        let mixed: String = line.chars().zip(mask.iter().cycle())
            .map(|(c, &up)| if up { c.to_ascii_uppercase() } else { c.to_ascii_lowercase() })
            .collect();
        prop_assert_eq!(classify_final_line(&format!("  {mixed}\r\n")), Some(want));
        prop_assert_eq!(classify_final_line(&format!("+CME ERROR: {code}")), Some(ExchangeMatch::CmeError { code: Some(code) }));
        prop_assert_eq!(classify_final_line(&format!("+CMS ERROR: {code}: {tail}")), Some(ExchangeMatch::CmsError { code: Some(code) }));
        prop_assert_eq!(classify_numeric_final_line(&code.to_string()).is_some(), [0, 3, 4, 6, 7, 8].contains(&code));
    }
}

proptest! {
    #![proptest_config(cases(512))]

    /// Extended information lines start with one of `+ ^ # $ % *`; a numeric result
    /// line is one or two digits.
    #[test]
    fn ext_and_numeric_line_shapes(line in "[ ]?[+^#$%*A-Z0-9a]{0,4}[ ]?") {
        let t = line.trim();
        prop_assert_eq!(is_ext_info_line(&line), t.starts_with(['+', '^', '#', '$', '%', '*']));
        prop_assert_eq!(is_numeric_result_line(&line),
            !t.is_empty() && t.len() <= 2 && t.bytes().all(|b| b.is_ascii_digit()));
    }
}

// ------------------------------------------------------------------ stories

/// Given the 27.005 SMS prompt `> ` (no line feed), `CONNECT`, `CONNECT 9600` or a
/// numeric `1`, or a prompt command answered with `+CMS ERROR`
/// When the prompt completion and the final completion run
/// Then the prompt check completes on all of them, the final check on none but the
/// error, and `1` is intermediate only in numeric mode
#[test]
fn prompt_and_connect() {
    use serial_codec::at::at_intermediate_line_complete as im;
    let inter = |m: Option<ExchangeMatch>| matches!(m, Some(ExchangeMatch::Intermediate { .. }));
    assert!(inter(im(b"AT+CMGS=20\r\r\n> ", F::Verbose)));
    assert!(inter(im(b"AT+CMGS=1\r\r\n>\r\n", F::Verbose)));
    assert!(inter(im(b"ATD123\r\r\nCONNECT\r\n", F::Verbose)));
    assert!(inter(im(b"ATD\r\r\n1\r\n", F::Numeric)));
    assert!(!inter(im(b"ATD\r\r\n1\r\n", F::Verbose)));
    assert_eq!(ok(b"ATD123\r\r\nCONNECT\r\n"), None);
    assert_eq!(at_final_line_complete(b"ATD\r\r\n1\r\n", F::Numeric), None);
    assert!(classify_intermediate_line("CONNECT 9600"));
    assert!(!classify_intermediate_line_with_format("5", F::Numeric));
    assert_eq!(
        im(b"AT+CMGS=20\r\r\n+CMS ERROR: 304\r\n", F::Verbose),
        Some(ExchangeMatch::CmsError { code: Some(304) })
    );
}

/// Given a `+CUSD` quote that is still open when the text ends
/// When the text is split into lines
/// Then the open line and what follows stay separate lines
#[test]
fn unclosed_cusd_quote_at_the_end() {
    assert_eq!(
        split_lines("+CUSD: 1,\"abc\nxyz"),
        s(&["+CUSD: 1,\"abc", "xyz"])
    );
}

/// Given a `+CUSD` quote that closes exactly at the 2048-byte span cap, or one byte later
/// When the text is split into lines
/// Then the first is one line and the second is cut after its first segment
#[test]
fn cusd_quote_span_cap_is_inclusive() {
    let head = "+CUSD: 0,\""; // 10 bytes; + 2037 + LF = 2048
    let fits = format!("{head}\n{}\"", "a".repeat(2036));
    assert_eq!(split_lines(&fits).len(), 1);
    let over = format!("{head}\n{}\"", "a".repeat(2037));
    assert_eq!(
        split_lines(&over),
        vec![head.to_string(), format!("{}\"", "a".repeat(2037))]
    );
}

/// Given text glued to a `+…:` line without CRLF
/// When it is split into lines
/// Then it is cut only when the head is a final result and the tail has a URC shape
#[test]
fn glued_final_needs_a_final_head_and_a_urc_tail() {
    assert_eq!(split_lines("OK+CUSD: 1"), s(&["OK", "+CUSD: 1"]));
    assert_eq!(split_lines("OK+CMGR: 1,10\r\n"), s(&["OK", "+CMGR: 1,10"]));
    assert_eq!(split_lines("abc+CMTI: 1"), s(&["abc+CMTI: 1"]));
    assert_eq!(split_lines("OK+7 900"), s(&["OK+7 900"]));
}

/// Given lines after the echo of a known command
/// When the reply is parsed
/// Then only `RING` and `+NAME:` / `^NAME:` lines with an upper-case name are URCs
#[test]
fn urc_shape_needs_an_upper_case_name() {
    let raw = b"AT+CSQ\r\r\n+CMTI: \"SM\",1\r\nRING\r\n+Cmti: 1\r\n+1AB: 1\r\nCMTI: 1\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.urc_lines, s(&["+CMTI: \"SM\",1", "RING"]));
    assert_eq!(p.solicited_body, s(&["+Cmti: 1", "+1AB: 1", "CMTI: 1"]));
}

/// Given headers whose next line is data (`+CMT`, `+CBM`, PDU-mode `+CDS: <len>`) and
/// ones that have none (text-mode `+CDS: <fo>,…`, an empty `+CDS:`)
/// When they are drained followed by a plain line
/// Then the plain line joins the header only in the first group; a text-mode `+CDS`
/// after `OK` is a single URC line
#[test]
fn two_line_headers() {
    for (head, joined) in [
        ("+CMT: ,20", true),
        ("+CBM: 20", true),
        ("+CDS: 25", true),
        ("+CDS: 6,1", false),
        ("+CDS:", false),
    ] {
        let drained = urc_lines_from_drain(format!("{head}\r\nhello\r\n").as_bytes(), &[]);
        assert_eq!(drained.len(), if joined { 2 } else { 1 }, "{head}");
    }
    let raw = b"AT\r\r\nOK\r\n+CDS: 6,12,\"+10000000000\",145,\"24/01/01,10:00:00+00\"\r\n";
    assert_eq!(
        parse_at_response(raw, "AT", &[], F::Verbose)
            .urc_lines
            .len(),
        1
    );
}

/// Given an unclosed `+CUSD` quote fed to the demux (verbose, and `ATV0` with CR-only
/// line ends)
/// When a result code (`OK`, numeric `0`) arrives in the next read
/// Then the result ends the quote early, the truncated `+CUSD` line is emitted, and a
/// later URC is not held behind the quote
#[test]
fn demux_result_code_cuts_an_open_cusd_quote() {
    let mut demux = ExchangeDemux::new("AT", &[]);
    assert!(demux.process_chunk(b"+CUSD: 0,\"abc\r\n").is_empty());
    assert_eq!(demux.process_chunk(b"OK\r\n"), s(&["+CUSD: 0,\"abc"]));

    let mut demux = ExchangeDemux::with_format("AT", &[], F::Numeric);
    assert!(demux.process_chunk(b"+CUSD: 1,\"menu\r").is_empty());
    assert_eq!(demux.process_chunk(b"0\r"), s(&["+CUSD: 1,\"menu"]));
    assert_eq!(demux.process_chunk(b"RING\r"), s(&["RING"]));
}

/// Given a reply `OK` followed in the same read by a two-line `+CMT` and its PDU
/// When completion and parsing look at the read
/// Then the exchange is complete with `OK` and both `+CMT` lines are URCs
#[test]
fn pdu_line_of_cmt_after_ok_does_not_hide_it() {
    let raw = format!("AT\r\r\nOK\r\n\r\n+CMT: ,23\r\n{PDU}\r\n");
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw.as_bytes(), "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CMT: ,23", PDU]));
}

/// Given a `+CSQ` reply, `OK`, then a PDU-mode `+CDS: <len>` and its PDU
/// When the read is parsed
/// Then the body is the `+CSQ` line and the `+CDS` pair is the URC
#[test]
fn pdu_line_of_cds_after_ok_does_not_hide_it() {
    let raw = format!("AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\n+CDS: 25\r\n{PDU}\r\n");
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
    assert_eq!(p.urc_lines, s(&["+CDS: 25", PDU]));
}

/// Given `OK` followed by boot banners (`RDY`, `Call Ready`, `SMS Ready`)
/// When the read is parsed
/// Then the exchange ends with `OK` and the banners are URCs
#[test]
fn boot_status_lines_after_ok_do_not_hide_it() {
    let raw = b"AT\r\r\nOK\r\n\r\nRDY\r\n\r\nCall Ready\r\n\r\nSMS Ready\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["RDY", "Call Ready", "SMS Ready"]));
}

/// Given `OK` followed by a `+CUSD` whose quoted menu spans lines
/// When the read is parsed
/// Then the exchange ends with `OK` and the whole menu is one URC
#[test]
fn multiline_cusd_after_ok_does_not_hide_it() {
    let raw = b"AT+CUSD=1,\"*100#\",15\r\r\nOK\r\n\r\n+CUSD: 1,\"1. Balance\n2. Tariff\",15\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT+CUSD=1,\"*100#\",15", &[], F::Verbose);
    assert_eq!(p.urc_lines, s(&["+CUSD: 1,\"1. Balance\n2. Tariff\",15"]));
}

/// Given `ATE0` (no echo) and a `+CMT` pair in the middle of a `+CSQ` reply
/// When the reply is parsed
/// Then the pair stays together as URCs and `+CSQ` is the body
#[test]
fn cmt_before_final_keeps_its_pdu_line() {
    // ATE0: no echo, the URC lands in the middle of the reply.
    let raw = format!("+CMT: ,23\r\n{PDU}\r\n+CSQ: 20,99\r\n\r\nOK\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CMT: ,23", PDU]));
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
}

/// Given drained bytes with `+CMT`, `+CDS <len>` and `+CBM` headers, each with its PDU, and noise
/// When URC lines are extracted
/// Then each header comes with its data line and the noise is dropped
#[test]
fn drain_keeps_two_line_urcs_together() {
    let raw = format!("+CMT: ,23\r\n{PDU}\r\n+CDS: 25\r\n{PDU}\r\n+CBM: 88\r\n{PDU}\r\nnoise\r\n");
    assert_eq!(
        urc_lines_from_drain(raw.as_bytes(), &[]),
        s(&["+CMT: ,23", PDU, "+CDS: 25", PDU, "+CBM: 88", PDU])
    );
}

/// Given a PDU-mode `+CMGL` / `+CMGR` reply
/// When it is parsed
/// Then headers and their PDU lines are the body, no URC
#[test]
fn solicited_cmgl_pdu_body_still_works() {
    let raw = format!("+CMGL: 1,1,,26\r\n{PDU}\r\n+CMGL: 2,1,,26\r\n{PDU}\r\n\r\nOK\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT+CMGL=4", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(
        p.solicited_body,
        s(&["+CMGL: 1,1,,26", PDU, "+CMGL: 2,1,,26", PDU])
    );
    assert!(p.urc_lines.is_empty());
    let raw = format!("AT+CMGR=1\r\r\n+CMGR: 1,,26\r\n{PDU}\r\n\r\nOK\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT+CMGR=1", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CMGR: 1,,26", PDU]));
}

/// Given a modem that glues `OK` and `+CUSD:` without CRLF
/// When the line is split, completed, parsed and demuxed
/// Then `OK` ends the exchange and the `+CUSD` line is a URC in all three
#[test]
fn glued_ok_cusd_is_split() {
    assert_eq!(
        split_lines("OK+CUSD: 0,\"x\",15\r\n"),
        s(&["OK", "+CUSD: 0,\"x\",15"])
    );
    let raw = b"AT+CUSD=1,\"*100#\"\r\r\nOK+CUSD: 0,\"x\",15\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT+CUSD=1,\"*100#\"", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CUSD: 0,\"x\",15"]));
    let mut demux = ExchangeDemux::new("AT", &[]);
    assert_eq!(
        demux.process_chunk(b"OK+CUSD: 0,\"x\",15\r\n"),
        s(&["+CUSD: 0,\"x\",15"])
    );
}

/// Given a glued `OK+CUSD: …` without its line end
/// When the line end arrives in a later read
/// Then nothing is judged before it and the URC comes out once
#[test]
fn glued_urc_waits_for_its_terminator() {
    // Only complete lines are judged; the glued URC comes out once its LF arrives.
    assert_eq!(ok(b"OK+CUSD: 0,\"x\",15"), None);
    let mut demux = ExchangeDemux::new("AT", &[]);
    assert!(demux.process_chunk(b"OK+CUSD: 0,\"x\",15").is_empty());
    assert_eq!(demux.process_chunk(b"\r\n"), s(&["+CUSD: 0,\"x\",15"]));
}

/// Given `+CUSD` texts with raw CR, LF and CRLF inside the quotes
/// When the text is split into lines
/// Then each `+CUSD` stays one line with its CR/LF and the next line is separate
#[test]
fn cusd_text_keeps_embedded_cr_and_lf() {
    assert_eq!(
        split_lines("\r\n+CUSD: 2,\"MSISDN:\r10000000000\",15\r\n\r\nOK\r\n"),
        s(&["+CUSD: 2,\"MSISDN:\r10000000000\",15", "OK"])
    );
    assert_eq!(
        split_lines("+CUSD: 1,\"1. Balance\n2. Tariff\",15\r\n"),
        s(&["+CUSD: 1,\"1. Balance\n2. Tariff\",15"])
    );
    assert_eq!(
        split_lines("+CUSD: 1,\"1. Balance\r\n2. Tariff\",15\r\nOK\r\n"),
        s(&["+CUSD: 1,\"1. Balance\r\n2. Tariff\",15", "OK"])
    );
}

/// Given a `+CUSD` quote that never closes, followed by `OK`, another URC or 2 KiB of lines
/// When the text is split and completed
/// Then the quote is cut there and the result / URC after it still counts
#[test]
fn truncated_cusd_quote_does_not_swallow_result_or_urc() {
    assert_eq!(
        split_lines("+CUSD: 0,\"abc\r\nOK\r\n"),
        s(&["+CUSD: 0,\"abc", "OK"])
    );
    assert_eq!(
        split_lines("+CUSD: 0,\"abc\r\n+CSQ: 20,99\r\n"),
        s(&["+CUSD: 0,\"abc", "+CSQ: 20,99"])
    );
    let long = format!("+CUSD: 0,\"{}\r\n", "A\r\n".repeat(2048));
    assert_eq!(
        split_lines(&long).first().map(String::as_str),
        Some("+CUSD: 0,\"A")
    );
    assert_eq!(
        ok(b"AT+CUSD=1\r\r\n+CUSD: 0,\"abc\r\nOK\r\n"),
        Some(ExchangeMatch::Ok)
    );
}

/// Given `+CME ERROR` with verbose texts (`AT+CMEE=2`) in any case, quoted, or unknown
/// When the final line is classified
/// Then known texts map to their 27.007 codes and unknown text gives no code
#[test]
fn verbose_cme_texts_map_to_codes() {
    for (line, code) in [
        ("+CME ERROR: SIM busy", 14),
        ("+CME ERROR: SIM not inserted", 10),
        ("+CME ERROR: sim pin required", 11),
        ("+CME ERROR: SIM failure", 13),
        ("+CME ERROR: operation not allowed", 3),
        ("+CME ERROR: \"SIM PUK required\"", 12),
        ("+CME ERROR: 14", 14),
    ] {
        assert_eq!(cme(line), Some(code), "{line}");
    }
    assert_eq!(cme("+CME ERROR: raspberry"), None);
}

/// Given `+CME ERROR: 10: SIM not inserted`
/// When it is classified
/// Then the leading number is the code
#[test]
fn cme_code_with_text() {
    assert_eq!(cme("+CME ERROR: 10: SIM not inserted"), Some(10));
}

/// Given `+CMS ERROR` with verbose texts
/// When it is classified
/// Then known texts map to their 27.005 codes, unknown text to none
#[test]
fn verbose_cms_texts_map_to_codes() {
    assert_eq!(cme("+CMS ERROR: operation not allowed"), Some(302));
    assert_eq!(cme("+CMS ERROR: SIM busy"), Some(314));
    assert_eq!(cme("+CMS ERROR: memory full"), Some(322));
    assert_eq!(cme("+CMS ERROR: 314"), Some(314));
    assert_eq!(cme("+CMS ERROR: raspberry"), None);
}

/// Given a classified `+CME` / `+CMS` error
/// When it is serialized to JSON and read back
/// Then the wire form is `{cmeError: {code}}` (the frontend contract)
#[test]
fn cme_error_serializes_as_code_only() {
    let m = classify_final_line("+CME ERROR: 10").unwrap();
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        serde_json::json!({ "cmeError": { "code": 10 } })
    );
    let m = classify_final_line("+CME ERROR: SIM busy").unwrap();
    assert_eq!(
        serde_json::to_value(&m).unwrap(),
        serde_json::json!({ "cmeError": { "code": 14 } })
    );
    let back: ExchangeMatch = serde_json::from_str(r#"{"cmsError":{"code":304}}"#).unwrap();
    assert_eq!(back, ExchangeMatch::CmsError { code: Some(304) });
}

/// Given a text-mode `+CMGR` whose SMS text starts like an extended line (`*100#`, `+7 …`, `#tag`)
/// When it is parsed and demuxed without an echo
/// Then the text is body, never a URC
#[test]
fn text_mode_cmgr_body_is_not_urc_without_echo() {
    for body in ["*100#", "+7 900 000 00 00", "#tag", "$5 off", "%done"] {
        let raw = format!(
            "+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n{body}\r\n\r\nOK\r\n"
        );
        let p = parse_at_response(raw.as_bytes(), "AT+CMGR=1", &[], F::Verbose);
        assert!(p.urc_lines.is_empty(), "{body}: {:?}", p.urc_lines);
        assert_eq!(p.solicited_body.last().map(String::as_str), Some(body));
        let mut demux = ExchangeDemux::new("AT+CMGR=1", &[]);
        assert!(demux.process_chunk(raw.as_bytes()).is_empty(), "{body}");
    }
}

/// Given a text-mode `+CMGL` with multi-line bodies and a real `+CMTI` between them
/// When it is parsed
/// Then only `+CMTI` is a URC, the six other lines are body
#[test]
fn text_mode_cmgl_multiline_body_with_interleaved_urc() {
    let raw = "+CMGL: 1,\"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n\
               call me\r\n+7 900 000 00 00\r\n#tag\r\n+CMTI: \"SM\",2\r\n\
               +CMGL: 2,\"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n*100#\r\n\r\nOK\r\n";
    let p = parse_at_response(raw.as_bytes(), "AT+CMGL=\"ALL\"", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CMTI: \"SM\",2"]));
    assert_eq!(p.solicited_body.len(), 6);
}

/// Given a text-mode `+CMT` header and its body inside a `+CSQ` reply
/// When it is parsed
/// Then header and body are URCs, `+CSQ` is the body
#[test]
fn text_mode_cmt_body_belongs_to_urc() {
    let raw =
        "+CMT: \"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n#tag\r\n+CSQ: 20,99\r\n\r\nOK\r\n";
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.urc_lines.len(), 2);
    assert_eq!(p.urc_lines[1], "#tag");
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
}

/// Given an SMS body that reads `ok`
/// When completion and parsing run
/// Then it neither completes the exchange nor ends the body
#[test]
fn lowercase_ok_sms_body_is_data() {
    let raw = b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nok\r\n";
    assert_eq!(ok(raw), None);
    let raw = b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nok\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CMGR=1", &[], F::Verbose);
    assert_eq!(p.solicited_body.last().map(String::as_str), Some("ok"));
}

/// Given an SMS body `5` in a `+CMGL` reply
/// When completion runs in verbose and numeric mode
/// Then it is data in both (5 is no V.250 code)
#[test]
fn one_digit_sms_body_does_not_end_cmgl() {
    let head = "+CMGL: 1,\"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n5\r\n";
    assert_eq!(ok(head.as_bytes()), None);
    // `5` is not a V.250 code, so it is data in numeric mode too.
    assert_eq!(at_final_line_complete(head.as_bytes(), F::Numeric), None);
    let raw = format!("{head}OK\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT+CMGL=\"ALL\"", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body.last().map(String::as_str), Some("5"));
}

/// Given an SMS body `at home`
/// When the reply is parsed
/// Then it is body, not an echo
#[test]
fn lowercase_at_text_is_data_not_echo() {
    let raw =
        b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nat home\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CMGR=1", &[], F::Verbose);
    assert_eq!(p.solicited_body.last().map(String::as_str), Some("at home"));
}

/// Given `AT+CPIN?` answered by `+CPIN: NOT READY` without echo
/// When it is parsed
/// Then the line is the body, not a URC
#[test]
fn cpin_not_ready_is_command_response() {
    let p = parse_at_response(b"+CPIN: NOT READY\r\nOK\r\n", "AT+CPIN?", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, s(&["+CPIN: NOT READY"]));
    assert!(p.urc_lines.is_empty());
}

/// Given a vendor error text that is no result code
/// When `OK` follows later
/// Then only `OK` completes the exchange
#[test]
fn random_error_text_then_ok_completes() {
    assert_eq!(ok(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\n"), None);
    assert_eq!(
        ok(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\nOK\r\n"),
        Some(ExchangeMatch::Ok)
    );
}

/// Given binary garbage / invalid UTF-8 before `OK`
/// When completion runs
/// Then `OK` still completes
#[test]
fn garbage_prefix_before_ok_is_drained() {
    assert_eq!(ok(b"\x00\xFFGARBAGE\r\nOK\r\n"), Some(ExchangeMatch::Ok));
}

/// Given a 4 KiB garbage line before `OK`
/// When completion runs
/// Then `OK` still completes
#[test]
fn oversized_garbage_line_before_ok_drains() {
    let raw = format!("{}\r\nOK\r\n", "G".repeat(4096));
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
}

/// Given `ATV0`: result codes and echo end with CR alone (V.250 §5.7.1)
/// When the reply is completed, split, parsed and demuxed
/// Then CR ends the lines, the code completes and a URC glued behind it is routed
#[test]
fn numeric_mode_cr_only_lines_complete_and_split() {
    let m = |b: &[u8]| at_final_line_complete(b, F::Numeric);
    assert_eq!(m(b"AT\r0\r"), Some(ExchangeMatch::Ok));
    assert_eq!(m(b"AT\r\r\n0\r"), Some(ExchangeMatch::Ok));
    assert_eq!(m(b"AT+CSQ\r+CSQ: 1,2\r\n4\r"), Some(ExchangeMatch::Error));
    assert_eq!(m(b"AT\r"), None);
    assert_eq!(split_lines("AT\r0\r"), s(&["AT", "0"]));
    assert_eq!(
        split_lines("AT+CSQ\r+CSQ: 1,2\r\n0\r"),
        s(&["AT+CSQ", "+CSQ: 1,2", "0"])
    );
    let p = parse_at_response(b"AT+CSQ\r+CSQ: 1,2\r\n0\r", "AT+CSQ", &[], F::Numeric);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, s(&["+CSQ: 1,2"]));
    // A numeric-aware demux routes a URC glued behind a CR-only result code.
    let mut d = ExchangeDemux::with_format("AT+CSQ", &[], F::Numeric);
    let urc = d.process_chunk(b"AT+CSQ\r+CSQ: 1,2\r\n0\r+CREG: 1\r\n");
    assert_eq!(urc, s(&["+CREG: 1"]));
}

/// Given commands with V.250 basic commands before an extended one (`ATE1+CSQ`, `AT&F+CSQ`, `ATS3?+CSQ`, `ATD*99#`)
/// When solicited prefixes are derived
/// Then the extended command's header is used, `D` keeps its dial string, a malformed basic command keeps its name
#[test]
fn basic_commands_before_an_extended_one_are_skipped() {
    use serial_codec::at::derive_solicited_prefixes as d;
    assert_eq!(d("ATE1+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("ATE0V1+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("AT&F+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("AT&C1S0=0+CREG?;+CSQ"), s(&["+CREG:", "+CSQ:"]));
    // An S-register query (`S3?`) is a basic command too.
    assert_eq!(d("ATS3?+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("ATI"), s(&["I:"]));
    // `D` takes the rest of the part as its dial string, `*` and all.
    assert_eq!(d("ATD*99#"), s(&["D:"]));
    // A malformed basic command is no skip: the part keeps its own name.
    assert_eq!(d("ATS?+CSQ"), s(&["S:"]));
    assert!(d("AT&").is_empty());
    let raw = b"ATE1+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "ATE1+CSQ", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
    assert!(p.urc_lines.is_empty());
    assert!(ExchangeDemux::new("ATE1+CSQ", &[])
        .process_chunk(raw)
        .is_empty());
}

/// Given `CONNECT` or `OK` followed by PPP / CMUX bytes in the same read
/// When the prompt completion runs
/// Then the result is found and the data after it does not hide it
#[test]
fn connect_or_final_followed_by_data_in_the_same_read() {
    use serial_codec::at::at_intermediate_line_complete as im;
    let m = im(
        b"ATD*99#\r\r\nCONNECT 150000000\r\n~\xff}#\xc0!}!}!} }",
        F::Verbose,
    );
    assert_eq!(
        m,
        Some(ExchangeMatch::Intermediate {
            line: "CONNECT 150000000".into()
        })
    );
    assert_eq!(
        im(b"AT+CMUX=0\r\r\nOK\r\n\xF9\xF9", F::Verbose),
        Some(ExchangeMatch::Ok)
    );
    assert_eq!(
        ok(b"AT+CMUX=0\r\r\nOK\r\n\xF9\xF9\xF9\xF9"),
        Some(ExchangeMatch::Ok)
    );
    assert_eq!(im(b"ATD*99#\r\r\nCONNE", F::Verbose), None);
}

/// Given a reply that ends without a final result (prompt, timeout, only a URC)
/// When it is parsed
/// Then status is Unknown, no final line, every line is body or URC
#[test]
fn reply_without_final_keeps_every_line() {
    let p = parse_at_response(b"AT+CSQ\r\r\n+CSQ: 20,99\r\n", "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Unknown);
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
    assert_eq!(p.final_line, None);
    let p = parse_at_response(b"AT+CMGS=20\r\r\n> ", "AT+CMGS=20", &[], F::Verbose);
    assert_eq!(p.final_line, None);
    let p = parse_at_response(b"\r\n+CMTI: \"SM\",1\r\n", "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.urc_lines, s(&["+CMTI: \"SM\",1"]));
}

/// Given `AT+CLAC` without echo listing `+CGMI`, `+CGMM`, …
/// When it is parsed and demuxed
/// Then the list is body, no URC
#[test]
fn clac_list_without_echo_is_body() {
    let raw = b"\r\n+CGMI\r\n+CGMM\r\n+CSQ\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CLAC", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CGMI", "+CGMM", "+CSQ"]));
    assert!(p.urc_lines.is_empty());
    assert!(ExchangeDemux::new("AT+CLAC", &[])
        .process_chunk(raw)
        .is_empty());
}

/// Given an `OK` followed by a text `+CMT` whose body starts with `ATM`
/// When completion runs
/// Then the body is no echo; a real new echo still makes an earlier `OK` stale
#[test]
fn cmt_text_starting_with_at_does_not_reset_completion() {
    let raw = b"AT\r\r\nOK\r\n+CMT: \"+10000000001\",,\"24/01/01,00:00:00+00\"\r\nATM closed\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    // A real new echo still makes an earlier final stale.
    assert_eq!(ok(b"OK\r\nAT+CSQ\r\r\n"), None);
    assert_eq!(ok(b"OK\r\nat+csq\r\r\n"), None);
}

/// Given vendor finals (`COMMAND NOT SUPPORT`, `SEND OK`)
/// When they are classified
/// Then only the TCP-stack `SEND OK` is kept as a final
#[test]
fn vendor_final_text_is_not_a_result_code() {
    assert_eq!(classify_final_line("COMMAND NOT SUPPORT"), None);
    // Kept for compatibility (TCP stack finals, not V.250).
    assert_eq!(classify_final_line("SEND OK"), Some(ExchangeMatch::SendOk));
}

/// Given result-like lines before the echo (`NO CARRIER`, a stale `OK`)
/// When the reply is parsed
/// Then `NO CARRIER` is a URC, a stale `OK` is body, and only a final after the echo ends the reply
#[test]
fn result_codes_before_the_final_one() {
    // `NO CARRIER` before the echo is a URC; the final result is the `OK` after it.
    let p = parse_at_response(b"NO CARRIER\r\nATD1;\r\nOK\r\n", "ATD1;", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.final_line.as_deref(), Some("OK"));
    assert_eq!(p.urc_lines, s(&["NO CARRIER"]));
    assert!(p.solicited_body.is_empty());
    // A stale `OK` before the echo is no URC: it stays in the body.
    let p = parse_at_response(b"OK\r\nAT\r\nOK\r\n", "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, s(&["OK"]));
    assert!(p.urc_lines.is_empty());
    // Only a result before the echo: the reply has no final result yet.
    let p = parse_at_response(b"NO CARRIER\r\nATD1;\r\n", "ATD1;", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Unknown);
    assert_eq!(p.final_line, None);
    assert_eq!(p.urc_lines, s(&["NO CARRIER"]));
}

/// Given drained text-mode `+CMGR` with a body line `+7 900 …`
/// When URC lines are extracted
/// Then the body line is SMS text, not a URC
#[test]
fn drained_sms_text_body_needs_a_strict_urc_shape() {
    // No command: after the data line of a text `+CMGR`, `+7 900 …` is still SMS text.
    let raw = b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/02,03:04:05+08\"\r\nhello\r\n+7 900 123\r\n";
    assert_eq!(
        urc_lines_from_drain(raw, &[]),
        s(&[
            "+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/02,03:04:05+08\"",
            "hello"
        ])
    );
}

/// Given `AT+IFC=?` answered `+IFC (0,1,2)` without echo
/// When it is parsed
/// Then the line has no URC shape and is body
#[test]
fn known_command_without_echo_needs_a_strict_urc_shape() {
    // `ATE0`: no echo, but the command is known, so `+IFC (0,1,2)` (no colon) is reply text.
    let p = parse_at_response(b"+IFC (0,1,2)\r\nOK\r\n", "AT+IFC=?", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+IFC (0,1,2)"]));
    assert!(p.urc_lines.is_empty());
}

/// Given an open `+CUSD` quote whose kept bytes are exactly the demux cap
/// When the closing quote arrives
/// Then nothing was dropped and the span cap cuts the first line
#[test]
fn demux_open_cusd_quote_keeps_exactly_max_len_bytes() {
    // The open `+CUSD` quote waits for more bytes; what is kept (from the `+CUSD` line
    // on) is exactly the cap, so nothing is dropped.
    let head = "+CUSD: 1,\"ab\r\n";
    let rest = "c".repeat(MAX_DEMUX_LINE - head.len());
    let mut d = ExchangeDemux::new("AT", &[]);
    assert_eq!(
        d.process_chunk(format!("RING\r\n{head}{rest}").as_bytes()),
        s(&["RING"])
    );
    // The quoted text outgrew the `+CUSD` span cap: the first line ends truncated.
    assert_eq!(d.process_chunk(b"\"\r\n"), s(&["+CUSD: 1,\"ab"]));
}

/// Given V.250 numeric result codes (`ATV0`)
/// When each digit is classified, and a reply ends with `7`
/// Then 0/3/4/6/7/8 are finals (OK, NO CARRIER, ERROR, NO DIALTONE, BUSY, NO ANSWER),
/// 1 (CONNECT), 2 (RING), 5 and 9 are not, and the reply completes with BUSY
#[test]
fn numeric_codes_follow_v250() {
    use ExchangeMatch as M;
    let want = [
        Some(M::Ok),
        None,
        None,
        Some(M::NoCarrier),
        Some(M::Error),
        None,
        Some(M::NoDialtone),
        Some(M::Busy),
        Some(M::NoAnswer),
        None,
    ];
    for (n, w) in want.into_iter().enumerate() {
        assert_eq!(classify_numeric_final_line(&n.to_string()), w, "{n}");
    }
    assert_eq!(
        at_final_line_complete(b"ATD1;\r\r\n7\r\n", F::Numeric),
        Some(M::Busy)
    );
    assert_eq!(
        parse_at_response(b"AT\r\r\n0\r\n", "AT", &[], F::Numeric).status,
        AtParseStatus::Ok
    );
}
