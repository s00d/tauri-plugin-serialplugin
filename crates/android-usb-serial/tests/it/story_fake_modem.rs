//! Story: the scripted AT responder of `FakeTransport`, used by downstream harnesses.

use super::common::*;

/// Given a CDC port whose fake device answers AT commands
/// When known commands are written split across writes, then an unknown one, then any command
/// after the responder is switched off
/// Then the answers come in order (a `+CREG` URC in front of `+CSQ`), `ERROR` for the unknown
/// one, and nothing once it is off
#[test]
fn the_scripted_modem_answers() {
    let fake = FakeTransport::cdc_single_iface();
    fake.enable_at_modem();
    let mut port = open_port(shared(&fake), 0).unwrap();
    let received = || {
        let mut bulk = fake.open_bulk_in(0x81, 64).unwrap();
        let mut out = vec![];
        while let Ok(ReadOutcome::Data(d)) = bulk.read(&mut [0; 256], 0) {
            out.extend(d);
        }
        String::from_utf8(out).unwrap()
    };
    port.write(b"AT\rAT+C").unwrap();
    port.write(b"SQ\r").unwrap();
    assert_eq!(
        received(),
        "\r\nOK\r\n\r\n+CREG: 0,1\r\n\r\n+CSQ: 20,99\r\n\r\nOK\r\n"
    );
    port.write(b"AT+FOO\r").unwrap();
    assert_eq!(received(), "\r\nERROR\r\n");
    fake.disable_at_modem();
    port.write(b"AT\r").unwrap();
    assert_eq!(received(), "");
}
