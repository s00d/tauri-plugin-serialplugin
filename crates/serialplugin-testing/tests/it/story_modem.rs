//! Stories: an AT modem, its SMS prompt and its multiplexer.

use serialplugin_testing::mux::{self, Frame, Reader, CLD, DISC, DM, PF, SABM, UA, UIH};
use serialplugin_testing::{FakeDevice, Modem, MuxOptions};
use std::collections::HashSet;
use std::io::{Read, Write};

/// Given a modem with scripted replies
/// When the host sends commands whole, split and several at once
/// Then each gets its reply in the modem's format, and the log has every command
#[test]
fn commands_get_their_replies() {
    let mut modem = Modem::new()
        .reply("AT+CPIN?", &["+CPIN: READY"])
        .fails("AT+COPS?", "+CME ERROR: 30")
        .raw("AT+RAW", b"\x01\x02")
        .silent("AT+CFUN=1,1");
    let log = modem.log();
    assert_eq!(modem.feed(b"AT+CP"), b"");
    assert_eq!(modem.feed(b"IN?\r"), b"\r\n+CPIN: READY\r\n\r\nOK\r\n");
    assert_eq!(
        modem.feed(b"AT+COPS?\rAT+RAW\rAT+CFUN=1,1\rAT+NOPE\r"),
        b"\r\n+CME ERROR: 30\r\n\x01\x02\r\nERROR\r\n"
    );
    log.wait_command("AT+NOPE");
    let lines: Vec<String> = log.commands().into_iter().map(|c| c.line).collect();
    assert_eq!(
        lines,
        ["AT+CPIN?", "AT+COPS?", "AT+RAW", "AT+CFUN=1,1", "AT+NOPE"]
    );

    let mut numeric = Modem::new().echo().numeric();
    assert_eq!(numeric.feed(b"AT+CSQ\r"), b"AT+CSQ\r+CSQ: 20,99\r\n0\r");
    assert_eq!(numeric.feed(b"AT+NOPE\r"), b"AT+NOPE\r4\r");
}

/// Given a modem waiting for a PDU
/// When the host drops one with ESC and sends the next with Ctrl-Z
/// Then only the second is kept, and commands work again
#[test]
fn sms_prompt() {
    let mut modem = Modem::new();
    let log = modem.log();
    assert_eq!(modem.feed(b"AT+CMGS=5\r"), b"\r\n> ");
    assert_eq!(modem.feed(b"00FF\x1b"), b"\r\nOK\r\n");
    assert_eq!(modem.feed(b"AT+CMGS=7\r"), b"\r\n> ");
    assert_eq!(modem.feed(b"0011"), b"");
    assert_eq!(modem.feed(b"22\x1a"), b"\r\n+CMGS: 7\r\n\r\nOK\r\n");
    assert_eq!(log.pdus(), ["001122"]);
    assert_eq!(modem.feed(b"AT\r"), b"\r\nOK\r\n");
}

/// Given 27.010 octets as modems send them
/// When they arrive in any split, after noise
/// Then the frames are read whole, and what the fake sends has the same octets
#[test]
fn frames_match_known_octets_and_survive_any_split() {
    assert_eq!(
        mux::frame(0, true, SABM | PF, &[]),
        [0xF9, 0x03, 0x3F, 0x01, 0x1C, 0xF9]
    );
    assert_eq!(
        mux::frame(0, true, UA | PF, &[]),
        [0xF9, 0x03, 0x73, 0x01, 0xD7, 0xF9]
    );
    let long = vec![0x55; 300];
    let stream = [
        b"noise".to_vec(),
        mux::frame(1, true, SABM | PF, &[]),
        mux::data(2, &long),
        mux::control(CLD, &[]),
    ]
    .concat();
    for size in 1..20 {
        let mut reader = Reader::default();
        let got: Vec<Frame> = stream.chunks(size).flat_map(|c| reader.feed(c)).collect();
        let kinds: Vec<_> = got
            .iter()
            .map(|f| (f.dlci, f.kind, f.payload.len()))
            .collect();
        assert_eq!(
            kinds,
            [(1, SABM, 0), (2, UIH, 300), (0, UIH, 2)],
            "chunks of {size}"
        );
    }
}

/// Given a multiplexed modem that ignores one channel and refuses another
/// When the host opens channels, talks AT on one and leaves the multiplexer
/// Then the good channel answers on itself, the bad ones do as told, and after CLD plain AT
/// works again
#[test]
fn multiplexer_session() {
    let modem = Modem::new().mux(MuxOptions {
        deaf: HashSet::from([3]),
        refuse: HashSet::from([2]),
        ..MuxOptions::default()
    });
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    let mut port = device.port();
    let mut reader = Reader::default();
    let mut ask = |bytes: &[u8]| {
        port.write_all(bytes).unwrap();
        let mut reply = vec![0; device.unread()];
        port.read_exact(&mut reply).unwrap();
        reply
    };
    assert_eq!(ask(b"AT+CMUX=0\r"), b"\r\nOK\r\n");
    let mut answers = |bytes: &[u8]| -> Vec<(u8, u8, Vec<u8>)> {
        let reply = ask(bytes);
        let frames = reader.feed(&reply);
        frames
            .into_iter()
            .map(|f| (f.dlci, f.kind, f.payload))
            .collect()
    };
    let sabm = |dlci| mux::frame(dlci, true, SABM | PF, &[]);
    assert_eq!(answers(&sabm(0)), [(0, UA, vec![])]);
    assert_eq!(answers(&sabm(1)), [(1, UA, vec![])]);
    assert_eq!(answers(&sabm(2)), [(2, DM, vec![])]);
    assert_eq!(answers(&sabm(3)), []);
    assert_eq!(
        answers(&mux::frame(1, true, UIH, b"AT+CSQ\r")),
        [(1, UIH, b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n".to_vec()),]
    );
    let csq = log.commands().pop().unwrap();
    assert_eq!((csq.dlci, csq.line.as_str()), (Some(1), "AT+CSQ"));
    assert_eq!(
        answers(&mux::frame(1, true, DISC | PF, &[])),
        [(1, UA, vec![])]
    );
    assert_eq!(
        answers(&mux::control(CLD, &[])),
        [(0, UIH, vec![CLD & !2, 1])]
    );
    log.wait_frame("the DISC", |f| f.dlci == 1 && f.kind == DISC);
    assert!(!log.cld_before_ua());
    assert_eq!(ask(b"AT\r"), b"\r\nOK\r\n");
}
