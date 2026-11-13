//! `FakeTransport` scripted AT responder stories (used by downstream harnesses).

use super::common::*;

fn rx_text(fake: &FakeTransport) -> String {
    let mut bulk = fake.open_bulk_in(0x81, 64).unwrap();
    let mut out = vec![];
    let mut buf = [0u8; 256];
    while let Ok(ReadOutcome::Data(d)) = bulk.read(&mut buf, 0) {
        out.extend(d);
    }
    String::from_utf8(out).unwrap()
}

/// Given the AT responder enabled on a CDC port
/// When `AT` and `AT+CSQ` are written (split across writes)
/// Then RX holds OK, then the +CREG URC and the +CSQ answer.
#[test]
fn at_responder_answers_known_commands() {
    let fake = FakeTransport::cdc_single_iface();
    fake.enable_at_modem();
    let mut port = open_port(shared(&fake), 0).unwrap();
    port.write(b"AT\rAT+C").unwrap();
    port.write(b"SQ\r").unwrap();
    assert_eq!(
        rx_text(&fake),
        "\r\nOK\r\n\r\n+CREG: 0,1\r\n\r\n+CSQ: 20,99\r\n\r\nOK\r\n"
    );
}

/// Given the AT responder enabled, then disabled
/// When an unknown AT command is written, and later any command
/// Then the first gets ERROR and nothing answers after disable.
#[test]
fn at_responder_rejects_unknown_commands_until_disabled() {
    let fake = FakeTransport::cdc_single_iface();
    fake.enable_at_modem();
    let mut port = open_port(shared(&fake), 0).unwrap();
    port.write(b"AT+FOO\r").unwrap();
    assert_eq!(rx_text(&fake), "\r\nERROR\r\n");
    fake.disable_at_modem();
    port.write(b"AT\r").unwrap();
    assert_eq!(rx_text(&fake), "");
}
