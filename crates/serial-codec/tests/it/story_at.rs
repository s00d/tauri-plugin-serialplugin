//! Stories: AT sessions with modems that do not behave like the textbook. Phone numbers and
//! PDUs are fake.

use serial_codec::at::*;
use AtResultFormat::{Numeric, Verbose};

fn done(buf: &[u8]) -> Option<ExchangeMatch> {
    at_final_line_complete(buf, Verbose)
}

fn prompted(buf: &[u8]) -> Option<ExchangeMatch> {
    at_intermediate_line_complete(buf, Verbose)
}

fn intermediate(line: &str) -> Option<ExchangeMatch> {
    Some(ExchangeMatch::Intermediate { line: line.into() })
}

/// Given a modem that sends SMS and dials data calls
/// When `AT+CMGS` waits for its prompt, is rejected, or times out with the prompt only, and
/// `ATD` connects with PPP bytes behind `CONNECT` in the same read
/// Then the prompt check completes on `> ` (no line feed), `CONNECT` and the error, never on
/// half a line; the final check only on the error; CMUX flags behind `OK` hide nothing; and a
/// reply without a final result keeps every line with an unknown status
#[test]
fn sms_prompt_and_data_call() {
    assert_eq!(prompted(b"AT+CMGS=20\r\r\n> "), intermediate(">"));
    assert_eq!(prompted(b"AT+CMGS=1\r\r\n>\r\n"), intermediate(">"));
    assert_eq!(done(b"AT+CMGS=20\r\r\n> "), None);
    let p = parse_at_response(b"AT+CMGS=20\r\r\n> ", "AT+CMGS=20", &[], Verbose);
    assert_eq!((p.status, p.final_line), (AtParseStatus::Unknown, None));
    let rejected = b"AT+CMGS=20\r\r\n+CMS ERROR: 304\r\n";
    let error = Some(ExchangeMatch::CmsError { code: Some(304) });
    assert_eq!((prompted(rejected), done(rejected)), (error.clone(), error));

    assert_eq!(prompted(b"ATD*99#\r\r\nCONNE"), None);
    let call = b"ATD*99#\r\r\nCONNECT 150000000\r\n~\xff}#\xc0!}!}!} }";
    assert_eq!(prompted(call), intermediate("CONNECT 150000000"));
    assert_eq!(done(call), None);
    let mux = b"AT+CMUX=0\r\r\nOK\r\n\xF9\xF9\xF9\xF9";
    assert_eq!(
        (prompted(mux), done(mux)),
        (Some(ExchangeMatch::Ok), Some(ExchangeMatch::Ok))
    );

    let timeout = b"AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\n+CMTI: \"SM\",1\r\n";
    let p = parse_at_response(timeout, "AT+CSQ", &[], Verbose);
    assert_eq!((p.status, p.final_line), (AtParseStatus::Unknown, None));
    assert_eq!(p.solicited_body, ["+CSQ: 20,99"]);
    assert_eq!(p.urc_lines, ["+CMTI: \"SM\",1"]);
}

/// Given a modem in `ATV0`, whose result codes and echo end with CR alone (V.250 §5.7.1)
/// When a query succeeds, a command fails, a call connects and one is busy
/// Then CR ends the lines, the codes complete the exchange (`1` only as an intermediate result
/// and only in numeric mode), and a URC glued behind a code is routed live
#[test]
fn a_numeric_mode_session() {
    let done = |b: &[u8]| at_final_line_complete(b, Numeric);
    assert_eq!(done(b"AT\r"), None);
    assert_eq!(done(b"AT\r0\r"), Some(ExchangeMatch::Ok));
    assert_eq!(done(b"AT\r\r\n0\r"), Some(ExchangeMatch::Ok));
    let csq = b"AT+CSQ\r+CSQ: 1,2\r\n0\r";
    assert_eq!(
        split_lines("AT+CSQ\r+CSQ: 1,2\r\n0\r"),
        ["AT+CSQ", "+CSQ: 1,2", "0"]
    );
    let p = parse_at_response(csq, "AT+CSQ", &[], Numeric);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, ["+CSQ: 1,2"]);
    assert_eq!(
        done(b"AT+CSQ\r+CSQ: 1,2\r\n4\r"),
        Some(ExchangeMatch::Error)
    );
    let mut demux = ExchangeDemux::with_format("AT+CSQ", &[], Numeric);
    assert_eq!(
        demux.process_chunk(b"AT+CSQ\r+CSQ: 1,2\r\n0\r+CREG: 1\r\n"),
        ["+CREG: 1"]
    );
    assert_eq!(demux.phase(), ExchangePhase::CollectingSolicited);

    let dial = b"ATD\r\r\n1\r\n";
    assert_eq!(
        at_intermediate_line_complete(dial, Numeric),
        intermediate("1")
    );
    assert_eq!((prompted(dial), done(dial)), (None, None));
    assert_eq!(done(b"ATD1;\r\r\n7\r\n"), Some(ExchangeMatch::Busy));
}

/// Given commands with V.250 basic commands in front of or instead of extended ones
/// When the headers of their replies are derived and merged with the caller's
/// Then `E1`, `&F`, `S0=0`, `S3?` are skipped, `D` keeps its dial string, a malformed basic
/// command keeps its own name, text that is no command has none; and the reply of such a
/// command is its body, not a URC
#[test]
fn a_command_names_its_own_reply_lines() {
    for (cmd, want) in [
        ("ATE1+CSQ", &["+CSQ:"][..]),
        ("ATE0V1+CSQ", &["+CSQ:"]),
        ("AT&F+CSQ", &["+CSQ:"]),
        ("AT&C1S0=0+CREG?;+CSQ", &["+CREG:", "+CSQ:"]),
        ("ATS3?+CSQ\r\n", &["+CSQ:"]),
        ("ATI", &["I:"]),
        ("ATD*99#", &["D:"]),
        ("ATS?+CSQ", &["S:"]),
        ("AT+CSQ;+csq", &["+CSQ:"]),
        ("AT&", &[]),
        ("hello", &[]),
    ] {
        assert_eq!(derive_solicited_prefixes(cmd), want, "{cmd}");
    }
    let user = [" +CSQ: ".to_string(), "+X:".into(), " ".into()];
    assert_eq!(merge_solicited_prefixes("AT+CSQ", &user), ["+CSQ:", "+X:"]);

    let raw = b"ATE1+CSQ\r\r\n+CSQ: 20,99\r\n+X: 1\r\n\r\nOK\r\n";
    let p = parse_at_response(raw, "ATE1+CSQ", &user, Verbose);
    assert_eq!(p.solicited_body, ["+CSQ: 20,99", "+X: 1"]);
    assert!(p.urc_lines.is_empty());
    assert!(ExchangeDemux::new("ATE1+CSQ", &user)
        .process_chunk(raw)
        .is_empty());
}

/// Given errors as numbers, as `AT+CMEE=2` texts in any spelling, as both, and as vendor texts
/// When they end an exchange
/// Then known texts give their 27.007 / 27.005 code, unknown texts none, vendor texts end
/// nothing, and the JSON the frontend sees is `{cmeError: {code}}`
#[test]
fn errors_in_every_form() {
    for (detail, cme, cms) in [
        ("14", Some(14), Some(14)),
        ("10: SIM not inserted", Some(10), Some(10)),
        ("SIM busy", Some(14), Some(314)),
        ("\"sim-busy\"", Some(14), Some(314)),
        ("sim pin required", Some(11), Some(311)),
        ("operation not allowed", Some(3), Some(302)),
        ("memory full", Some(20), Some(322)),
        ("raspberry", None, None),
    ] {
        for (line, code, want) in [
            (format!("+CME ERROR: {detail}"), cme, "cmeError"),
            (format!("+CMS ERROR: {detail}"), cms, "cmsError"),
        ] {
            let raw = format!("AT+X\r\r\n{line}\r\n");
            let m = done(raw.as_bytes()).unwrap();
            let json = serde_json::json!({ want: { "code": code } });
            assert_eq!(serde_json::to_value(&m).unwrap(), json, "{line}");
            assert_eq!(serde_json::from_value::<ExchangeMatch>(json).unwrap(), m);
            let p = parse_at_response(raw.as_bytes(), "AT+X", &[], Verbose);
            assert_eq!(p.status, m.to_parse_status());
        }
    }
    assert_eq!(
        done(b"AT+CIPSEND\r\r\nSEND OK\r\n"),
        Some(ExchangeMatch::SendOk)
    );
    assert_eq!(classify_final_line("COMMAND NOT SUPPORT"), None);
    let failed = AtCommandResult::failed("AT".into(), "port closed".into());
    assert_eq!(
        (failed.status, failed.matched, failed.response.as_str()),
        (AtParseStatus::Error, ExchangeMatch::Error, "port closed")
    );
}

/// Given a port with line noise, a modem that just booted and results of earlier commands
/// When an exchange runs over them
/// Then noise, a 4 KiB line and vendor text before `OK` do not stop it; boot banners after it
/// are URCs; `NO CARRIER` before the echo is a URC and a stale `OK` is body; a new echo makes an
/// earlier result stale, and a result before the echo only leaves the reply open
#[test]
fn noise_boot_banners_and_stale_results() {
    assert_eq!(done(b"\x00\xFFGARBAGE\r\nOK\r\n"), Some(ExchangeMatch::Ok));
    let long = format!("{}\r\nOK\r\n", "G".repeat(4096));
    assert_eq!(done(long.as_bytes()), Some(ExchangeMatch::Ok));
    assert_eq!(done(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\n"), None);
    assert_eq!(
        done(b"AT+TEST\r\r\nSOME RANDOM ERROR TEXT\r\nOK\r\n"),
        Some(ExchangeMatch::Ok)
    );

    let boot = b"AT\r\r\nOK\r\n\r\nRDY\r\n\r\nCall Ready\r\n\r\nSMS Ready\r\n";
    assert_eq!(done(boot), Some(ExchangeMatch::Ok));
    let p = parse_at_response(boot, "AT", &[], Verbose);
    assert_eq!(p.urc_lines, ["RDY", "Call Ready", "SMS Ready"]);

    let p = parse_at_response(b"NO CARRIER\r\nATD1;\r\nOK\r\n", "ATD1;", &[], Verbose);
    assert_eq!(
        (p.status, p.final_line.as_deref()),
        (AtParseStatus::Ok, Some("OK"))
    );
    assert_eq!(p.urc_lines, ["NO CARRIER"]);
    let p = parse_at_response(b"OK\r\nAT\r\nOK\r\n", "AT", &[], Verbose);
    assert_eq!(
        (p.status, p.solicited_body),
        (AtParseStatus::Ok, vec!["OK".to_string()])
    );
    let open = b"NO CARRIER\r\nATD1;\r\n";
    assert_eq!(done(open), None);
    let p = parse_at_response(open, "ATD1;", &[], Verbose);
    assert_eq!((p.status, p.final_line), (AtParseStatus::Unknown, None));
    assert_eq!(p.urc_lines, ["NO CARRIER"]);
    assert_eq!(done(b"OK\r\nat+csq\r\r\n"), None);
}
