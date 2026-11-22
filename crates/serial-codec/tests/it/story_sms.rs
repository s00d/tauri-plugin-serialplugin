//! Stories: SMS from the lines on the port to the text. Numbers and PDUs are fake.

use serial_codec::at::*;
use serial_codec::sms::concat::{Fragment, Key, Kind, Reassembler, DEFAULT_MAX_PENDING};
use serial_codec::sms::gsm7::Tables;
use serial_codec::sms::pdu::{decode_hex, encode_submit, Alphabet, PduError, SubmitRequest, Tpdu};
use serial_codec::urc::{parse_any, CpmsStatus, MessageHeader, MessageStatus, SmsMode, Urc};
use AtResultFormat::{Numeric, Verbose};

/// SMS-DELIVER, `TEST`-sized text from a fake number.
const PDU: &str = "07910000000000F0040B910000000000F000004210101000000004D4F29C0E";

/// Given a modem that reports new messages on its own (`+CMT`, `+CDS`, `+CBM` with the data on
/// the next line)
/// When they arrive after `OK`, in the middle of a reply without echo, with text that looks like
/// something else, and while the port is drained
/// Then header and data stay together as URCs, the reply and its result are untouched, the
/// header parses and its PDU decodes; a text-mode status report has no data line
#[test]
fn messages_arrive_while_commands_run() {
    let after = format!(
        "AT+CSQ\r\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\n+CMT: ,23\r\n{PDU}\r\n+CDS: 25\r\n{PDU}\r\n"
    );
    let p = parse_at_response(after.as_bytes(), "AT+CSQ", &[], Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(p.solicited_body, ["+CSQ: 20,99"]);
    assert_eq!(p.urc_lines, ["+CMT: ,23", PDU, "+CDS: 25", PDU]);
    assert_eq!(
        at_final_line_complete(after.as_bytes(), Verbose),
        Some(ExchangeMatch::Ok)
    );
    let Some(Urc::Cmt(cmt)) = parse_any(&p.urc_lines[0]) else {
        panic!("{:?}", p.urc_lines)
    };
    assert_eq!((cmt.mode, cmt.length), (SmsMode::Pdu, Some(23)));
    let pdu = decode_hex(&p.urc_lines[1]).unwrap();
    assert!(matches!(pdu.tpdu, Tpdu::Deliver(_)));
    assert_eq!(pdu.tpdu.address().to_string(), "+00000000000");

    let text = "+CMT: \"+10000000000\",,\"24/01/01,10:00:00+00\"";
    let inside = format!("{text}\r\n#tag\r\n+CMT: ,23\r\n{PDU}\r\n+CSQ: 20,99\r\n\r\nOK\r\n");
    let p = parse_at_response(inside.as_bytes(), "AT+CSQ", &[], Verbose);
    assert_eq!(p.solicited_body, ["+CSQ: 20,99"]);
    assert_eq!(p.urc_lines, [text, "#tag", "+CMT: ,23", PDU]);
    // The text is no echo of a new command, so the result before it stands.
    let atm = format!("AT\r\r\nOK\r\n{text}\r\nATM closed\r\n");
    assert_eq!(
        at_final_line_complete(atm.as_bytes(), Verbose),
        Some(ExchangeMatch::Ok)
    );

    let drained =
        format!("+CMT: ,23\r\n{PDU}\r\n+CDS: 25\r\n{PDU}\r\n+CBM: 88\r\n{PDU}\r\nnoise\r\n");
    assert_eq!(
        urc_lines_from_drain(drained.as_bytes(), &[]),
        ["+CMT: ,23", PDU, "+CDS: 25", PDU, "+CBM: 88", PDU]
    );
    for header in ["+CDS: 6,1", "+CDS:"] {
        let drained = format!("{header}\r\nhello\r\n");
        assert_eq!(urc_lines_from_drain(drained.as_bytes(), &[]), [header]);
    }
    let report =
        "+CDS: 6,12,\"+10000000000\",145,\"24/01/01,10:00:00+00\",\"24/01/01,10:00:09+00\",0";
    let raw = format!("AT\r\r\nOK\r\n{report}\r\n");
    let p = parse_at_response(raw.as_bytes(), "AT", &[], Verbose);
    assert_eq!(p.urc_lines, [report]);
    assert!(matches!(parse_any(report), Some(Urc::Cds(c)) if c.st == Some(0)));
}

/// Given a SIM with stored messages, in PDU and in text mode, whose texts look like URCs, result
/// codes, echoes and digits
/// When the inbox is listed and single messages are read, with and without echo
/// Then headers and texts are the reply (a real `+CMTI` between them is not), an `ok` or `5` in
/// a text ends nothing, each header parses (a status report line does not), and the storage
/// counters read back
#[test]
fn reading_the_inbox() {
    let list = format!("+CMGL: 1,1,,26\r\n{PDU}\r\n+CMGL: 2,1,,26\r\n{PDU}\r\n\r\nOK\r\n");
    let p = parse_at_response(list.as_bytes(), "AT+CMGL=4", &[], Verbose);
    assert_eq!(p.status, AtParseStatus::Ok);
    assert_eq!(
        p.solicited_body,
        ["+CMGL: 1,1,,26", PDU, "+CMGL: 2,1,,26", PDU]
    );
    assert!(p.urc_lines.is_empty());
    let h = MessageHeader::parse_cmgl(&p.solicited_body[2]).unwrap();
    assert_eq!(
        (h.index, h.stat, h.mode),
        (Some(2), MessageStatus::ReceivedRead, SmsMode::Pdu)
    );

    let sms = "\"REC READ\",\"+10000000000\",,\"24/01/01,10:00:00+00\"";
    let list = format!(
        "+CMGL: 1,{sms}\r\ncall me\r\n+7 900 000 00 00\r\n#tag\r\n+CMTI: \"SM\",2\r\n+CMGL: 2,{sms}\r\n*100#\r\n\r\nOK\r\n"
    );
    let p = parse_at_response(list.as_bytes(), "AT+CMGL=\"ALL\"", &[], Verbose);
    assert_eq!(p.urc_lines, ["+CMTI: \"SM\",2"]);
    assert_eq!(p.solicited_body.len(), 6);

    for text in [
        "*100#",
        "+7 900 000 00 00",
        "#tag",
        "$5 off",
        "%done",
        "ok",
        "5",
        "at home",
    ] {
        let open = format!("+CMGR: {sms}\r\n{text}\r\n");
        assert_eq!(
            at_final_line_complete(open.as_bytes(), Verbose),
            None,
            "{text}"
        );
        // `5` is no V.250 code, so it is data in numeric mode too.
        assert_eq!(
            at_final_line_complete(open.as_bytes(), Numeric),
            None,
            "{text}"
        );
        let raw = format!("{open}\r\nOK\r\n");
        let p = parse_at_response(raw.as_bytes(), "AT+CMGR=1", &[], Verbose);
        assert_eq!(p.status, AtParseStatus::Ok, "{text}");
        assert_eq!(p.solicited_body.last().map(String::as_str), Some(text));
        assert!(p.urc_lines.is_empty(), "{text}");
        let mut demux = ExchangeDemux::new("AT+CMGR=1", &[]);
        assert!(demux.process_chunk(raw.as_bytes()).is_empty(), "{text}");
    }
    // Without a command, only the header and its first line are taken for a URC.
    let header = format!("+CMGR: {sms}");
    let drained = format!("{header}\r\nhello\r\n+7 900 123\r\n");
    assert_eq!(
        urc_lines_from_drain(drained.as_bytes(), &[]),
        [header.as_str(), "hello"]
    );

    let report = "6,12,\"+10000000001\",145,\"24/01/02,03:04:05+08\",\"24/01/02,03:04:09+08\",0";
    assert_eq!(
        MessageHeader::parse_cmgr(&format!("+CMGR: \"REC READ\",{report}")),
        None
    );
    assert_eq!(
        MessageHeader::parse_cmgl(&format!("+CMGL: 3,\"REC READ\",{report}")),
        None
    );

    let counts = CpmsStatus::parse("+CPMS: \"SM\",2,30,\"ME\",0,100").unwrap();
    let counts: Vec<_> = counts
        .storages
        .iter()
        .map(|s| (s.mem.as_deref(), s.used, s.total))
        .collect();
    assert_eq!(counts, [(Some("SM"), 2, 30), (Some("ME"), 0, 100)]);
}

/// Given a text with an emoji, too long for one SMS
/// When it is sent without and with a concatenation reference, and its parts arrive reversed,
/// one twice, between parts of another sender with the same reference
/// Then sending without a reference is refused, the parts come out as `AT+CMGS` needs them, and
/// the message completes once, with its text, when the last part is in
#[test]
fn a_long_message_goes_out_and_comes_back() {
    let text = "Привет, это длинное сообщение 😁. ".repeat(6);
    let mut req = SubmitRequest::new("+10000000001", text.clone());
    assert_eq!(encode_submit(&req), Err(PduError::MissingConcatRef));
    req.concat_ref = Some(7);
    let parts = encode_submit(&req).unwrap();
    assert_eq!(parts.len(), 4);
    let mut other = SubmitRequest::new("+10000000002", text.clone());
    other.concat_ref = Some(7);
    let stranger = encode_submit(&other).unwrap();

    let mut asm = Reassembler::new();
    let mut order: Vec<&(usize, String)> = parts.iter().rev().collect();
    order.insert(1, &parts[3]);
    order.insert(2, &stranger[0]);
    let mut done = Vec::new();
    for (len, hex) in order {
        assert_eq!(hex.len() / 2, len + 1, "the SMSC octet is not counted");
        done.extend(asm.push_tpdu(&decode_hex(hex).unwrap().tpdu, 1));
    }
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].text.as_deref(), Some(text.as_str()));
    assert_eq!(done[0].parts, 4);
    assert_eq!(asm.pending(), 1);
    let expired = asm.expire_before(2);
    assert_eq!(expired.len(), 1);
    assert_eq!(
        (expired[0].kind, expired[0].address.as_str()),
        (Kind::Submit, "+10000000002")
    );
}

/// Given a sender that starts 10 000 long messages and never finishes one
/// When their first parts arrive
/// Then the default reassembler never holds more than its cap
#[test]
fn a_flood_of_long_messages_is_bounded() {
    let mut asm = Reassembler::new();
    for reference in 0..10_000u16 {
        let key = Key {
            kind: Kind::Deliver,
            address: "+1".into(),
            reference,
            ref_16bit: true,
            total: 255,
        };
        let part = Fragment {
            alphabet: Alphabet::Gsm7,
            tables: Tables::DEFAULT,
            payload: vec![0x41; 153],
        };
        assert_eq!(asm.push(key, 1, part, u64::from(reference)), None);
        assert!(asm.pending() <= DEFAULT_MAX_PENDING);
    }
    assert_eq!(asm.pending(), DEFAULT_MAX_PENDING);
}
