//! Stories: an AT modem, its SMS prompt and its multiplexer.

use serialplugin_testing::mux::{self, Frame, Reader, CLD, DISC, DM, PF, SABM, UA, UIH};
use serialplugin_testing::{FakeDevice, Modem, MuxOptions};
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
    assert_eq!(log.wait_command("at+raw").line, "AT+RAW");
    let lines: Vec<String> = log.commands().into_iter().map(|c| c.line).collect();
    assert_eq!(
        lines,
        ["AT+CPIN?", "AT+COPS?", "AT+RAW", "AT+CFUN=1,1", "AT+NOPE"]
    );

    // Any case (V.250); the log has the line as it came.
    let mut lower = Modem::new().reply("at+gmm", &["M26"]);
    assert_eq!(lower.feed(b"At+GmM\r"), b"\r\nM26\r\n\r\nOK\r\n");
    assert_eq!(lower.feed(b"at+cmgs=1\r"), b"\r\n> ");
    assert_eq!(lower.log().commands()[0].line, "At+GmM");

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
    let options = MuxOptions::default().deaf([3]).refuse([2]);
    let modem = Modem::new().mux(options);
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

/// Given a multiplexed modem
/// When the host sends data or DISC on channels it never opened, that were refused, or that
/// it closed, and opens them again
/// Then the modem answers DM, reads no command there, and an opened one works
#[test]
fn only_an_established_channel_carries_data() {
    let mut modem = Modem::new().mux(MuxOptions::default().refuse([2]));
    let log = modem.log();
    let sabm = |dlci| mux::frame(dlci, true, SABM | PF, &[]);
    let disc = |dlci| mux::frame(dlci, true, DISC | PF, &[]);
    let at = |dlci| mux::frame(dlci, true, UIH, b"AT\r");
    let ua = |dlci| mux::frame(dlci, true, UA | PF, &[]);
    let dm = |dlci| mux::frame(dlci, true, DM | PF, &[]);
    modem.feed(b"AT+CMUX=0\r");
    // Nothing before the control channel, not even CLD.
    let cld = mux::frame(0, true, UIH, &[CLD, 1]);
    assert_eq!(modem.feed(&cld), dm(0));
    assert_eq!(modem.feed(&sabm(0)), ua(0));
    assert_eq!(modem.feed(&at(5)), dm(5));
    assert_eq!(modem.feed(&disc(5)), dm(5));
    assert_eq!(modem.feed(&sabm(2)), dm(2));
    assert_eq!(modem.feed(&at(2)), dm(2));
    assert_eq!(modem.feed(&sabm(1)), ua(1));
    assert_eq!(modem.feed(&at(1)), mux::data(1, b"\r\nOK\r\n"));
    assert_eq!(modem.feed(&disc(1)), ua(1));
    assert_eq!(modem.feed(&at(1)), dm(1));
    assert_eq!(modem.feed(&disc(1)), dm(1));
    let lines = log.commands();
    assert_eq!(lines.len(), 2, "{lines:?}");
    // What the host answers to the modem's own frames is no request.
    assert_eq!(modem.feed(&mux::frame(1, false, UA | PF, &[])), b"");
    assert_eq!(
        log.frames().pop(),
        Some(Frame::new(1, false, UA, &[])),
        "the log has it"
    );
}

/// Given a modem whose host sends the first frames together with AT+CMUX, and text together
/// with the frame that ends the multiplexer (CLD, or DISC on DLCI 0)
/// When it starts and ends that way
/// Then frames are frames and text is text, the modem is plain again, and a new session
/// starts with no channel open
#[test]
fn the_multiplexer_starts_and_ends_in_the_middle_of_a_write() {
    let mut modem = Modem::new().mux(MuxOptions::default());
    let frame = |dlci, kind| mux::frame(dlci, true, kind | PF, &[]);
    for end in [
        (
            mux::frame(0, true, UIH, &[CLD, 1]),
            mux::control(CLD & !2, &[]),
        ),
        (frame(0, DISC), frame(0, UA)),
    ] {
        let start = [&b"AT+CMUX=0\r"[..], &frame(0, SABM), &frame(1, SABM)].concat();
        let up = [&b"\r\nOK\r\n"[..], &frame(0, UA), &frame(1, UA)].concat();
        assert_eq!(modem.feed(&start), up);
        // The responder sends C/R 0 (27.010 5.4.3.1), the response has none in its type.
        assert_eq!(end.1[1] & 2, if end.1[2] == UIH { 0 } else { 2 });
        let down = [&end.0[..], b"AT+CSQ\r"].concat();
        let plain = [&end.1[..], b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n"].concat();
        assert_eq!(modem.feed(&down), plain);
    }
}

/// Given two channels
/// When both ask to send an SMS and send their PDUs in turns, and one is closed at its prompt
/// Then each PDU is its channel's, and the closed channel opens without a prompt
#[test]
fn each_channel_has_its_own_prompt() {
    let mut modem = Modem::new().mux(MuxOptions::default());
    let log = modem.log();
    modem.feed(b"AT+CMUX=0\r");
    for dlci in 0..3 {
        modem.feed(&mux::frame(dlci, true, SABM | PF, &[]));
    }
    let mut say = |dlci, text: &[u8]| modem.feed(&mux::frame(dlci, true, UIH, text));
    let prompt = |dlci| mux::data(dlci, b"\r\n> ");
    let sent = |dlci| mux::data(dlci, b"\r\n+CMGS: 7\r\n\r\nOK\r\n");
    assert_eq!(say(1, b"AT+CMGS=2\r"), prompt(1));
    assert_eq!(say(2, b"AT+CMGS=2\r"), prompt(2));
    assert_eq!(say(1, b"11"), b"");
    assert_eq!(say(2, b"22\x1a"), sent(2));
    assert_eq!(say(2, b"AT\r"), mux::data(2, b"\r\nOK\r\n"));
    assert_eq!(say(1, b"\x1a"), sent(1));
    assert_eq!(log.pdus(), ["22", "11"]);

    assert_eq!(say(1, b"AT+CMGS=2\r"), prompt(1));
    modem.feed(&mux::frame(1, true, DISC | PF, &[]));
    modem.feed(&mux::frame(1, true, SABM | PF, &[]));
    let ok = modem.feed(&mux::frame(1, true, UIH, b"AT\r"));
    assert_eq!(ok, mux::data(1, b"\r\nOK\r\n"));
}
