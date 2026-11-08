//! AT parsing edge cases: modem quirks found in the field and tests ported from a
//! production SMS gateway (generic 27.007 / 27.005 / V.250 behaviour only).
//! Phone numbers and PDUs are fake.

use serial_codec::at::{
    at_final_line_complete, classify_final_line, classify_numeric_final_line, parse_at_response,
    split_lines, urc_lines_from_drain, AtParseStatus, AtResultFormat as F, ExchangeDemux,
    ExchangeMatch,
};

const PDU: &str = "07910000000000F0040B910000000000F000004210101000000004D4F29C0E";

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

fn ok(buf: &[u8]) -> Option<ExchangeMatch> {
    at_final_line_complete(buf, F::Verbose)
}

// --- 1. lines after the final result must not hide it -------------------------------

#[test]
fn pdu_line_of_cmt_after_ok_does_not_hide_it() {
    let raw = format!("AT\r\r\nOK\r\n\r\n+CMT: ,23\r\n{PDU}\r\n");
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw.as_bytes(), "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CMT: ,23", PDU]));
}

#[test]
fn pdu_line_of_cds_after_ok_does_not_hide_it() {
    let raw = format!("AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\n+CDS: 25\r\n{PDU}\r\n");
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
    assert_eq!(p.urc_lines, s(&["+CDS: 25", PDU]));
}

#[test]
fn boot_status_lines_after_ok_do_not_hide_it() {
    let raw = b"AT\r\r\nOK\r\n\r\nRDY\r\n\r\nCall Ready\r\n\r\nSMS Ready\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["RDY", "Call Ready", "SMS Ready"]));
}

#[test]
fn multiline_cusd_after_ok_does_not_hide_it() {
    let raw = b"AT+CUSD=1,\"*100#\",15\r\r\nOK\r\n\r\n+CUSD: 1,\"1. Balance\n2. Tariff\",15\r\n";
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
    let p = parse_at_response(raw, "AT+CUSD=1,\"*100#\",15", &[], F::Verbose);
    assert_eq!(p.urc_lines, s(&["+CUSD: 1,\"1. Balance\n2. Tariff\",15"]));
}

#[test]
fn cmt_before_final_keeps_its_pdu_line() {
    // ATE0: no echo, the URC lands in the middle of the reply.
    let raw = format!("+CMT: ,23\r\n{PDU}\r\n+CSQ: 20,99\r\n\r\nOK\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.urc_lines, s(&["+CMT: ,23", PDU]));
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
}

#[test]
fn text_mode_cds_is_one_line() {
    let raw = b"AT\r\r\nOK\r\n+CDS: 6,12,\"+10000000000\",145,\"24/01/01,10:00:00+00\"\r\n";
    let p = parse_at_response(raw, "AT", &[], F::Verbose);
    assert_eq!(p.urc_lines.len(), 1);
}

#[test]
fn drain_keeps_two_line_urcs_together() {
    let raw = format!("+CMT: ,23\r\n{PDU}\r\n+CDS: 25\r\n{PDU}\r\n+CBM: 88\r\n{PDU}\r\nnoise\r\n");
    assert_eq!(
        urc_lines_from_drain(raw.as_bytes(), &[]),
        s(&["+CMT: ,23", PDU, "+CDS: 25", PDU, "+CBM: 88", PDU])
    );
}

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

#[test]
fn stale_final_before_echo_does_not_complete() {
    assert_eq!(ok(b"OK\r\nAT+CSQ\r\r\n"), None);
}

// --- 2. `OK+CUSD:` glued without CRLF ------------------------------------------------

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

#[test]
fn glued_ok_cmgr_is_split() {
    let lines = split_lines("OK+CMGR: 1,10\r\n");
    assert_eq!(lines, s(&["OK", "+CMGR: 1,10"]));
}

#[test]
fn glued_urc_waits_for_its_terminator() {
    // Only complete lines are judged; the glued URC comes out once its LF arrives.
    assert_eq!(ok(b"OK+CUSD: 0,\"x\",15"), None);
    let mut demux = ExchangeDemux::new("AT", &[]);
    assert!(demux.process_chunk(b"OK+CUSD: 0,\"x\",15").is_empty());
    assert_eq!(demux.process_chunk(b"\r\n"), s(&["+CUSD: 0,\"x\",15"]));
}

// --- 3. quoted +CUSD text with raw CR/LF ---------------------------------------------

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

#[test]
fn demux_keeps_cusd_open_across_chunks() {
    let mut demux = ExchangeDemux::new("AT", &[]);
    assert!(demux.process_chunk(b"+CUSD: 1,\"1. Balance\r\n").is_empty());
    assert_eq!(
        demux.process_chunk(b"2. Tariff\",15\r\n"),
        s(&["+CUSD: 1,\"1. Balance\r\n2. Tariff\",15"])
    );
    // A result code ends an unclosed quote early.
    assert!(demux.process_chunk(b"+CUSD: 0,\"abc\r\n").is_empty());
    assert_eq!(demux.process_chunk(b"OK\r\n"), s(&["+CUSD: 0,\"abc"]));
}

// --- 4. identical URCs are each emitted once ------------------------------------------

#[test]
fn demux_emits_every_identical_urc() {
    let mut demux = ExchangeDemux::new("AT+CSQ", &[]);
    assert_eq!(demux.process_chunk(b"RING\r\n"), s(&["RING"]));
    assert_eq!(demux.process_chunk(b"\r\nRING\r\n"), s(&["RING"]));
    assert_eq!(
        demux.process_chunk(b"+CREG: 1\r\n+CREG: 1\r\n"),
        s(&["+CREG: 1", "+CREG: 1"])
    );
}

#[test]
fn demux_emits_two_line_urc_together() {
    let mut demux = ExchangeDemux::new("AT+CSQ", &[]);
    let chunk = format!("AT+CSQ\r\r\n+CMT: ,23\r\n{PDU}\r\n+CSQ: 20,99\r\nOK\r\n");
    assert_eq!(
        demux.process_chunk(chunk.as_bytes()),
        s(&["+CMT: ,23", PDU])
    );
}

// --- 5. V.250 numeric result codes --------------------------------------------------

#[test]
fn numeric_codes_follow_v250() {
    assert_eq!(classify_numeric_final_line("0"), Some(ExchangeMatch::Ok));
    assert_eq!(classify_numeric_final_line("1"), None); // CONNECT: intermediate
    assert_eq!(classify_numeric_final_line("2"), None); // RING: unsolicited
    assert_eq!(
        classify_numeric_final_line("3"),
        Some(ExchangeMatch::NoCarrier)
    );
    assert_eq!(classify_numeric_final_line("4"), Some(ExchangeMatch::Error));
    assert_eq!(classify_numeric_final_line("5"), None);
    assert_eq!(
        classify_numeric_final_line("6"),
        Some(ExchangeMatch::NoDialtone)
    );
    assert_eq!(classify_numeric_final_line("7"), Some(ExchangeMatch::Busy));
    assert_eq!(
        classify_numeric_final_line("8"),
        Some(ExchangeMatch::NoAnswer)
    );
    assert_eq!(
        at_final_line_complete(b"ATD1;\r\r\n7\r\n", F::Numeric),
        Some(ExchangeMatch::Busy)
    );
}

// --- 6. verbose +CME / +CMS errors ---------------------------------------------------

fn cme(line: &str) -> Option<i32> {
    match classify_final_line(line) {
        Some(ExchangeMatch::CmeError { code }) => code,
        Some(ExchangeMatch::CmsError { code }) => code,
        other => panic!("{line}: {other:?}"),
    }
}

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

#[test]
fn cme_code_with_text() {
    assert_eq!(cme("+CME ERROR: 10: SIM not inserted"), Some(10));
}

#[test]
fn verbose_cms_texts_map_to_codes() {
    assert_eq!(cme("+CMS ERROR: operation not allowed"), Some(302));
    assert_eq!(cme("+CMS ERROR: SIM busy"), Some(314));
    assert_eq!(cme("+CMS ERROR: memory full"), Some(322));
    assert_eq!(cme("+CMS ERROR: 314"), Some(314));
    assert_eq!(cme("+CMS ERROR: raspberry"), None);
}

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

// --- 8. ATE0: SMS text that starts like an extended line is data --------------------

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

#[test]
fn text_mode_cmt_body_belongs_to_urc() {
    let raw =
        "+CMT: \"+10000000000\",,\"24/01/01,10:00:00+00\"\r\n#tag\r\n+CSQ: 20,99\r\n\r\nOK\r\n";
    let p = parse_at_response(raw.as_bytes(), "AT+CSQ", &[], F::Verbose);
    assert_eq!(p.urc_lines.len(), 2);
    assert_eq!(p.urc_lines[1], "#tag");
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
}

// --- 9. result codes in any case -----------------------------------------------------

#[test]
fn result_codes_are_case_insensitive() {
    assert_eq!(classify_final_line("ok"), Some(ExchangeMatch::Ok));
    assert_eq!(
        classify_final_line("No Carrier"),
        Some(ExchangeMatch::NoCarrier)
    );
    assert_eq!(cme("+cme error: 10"), Some(10));
    assert_eq!(ok(b"AT\r\r\nok\r\n"), Some(ExchangeMatch::Ok));
}

#[test]
fn lowercase_ok_sms_body_is_data() {
    let raw = b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nok\r\n";
    assert_eq!(ok(raw), None);
    let raw = b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nok\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CMGR=1", &[], F::Verbose);
    assert_eq!(p.solicited_body.last().map(String::as_str), Some("ok"));
}

// --- ported from the gateway's line processor / stream buffer tests -----------------

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

#[test]
fn lowercase_at_text_is_data_not_echo() {
    let raw =
        b"+CMGR: \"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"\r\nat home\r\nOK\r\n";
    let p = parse_at_response(raw, "AT+CMGR=1", &[], F::Verbose);
    assert_eq!(p.solicited_body.last().map(String::as_str), Some("at home"));
}

#[test]
fn cpin_not_ready_is_command_response() {
    let p = parse_at_response(b"+CPIN: NOT READY\r\nOK\r\n", "AT+CPIN?", &[], F::Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, s(&["+CPIN: NOT READY"]));
    assert!(p.urc_lines.is_empty());
}

#[test]
fn interleaved_echo_and_cmti_in_one_chunk() {
    let raw = b"AT+CMGL=4\r\r\n+CMTI: \"ME\",1\r\nOK\r\n";
    let mut demux = ExchangeDemux::new("AT+CMGL=4", &[]);
    assert_eq!(demux.process_chunk(raw), s(&["+CMTI: \"ME\",1"]));
    assert_eq!(ok(raw), Some(ExchangeMatch::Ok));
}

#[test]
fn random_error_text_then_ok_completes() {
    assert_eq!(ok(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\n"), None);
    assert_eq!(
        ok(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\nOK\r\n"),
        Some(ExchangeMatch::Ok)
    );
}

#[test]
fn garbage_prefix_before_ok_is_drained() {
    assert_eq!(ok(b"\x00\xFFGARBAGE\r\nOK\r\n"), Some(ExchangeMatch::Ok));
}

#[test]
fn unterminated_line_waits_for_completion() {
    assert_eq!(ok(b"+CSQ: 20"), None);
    assert_eq!(ok(b"+CSQ: 20,99\r\nOK"), None);
    assert_eq!(ok(b"+CSQ: 20,99\r\nOK\r\n"), Some(ExchangeMatch::Ok));
    assert_eq!(
        split_lines("+CSQ: 20,99\r\nOK\r\n"),
        s(&["+CSQ: 20,99", "OK"])
    );
}

#[test]
fn oversized_garbage_line_before_ok_drains() {
    let raw = format!("{}\r\nOK\r\n", "G".repeat(4096));
    assert_eq!(ok(raw.as_bytes()), Some(ExchangeMatch::Ok));
}

// --- audit regressions --------------------------------------------------------------

/// V.250 §5.7.1: with `ATV0` a result code is `<code><CR>` (no LF), the echo ends
/// with `S3` too.
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

#[test]
fn basic_commands_before_an_extended_one_are_skipped() {
    use serial_codec::at::derive_solicited_prefixes as d;
    assert_eq!(d("ATE1+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("ATE0V1+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("AT&F+CSQ"), s(&["+CSQ:"]));
    assert_eq!(d("AT&C1S0=0+CREG?;+CSQ"), s(&["+CREG:", "+CSQ:"]));
    assert_eq!(d("ATI"), s(&["I:"]));
    let raw = b"ATE1+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "ATE1+CSQ", &[], F::Verbose);
    assert_eq!(p.solicited_body, s(&["+CSQ: 20,99"]));
    assert!(p.urc_lines.is_empty());
    assert!(ExchangeDemux::new("ATE1+CSQ", &[])
        .process_chunk(raw)
        .is_empty());
}

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
    assert_eq!(im(b"ATD*99#\r\r\nCONNE", F::Verbose), None);
}

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

/// `ATE0`: no echo, but the command is known, so `AT+CLAC` lines are its body.
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

/// A text `+CMT` body that starts with `AT` is SMS data, not an echo.
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

#[test]
fn vendor_final_text_is_not_a_result_code() {
    assert_eq!(classify_final_line("COMMAND NOT SUPPORT"), None);
    // Kept for compatibility (TCP stack finals, not V.250).
    assert_eq!(classify_final_line("SEND OK"), Some(ExchangeMatch::SendOk));
}

#[test]
fn demux_is_linear_and_bounded_on_lf_less_data() {
    let mut d = ExchangeDemux::new("ATD*99#", &[]);
    let t = std::time::Instant::now();
    let chunk = [0x7eu8; 64];
    for _ in 0..(1 << 16) {
        assert!(d.process_chunk(&chunk).is_empty());
    }
    // 4 MiB; the old demux rescanned its whole buffer per chunk (256 KiB took 355 ms).
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
    // The dropped overlong line ends at its LF; the next line is judged normally.
    assert_eq!(d.process_chunk(b"xx\r\n+CREG: 1\r\n"), s(&["+CREG: 1"]));
}

/// The incremental completion check gives the one-shot answer for every prefix of a
/// reply, fed in chunks, and stays linear on a long reply.
#[test]
fn incremental_completion_matches_one_shot_and_is_linear() {
    use serial_codec::at::{at_intermediate_line_complete, AtCompletion};
    let replies: [&[u8]; 6] = [
        b"AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\nRING\r\n",
        b"OK\r\nAT+CSQ\r\r\n+CSQ: 1,2\r\n\r\nERROR\r\n",
        b"AT\r\r\nOK\r\n+CMT: \"+10000000001\",,\"24/01/01,00:00:00+00\"\r\nATM\r\n",
        b"ATD*99#\r\r\nCONNECT 150000000\r\n~\xff}#",
        b"AT+CMGS=20\r\r\n> ",
        b"AT+CUSD=1\r\r\nOK\r\n\r\n+CUSD: 0,\"menu\r\n1. a\r\n2. b\",15\r\n",
    ];
    for raw in replies {
        for intermediate in [false, true] {
            for fmt in [F::Verbose, F::Numeric] {
                for chunk in [1, 3, 64] {
                    let mut c = AtCompletion::new(intermediate, fmt);
                    let mut seen = 0;
                    for part in raw.chunks(chunk) {
                        let got = c.push(part);
                        seen += part.len();
                        let want = if intermediate {
                            at_intermediate_line_complete(&raw[..seen], fmt)
                        } else {
                            at_final_line_complete(&raw[..seen], fmt)
                        };
                        assert_eq!(
                            got, want,
                            "{raw:?} {intermediate} {fmt:?} chunk {chunk} at {seen}"
                        );
                    }
                }
            }
        }
    }
    let mut c = AtCompletion::new(false, F::Verbose);
    let t = std::time::Instant::now();
    assert_eq!(c.push(b"AT+CPBR=1,9999\r\r\n"), None);
    let line = b"+CPBR: 1,\"+10000000001\",145,\"Name\"\r\n".repeat(2);
    for _ in 0..(1 << 13) {
        for part in line.chunks(64) {
            assert_eq!(c.push(part), None);
        }
    }
    assert_eq!(c.push(b"\r\nOK\r\n"), Some(ExchangeMatch::Ok));
    assert!(
        t.elapsed() < std::time::Duration::from_secs(5),
        "{:?}",
        t.elapsed()
    );
}
