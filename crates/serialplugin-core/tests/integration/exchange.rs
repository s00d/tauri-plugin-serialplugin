//! Stories: exchange / AT details the manager model does not drive.

use crate::common::*;

/// Given an exchange waiter whose reply completed,
/// when a failure arrives after it,
/// then wait returns the reply.
#[test]
fn exchange_waiter_first_finish_wins() {
    use serialplugin_core::hub::ExchangeWaiter;
    let options = ExchangeOptions {
        command: Some("AT".into()),
        ..Default::default()
    }
    .resolve();
    let waiter = ExchangeWaiter::new(options, Arc::new(AtomicBool::new(false)));
    waiter.push_bytes(b"AT\r\r\nOK\r\n");
    waiter.fail_with_reason("late failure".into());
    assert!(waiter.wait(100).is_ok(), "the completed reply must win");
}

/// Given a modem that answers ERROR to AT+BAD,
/// when the AT session is set to expect OK,
/// then later at() calls fail on ERROR; an unknown path cannot be configured.
#[test]
fn at_session_config_applies_to_later_at_calls() {
    use serialplugin_core::at::session::AtSessionConfig;
    let fake = Fake::new();
    fake.modem("");
    let m = manager_with("cfg", fake.port(true));
    let r = m.at("cfg".into(), "AT+BAD".into(), None).unwrap();
    assert_ne!(r.status, serialplugin_core::at::parse::AtParseStatus::Ok);
    let strict = || AtSessionConfig {
        expect_ok: Some(true),
        ..Default::default()
    };
    assert!(m.configure_at_session("nope".into(), strict()).is_err());
    m.configure_at_session("cfg".into(), strict()).unwrap();
    assert!(m.at("cfg".into(), "AT+BAD".into(), None).is_err());
    m.close("cfg".into()).unwrap();
}

/// Given ports split or not,
/// when an exchange uses the Purge prepare,
/// then the device input is cleared first.
#[test]
fn purge_prepare_clears_the_port_input() {
    let fake = Fake::new();
    fake.modem("");
    for clonable in [true, false] {
        let m = manager_with("purge", fake.port(clonable));
        let before = fake.0.cleared_input.load(Ordering::SeqCst);
        let opts = ExchangeOptions {
            rx_prepare: Some(RxPrepareMode::Purge),
            timeout_ms: Some(1000),
            ..Default::default()
        };
        m.exchange("purge".into(), "AT\r".into(), opts).unwrap();
        assert!(fake.0.cleared_input.load(Ordering::SeqCst) > before);
        m.close("purge".into()).unwrap();
    }
}

/// Given a modem that prompts for a PDU,
/// when at_phases and send_sms_pdu run,
/// then every phase result comes back, the last with +CMGS.
#[test]
fn at_phases_and_send_sms_pdu_return_every_result() {
    use serialplugin_core::at::session::{AtPhase, AtPhaseWrite};
    let fake = Fake::new();
    fake.respond(|bytes| {
        if bytes.ends_with(&[0x1a]) {
            b"\r\n+CMGS: 5\r\n\r\nOK\r\n".to_vec()
        } else if bytes.starts_with(b"AT+CMGS") {
            b"\r\n> ".to_vec()
        } else {
            b"\r\nOK\r\n".to_vec()
        }
    });
    let m = manager_with("sms", fake.port(true));
    let phase = AtPhase {
        write: AtPhaseWrite::Text("AT".into()),
        completion_mode: None,
        result_format: None,
        timeout_ms: Some(1000),
        command: None,
        rx_prepare: Some(RxPrepareMode::None),
    };
    assert_eq!(m.at_phases("sms".into(), vec![phase]).unwrap().len(), 1);
    let r = m
        .send_sms_pdu("sms".into(), 2, b"00".to_vec(), None)
        .unwrap();
    assert_eq!(r.len(), 2);
    assert_eq!(r[1].solicited_body, vec!["+CMGS: 5".to_string()]);
    m.close("sms".into()).unwrap();
}
