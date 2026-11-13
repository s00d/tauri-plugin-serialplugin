//! Stories: error kinds and their 4.x texts.

use crate::common::*;

/// Given a manager with a fake port,
/// when calls fail as NotFound, Disconnected, PermissionDenied, Timeout, Cancelled,
/// then each CoreError has that kind, keeps its io::Error source, and converts to the exact 4.x text.
#[test]
fn core_error_kinds_and_old_texts() {
    let m = SerialManager::new();

    // NotFound, with the 4.x text on conversion.
    let e = m.write_core("nope".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    let old = m.write("nope".into(), "x".into()).unwrap_err();
    assert_eq!(Error::from(e).to_string(), old.to_string());

    // open of a missing device: same text as `open`, kind NotFound.
    let missing = "/dev/serialplugin-does-not-exist".to_string();
    let e = m
        .open_core(
            missing.clone(),
            9600,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
        )
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound, "{e}");
    let old = m
        .open(missing, 9600, None, None, None, None, None)
        .unwrap_err();
    assert_eq!(e.to_string(), old.to_string());
    assert!(matches!(Error::from(e), Error::String(_)));

    // Disconnected / PermissionDenied keep the io::Error as source.
    let fake = Fake::new();
    let m = manager_with("kinds", fake.port(true));
    for (io_kind, kind) in [
        (io::ErrorKind::BrokenPipe, ErrorKind::Disconnected),
        (io::ErrorKind::PermissionDenied, ErrorKind::PermissionDenied),
    ] {
        fake.fail_writes(io_kind);
        let e = m.write_core("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.kind(), kind, "{e}");
        let src = std::error::Error::source(&e)
            .and_then(|s| s.downcast_ref::<io::Error>())
            .expect("io source");
        assert_eq!(src.kind(), io_kind);
        let old = m.write_binary("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.to_string(), old.to_string());
    }
    *fake.0.write_error.lock().unwrap() = None;

    // Timeout.
    let opts = ExchangeOptions {
        timeout_ms: Some(100),
        rx_prepare: Some(RxPrepareMode::None),
        ..Default::default()
    };
    let e = m
        .exchange_core("kinds".into(), b"AT\r".to_vec(), opts)
        .unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Timeout, "{e}");
    assert_eq!(e.to_string(), "exchange timed out after 100 ms");

    // Cancelled: once the exchange's command reached the device, it is waiting.
    let (wrote, written) = mpsc::channel();
    let wrote = Mutex::new(wrote);
    fake.respond(move |_| {
        let _ = wrote.lock().unwrap().send(());
        Vec::new()
    });
    let bg = m.clone();
    let t = thread::spawn(move || {
        bg.exchange_core(
            "kinds".into(),
            b"AT\r".to_vec(),
            ExchangeOptions {
                timeout_ms: Some(5000),
                rx_prepare: Some(RxPrepareMode::None),
                ..Default::default()
            },
        )
    });
    written
        .recv_timeout(Duration::from_secs(3))
        .expect("exchange wrote its command");
    m.cancel_exchange("kinds".into()).unwrap();
    let e = t.join().unwrap().unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Cancelled, "{e}");

    // Busy / Other from old errors.
    assert_eq!(
        CoreError::from(Error::new("read already in progress")).kind(),
        ErrorKind::Busy
    );
    assert_eq!(
        CoreError::from(Error::new("weird")).kind(),
        ErrorKind::Other
    );
    m.close_core("kinds".into()).unwrap();
}

/// Given NoDevice errors with the texts flock and Windows give for a port opened elsewhere,
/// when they are mapped to an ErrorKind,
/// then they are Busy, a missing device NotFound.
#[test]
fn port_in_use_errors_are_busy() {
    for text in [
        "Unable to acquire exclusive lock on serial port",
        "Access is denied.",
        "Device or resource busy",
    ] {
        let e = serialport::Error::new(serialport::ErrorKind::NoDevice, text);
        assert_eq!(ErrorKind::from_serialport(&e), ErrorKind::Busy, "{text}");
    }
    let gone = serialport::Error::new(serialport::ErrorKind::NoDevice, "No such file");
    assert_eq!(ErrorKind::from_serialport(&gone), ErrorKind::NotFound);
}
