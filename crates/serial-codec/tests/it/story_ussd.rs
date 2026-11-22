//! Stories: USSD sessions from the bytes on the port to the text on the screen.

use serial_codec::at::*;
use serial_codec::sms::hex_decode;
use serial_codec::ussd::{self, parse_cusd, Cscs, CusdStatus};
use AtResultFormat::{Numeric, Verbose};

const CMD: &str = "AT+CUSD=1,\"*100#\",15";

/// Given a modem that glues `OK` and `+CUSD:` together and sends menus with raw CR, LF and CRLF
/// When the user dials `*100#`, answers the menu and the network ends the session
/// Then the menu stays one line through the splitter, the parser and the live demux (nothing
/// comes out before its line end), `OK` ends the exchange, and each `+CUSD` gives its status
/// and its text: as sent, from UCS2 hex, from a UCS2 terminal, and from the packed septets some
/// firmware sends (only on request)
#[test]
fn a_menu_session() {
    let menu = "+CUSD: 1,\"1. Balance\n2. Tariff\r\n3. Exit\",15";
    let raw = format!("{CMD}\r\r\nOK{menu}\r\n");
    assert_eq!(split_lines(&format!("OK{menu}\r\n")), ["OK", menu]);
    assert_eq!(split_lines(&format!("{menu}\r\nOK\r\n")), [menu, "OK"]);
    assert_eq!(split_lines("OK+CMGR: 1,10\r\n"), ["OK", "+CMGR: 1,10"]);

    let mut demux = ExchangeDemux::new("AT+CSQ", &[]);
    let (head, end) = raw.split_at(raw.len() - 2);
    for byte in head.as_bytes().chunks(1) {
        assert!(demux.process_chunk(byte).is_empty());
    }
    assert_eq!(
        at_final_line_complete(b"OK+CUSD: 0,\"x\",15", Verbose),
        None
    );
    assert_eq!(demux.process_chunk(end.as_bytes()), [menu]);
    assert_eq!(
        at_final_line_complete(raw.as_bytes(), Verbose),
        Some(ExchangeMatch::Ok)
    );
    let p = parse_at_response(raw.as_bytes(), CMD, &[], Verbose);
    assert_eq!(
        (p.status, &p.urc_lines[..]),
        (AtParseStatus::Ok, &[menu.to_string()][..])
    );

    let c = parse_cusd(menu).unwrap();
    assert_eq!((c.status, c.dcs), (CusdStatus::ActionRequired, Some(15)));
    assert!(c.status.session_active());
    assert_eq!(
        c.text().as_deref(),
        Some("1. Balance\n2. Tariff\r\n3. Exit")
    );

    let msisdn = "\r\n+CUSD: 2,\"MSISDN:\r10000000000\",15\r\n\r\nOK\r\n";
    assert_eq!(
        split_lines(msisdn),
        ["+CUSD: 2,\"MSISDN:\r10000000000\",15", "OK"]
    );
    for (line, text) in [
        (
            r#"+CUSD: 0,"041F0440043804320435044200210020",72"#,
            "Привет! ",
        ),
        (r#"+CUSD: 0,"He said "hi", ok",15"#, r#"He said "hi", ok"#),
        (
            r#"+cusd: 1,"Menu: 1, 2, \"three\"",15OK"#,
            r#"Menu: 1, 2, "three""#,
        ),
        ("+CUSD: 2,Balance 5,00 EUR,15 OK", "Balance 5,00 EUR"),
        // Looks like hex, but a GSM 7-bit DCS is text in the terminal's character set.
        (r#"+CUSD: 0,"AA58ACA6AA8D1A",15"#, "AA58ACA6AA8D1A"),
    ] {
        assert_eq!(parse_cusd(line).unwrap().text().as_deref(), Some(text));
    }
    let packed = parse_cusd(r#"+CUSD: 0,"AA58ACA6AA8D1A",15"#).unwrap();
    assert_eq!(packed.text_packed_hex().as_deref(), Some("*115*5#"));
    let terminal = parse_cusd(r#"+CUSD: 0,"00310032",15"#).unwrap();
    assert_eq!(terminal.text_as(Cscs::Ucs2).as_deref(), Some("12"));
    // A language in front of the text: two characters and CR, or packed in front of UCS2.
    let t = ussd::decode(&ussd::encode("en\rHi").unwrap(), 0x10).unwrap();
    assert_eq!((t.language.as_deref(), t.text.as_str()), (Some("en"), "Hi"));
    let t = ussd::decode(&hex_decode("61390041").unwrap(), 0x11).unwrap();
    assert_eq!((t.language.as_deref(), t.text.as_str()), (Some("ar"), "A"));

    let bye = parse_cusd("+CUSD: 2 OK").unwrap();
    assert_eq!((bye.status, bye.raw), (CusdStatus::Terminated, None));
}

/// Given a modem that truncates a `+CUSD` text, so its quote never closes
/// When a result code (verbose, or numeric with CR line ends), another URC, 2 KiB of text, or
/// nothing follows
/// Then the quote ends there: the result completes the exchange, the URC is not held back and
/// the truncated line is delivered; a quote that closes exactly at the 2048-byte span is one
/// line, one byte later it is cut; an open quote of exactly the demux limit loses nothing; and
/// text glued to a `+` is cut only between a final result and a URC
#[test]
fn menus_survive_a_bad_line() {
    let cut = "+CUSD: 0,\"abc";
    assert_eq!(split_lines(&format!("{cut}\nxyz")), [cut, "xyz"]);
    assert_eq!(split_lines(&format!("{cut}\r\nOK\r\n")), [cut, "OK"]);
    let urc = format!("{cut}\r\n+CSQ: 20,99\r\n");
    assert_eq!(split_lines(&urc), [cut, "+CSQ: 20,99"]);
    let raw = format!("AT+CUSD=1\r\r\n{cut}\r\nOK\r\n");
    assert_eq!(
        at_final_line_complete(raw.as_bytes(), Verbose),
        Some(ExchangeMatch::Ok)
    );

    let mut demux = ExchangeDemux::new("AT", &[]);
    assert!(demux
        .process_chunk(format!("{cut}\r\n").as_bytes())
        .is_empty());
    assert_eq!(demux.process_chunk(b"OK\r\n"), [cut]);
    let mut demux = ExchangeDemux::with_format("AT", &[], Numeric);
    assert!(demux.process_chunk(b"+CUSD: 1,\"menu\r").is_empty());
    assert_eq!(demux.process_chunk(b"0\r"), ["+CUSD: 1,\"menu"]);
    assert_eq!(demux.process_chunk(b"RING\r"), ["RING"]);

    let head = "+CUSD: 0,\"";
    let long = format!("{head}{}\r\n", "A\r\n".repeat(2048));
    assert_eq!(split_lines(&long)[0], "+CUSD: 0,\"A");
    let span = |n: usize| format!("{head}\n{}\"", "a".repeat(n - head.len() - 2));
    assert_eq!(split_lines(&span(2048)), [span(2048)]);
    let over = span(2049);
    assert_eq!(split_lines(&over), [head, &over[head.len() + 1..]]);

    let open = "+CUSD: 1,\"ab\r\n";
    let rest = "c".repeat(MAX_DEMUX_LINE - open.len());
    let mut demux = ExchangeDemux::new("AT", &[]);
    let read = format!("RING\r\n{open}{rest}");
    assert_eq!(demux.process_chunk(read.as_bytes()), ["RING"]);
    assert_eq!(demux.process_chunk(b"\"\r\n"), ["+CUSD: 1,\"ab"]);

    assert_eq!(split_lines("abc+CMTI: 1"), ["abc+CMTI: 1"]);
    assert_eq!(split_lines("OK+7 900"), ["OK+7 900"]);

    let c = parse_cusd(r#"+CUSD: 0,"1234 OK"#).unwrap();
    assert_eq!(c.raw.as_deref(), Some("1234 OK"));
    let c = parse_cusd(r#"+CUSD: 0,"Hi",300"#).unwrap();
    assert_eq!((c.dcs, c.invalid_dcs.as_deref()), (None, Some("300")));
    assert_eq!(c.text().as_deref(), Some("Hi"));
}
