//! Stories: AT sessions: defaults that stick, phases, SMS, a modem without verbose
//! results, commands queued on one port.

use crate::common::*;
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::at::session::{
    AtCommandOptions, AtPhase, AtPhaseWrite, AtSessionConfig, SendSmsPduOptions,
};
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{
    AtResultFormat, CoreEvent, ExchangeOptions, RxPrepareMode, SerialEvent,
};
use serialplugin_core::SerialManager;

fn phase(command: &str) -> AtPhase {
    AtPhase {
        write: AtPhaseWrite::Text(command.into()),
        completion_mode: None,
        result_format: None,
        timeout_ms: None,
        command: None,
        rx_prepare: None,
    }
}

fn with_modem(path: &str, modem: Modem) -> (SerialManager, FakeDevice, ModemLog) {
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    (manager_with(path, device.port()), device, log)
}

/// Given a modem that refuses AT+BAD,
/// when the AT session is configured (expect OK, no CR, then back),
/// then later commands follow it, an unknown path cannot be configured, and one command may
/// differ.
#[test]
fn a_session_applies_to_later_commands() {
    let (m, device, log) = with_modem("cfg", Modem::new().echo());
    let at = |command: &str, options| m.at("cfg".into(), command.into(), options);
    let configure = |config| m.configure_at_session("cfg".into(), config);
    assert_eq!(at("AT+BAD", None).unwrap().status, AtParseStatus::Error);
    let strict = || AtSessionConfig {
        expect_ok: Some(true),
        ..Default::default()
    };
    assert!(m.configure_at_session("nope".into(), strict()).is_err());
    configure(strict()).unwrap();
    assert!(at("AT+BAD", None).is_err());
    assert_eq!(at("AT", None).unwrap().status, AtParseStatus::Ok);

    // No CR: the modem waits for the rest of the line, so the command times out.
    configure(AtSessionConfig {
        append_cr: Some(false),
        default_timeout_ms: Some(50),
        ..Default::default()
    })
    .unwrap();
    assert_eq!(kind(at("AT+", None).unwrap_err()), ErrorKind::Timeout);
    let with_cr = AtCommandOptions {
        append_cr: Some(true),
        timeout_ms: Some(LONG_MS),
        ..serde_json::from_value(serde_json::json!({})).unwrap()
    };
    let reply = at("CSQ", Some(with_cr)).unwrap();
    assert_eq!(reply.lines.last().unwrap(), "OK");
    assert_eq!(log.commands().last().unwrap().line, "AT+CSQ");
    assert!(device.written().ends_with(b"AT+CSQ\r"));
    m.close("cfg".into()).unwrap();
}

/// Given a port prepared by purging,
/// when a command and a two-phase job run on split and unsplit ports,
/// then the device input is cleared once per job: a later phase must not eat the reply to
/// the one before it.
#[test]
fn only_the_first_phase_prepares_rx() {
    for clonable in [true, false] {
        let device = FakeDevice::with_modem(Modem::new());
        let m = if clonable {
            manager_with("purge", device.port())
        } else {
            manager_with("purge", device.unclonable_port())
        };
        let cleared = || {
            let calls = device.calls();
            calls.iter().filter(|c| **c == Call::ClearInput).count()
        };
        let options = ExchangeOptions {
            rx_prepare: Some(RxPrepareMode::Purge),
            ..no_prepare(LONG_MS)
        };
        m.exchange("purge".into(), "AT\r".into(), options).unwrap();
        assert_eq!(cleared(), 1);
        let config = AtSessionConfig {
            default_rx_prepare: Some(RxPrepareMode::Purge),
            ..Default::default()
        };
        m.configure_at_session("purge".into(), config).unwrap();
        let results = m
            .at_phases("purge".into(), vec![phase("AT"), phase("AT+CSQ")])
            .unwrap();
        assert_eq!(cleared(), 2);
        assert_eq!(results[0].command, "AT");
        assert_eq!(results[1].solicited_body, ["+CSQ: 20,99"]);
        m.close("purge".into()).unwrap();
    }
}

/// Given a session with `expectOk` that does not stop on errors,
/// when one phase gets a modem error that mentions a timeout, the next gets no answer and
/// the third is fine,
/// then only the silent phase is `timed_out`; a session that stops returns the error.
#[test]
fn a_timeout_is_not_a_modem_error() {
    let modem = Modem::new()
        .fails("AT+COPS?", "+CME ERROR: network timeout")
        .silent("AT+SLOW");
    let (m, _device, _log) = with_modem("flags", modem);
    let run = |stop| {
        let config = AtSessionConfig {
            expect_ok: Some(true),
            stop_on_error: Some(stop),
            ..Default::default()
        };
        m.configure_at_session("flags".into(), config).unwrap();
        let mut slow = phase("AT+SLOW");
        slow.timeout_ms = Some(20);
        m.at_phases("flags".into(), vec![phase("AT+COPS?"), slow, phase("AT")])
    };
    let flags: Vec<_> = run(false)
        .unwrap()
        .iter()
        .map(|r| (r.status == AtParseStatus::Ok, r.timed_out))
        .collect();
    assert_eq!(flags, [(false, false), (false, true), (true, false)]);
    assert!(run(true).is_err());
    m.close("flags".into()).unwrap();
}

/// Given a modem that prompts for a PDU, and a URC listener,
/// when SMS are sent,
/// then the PDU goes out after the prompt, the reference is the reply's body and no URC,
/// and real URCs still come.
#[test]
fn an_sms_goes_out_after_the_prompt() {
    let (m, device, log) = with_modem("sms", Modem::new());
    let (events, rx) = sink::<CoreEvent>();
    m.watch_core("sms".into(), watch_opts(0, true), None, events)
        .unwrap();
    let options = SendSmsPduOptions {
        timeout_ms: Some(LONG_MS),
        result_format: Some(AtResultFormat::Verbose),
    };
    for options in [None, Some(options)] {
        let results = m
            .send_sms_pdu("sms".into(), 2, b"0011".to_vec(), options)
            .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].command, "AT+CMGS=2");
        assert_eq!(results[1].solicited_body, ["+CMGS: 7"]);
        assert!(results[1].urc_lines.is_empty());
    }
    assert_eq!(log.pdus(), ["0011", "0011"]);
    device.send(b"+CMTI: \"SM\",3\r\n");
    assert_eq!(next_urc(&rx), "+CMTI: \"SM\",3");
    m.close("sms".into()).unwrap();
}

/// Given a modem with numeric results and echo,
/// when the session says so,
/// then commands complete on the result digits.
#[test]
fn numeric_results() {
    let (m, _device, _log) = with_modem("v0", Modem::new().numeric().echo());
    let config = AtSessionConfig {
        result_format: Some(AtResultFormat::Numeric),
        ..Default::default()
    };
    m.configure_at_session("v0".into(), config).unwrap();
    let reply = m.at("v0".into(), "AT+CSQ".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Ok);
    assert_eq!(reply.solicited_body, ["+CSQ: 20,99"]);
    let reply = m.at("v0".into(), "AT+BAD".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Error);
    m.close("v0".into()).unwrap();
}

/// Given a two-phase job whose first phase hangs,
/// when it is cancelled,
/// then the whole job ends as cancelled, its second phase is never sent, and the next
/// command runs as if nothing happened; so does one after a watch sink that calls back.
#[test]
fn a_cancel_ends_the_whole_job() {
    let modem = Modem::new().silent("AT+HANG").reply("AT+NEXT", &[]);
    let (m, device, log) = with_modem("job", modem);
    let job = background(&m, |m| {
        let first = AtPhase {
            timeout_ms: Some(LONG_MS),
            ..phase("AT+HANG")
        };
        m.at_phases("job".into(), vec![first, phase("AT+NEXT")])
    });
    log.wait_command("AT+HANG");
    m.cancel_exchange("job".into()).unwrap();
    assert_eq!(kind(job.join().unwrap().unwrap_err()), ErrorKind::Cancelled);
    let reply = m.at("job".into(), "AT".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Ok);
    assert!(log.commands().iter().all(|c| c.line != "AT+NEXT"));

    // A `watch_core` sink runs off the hub: it may talk to its own port.
    let (done, dones) = sink::<bool>();
    let manager = m.clone();
    let reentrant: Sink<CoreEvent> = std::sync::Arc::new(move |e| {
        if matches!(&e, CoreEvent::Event(SerialEvent::Data { .. })) {
            return done(manager.at("job".into(), "AT".into(), None).is_ok());
        }
        true
    });
    m.watch_core("job".into(), watch_opts(0, false), None, reentrant)
        .unwrap();
    device.send(b"ping");
    assert!(next(&dones, "the sink's own command"));
    m.close("job".into()).unwrap();
}
