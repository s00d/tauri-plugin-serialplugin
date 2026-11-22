//! Stories: error kinds and their 4.x texts.

use crate::common::*;
use serialplugin_core::error::{CoreError, Error, ErrorKind};
use serialplugin_core::events::ExchangeOptions;
use serialplugin_core::mock_serial::MockSerialPort;
use serialplugin_core::state::{PortState, SerialportInfo};
use serialplugin_core::SerialManager;
use std::error::Error as _;
use std::io;

/// Given a manager, a missing device and a port that fails in turn,
/// when calls fail (not found, unplugged, refused, timed out, cancelled, too long, busy),
/// then each error has its kind, keeps its io::Error, and has the exact 4.x text.
#[test]
fn every_failure_has_its_kind_and_its_4x_text() {
    let m = SerialManager::new();
    let e = m.write_core("nope".into(), b"x".to_vec()).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound);
    let old = m.write("nope".into(), "x".into()).unwrap_err();
    assert_eq!(Error::from(e).to_string(), old.to_string());

    let missing = "/dev/serialplugin-does-not-exist".to_string();
    let open = m.open_core(
        missing.clone(),
        9600,
        None,
        None,
        None,
        None,
        None,
        Default::default(),
    );
    let e = open.unwrap_err();
    assert_eq!(e.kind(), ErrorKind::NotFound, "{e}");
    let old = crate::common::open(&m, &missing).unwrap_err();
    assert_eq!(e.to_string(), old.to_string());
    assert!(matches!(Error::from(e), Error::String(_)));

    let device = FakeDevice::new();
    let m = manager_with("kinds", device.port());
    for (io_kind, kind) in [
        (io::ErrorKind::BrokenPipe, ErrorKind::Disconnected),
        (io::ErrorKind::PermissionDenied, ErrorKind::PermissionDenied),
        (io::ErrorKind::TimedOut, ErrorKind::Timeout),
        (io::ErrorKind::InvalidData, ErrorKind::Other),
    ] {
        device.fail_writes(io_kind);
        let e = m.write_core("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.kind(), kind, "{e}");
        let source = e.source().and_then(|s| s.downcast_ref::<io::Error>());
        assert_eq!(source.expect("io source").kind(), io_kind);
        let old = m.write_binary("kinds".into(), b"x".to_vec()).unwrap_err();
        assert_eq!(e.to_string(), old.to_string());
        assert_eq!(e.message(), old.to_string());
        // An exchange that cannot send fails with it, not at its timeout.
        let e = m
            .exchange_core("kinds".into(), b"AT\r".to_vec(), no_prepare(LONG_MS))
            .unwrap_err();
        assert!(e.to_string().contains("fake device: write error"), "{e}");
    }

    let device = FakeDevice::new();
    let m = manager_with("kinds", device.port());
    let exchange = |options| m.exchange_core("kinds".into(), b"AT\r".to_vec(), options);
    let e = exchange(no_prepare(20)).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::Timeout, "{e}");
    assert_eq!(e.to_string(), "exchange timed out after 20 ms");

    let sent = device.written().len();
    let waiting = background(&m, |m| {
        m.exchange_core("kinds".into(), b"AT\r".to_vec(), no_prepare(LONG_MS))
    });
    device.wait_written("the command", |w| w.len() == sent + 3);
    m.cancel_exchange("kinds".into()).unwrap();
    let e = waiting.join().unwrap().unwrap_err();
    assert_eq!(
        (e.kind(), e.to_string()),
        (ErrorKind::Cancelled, "exchange cancelled".into())
    );

    device.respond(|_| vec![b'x'; 9]);
    let options = ExchangeOptions {
        max_bytes: Some(9),
        ..no_prepare(LONG_MS)
    };
    let e = exchange(options).unwrap_err();
    assert_eq!(e.to_string(), "exchange response exceeded 9 bytes");

    // A read that waits, and a second one.
    let reading = background(&m, |m| loop {
        // A probe below may have its slot in first.
        match m.read("kinds".into(), Some(LONG_MS), None) {
            Err(e) if kind(e.clone()) == ErrorKind::Busy => {}
            done => break done,
        }
    });
    eventually("the second read is refused", || {
        match m.read("kinds".into(), Some(0), None) {
            Err(e) => kind(e) == ErrorKind::Busy,
            Ok(_) => false,
        }
    });
    device.send(b"done");
    assert_eq!(reading.join().unwrap().unwrap(), "done");
    m.close_core("kinds".into()).unwrap();
}

/// Given errors of the layers below and of the core's own texts,
/// when they are classified,
/// then a port open elsewhere is busy, a vanished one not found, and each I/O kind and
/// core text has its kind; a core error keeps its text and source.
#[test]
fn foreign_errors_are_classified() {
    use io::ErrorKind as K;
    for (io_kind, kind) in [
        (K::NotFound, ErrorKind::NotFound),
        (K::PermissionDenied, ErrorKind::PermissionDenied),
        (K::TimedOut, ErrorKind::Timeout),
        (K::WouldBlock, ErrorKind::Timeout),
        (K::BrokenPipe, ErrorKind::Disconnected),
        (K::NotConnected, ErrorKind::Disconnected),
        (K::ConnectionAborted, ErrorKind::Disconnected),
        (K::ConnectionReset, ErrorKind::Disconnected),
        (K::UnexpectedEof, ErrorKind::Disconnected),
        (K::AddrInUse, ErrorKind::Busy),
        (K::ResourceBusy, ErrorKind::Busy),
        (K::Interrupted, ErrorKind::Cancelled),
        (K::InvalidData, ErrorKind::Other),
    ] {
        assert_eq!(ErrorKind::from_io(io_kind), kind, "{io_kind:?}");
        let e = serialport::Error::new(serialport::ErrorKind::Io(io_kind), "io");
        assert_eq!(ErrorKind::from_serialport(&e), kind, "{io_kind:?}");
    }
    let no_device = |text: &str| {
        let e = serialport::Error::new(serialport::ErrorKind::NoDevice, text);
        ErrorKind::from_serialport(&e)
    };
    for text in [
        "Unable to acquire exclusive lock on serial port",
        "Access is denied.",
        "Device or resource busy",
    ] {
        assert_eq!(no_device(text), ErrorKind::Busy, "{text}");
    }
    assert_eq!(no_device("No such file"), ErrorKind::NotFound);
    for (text, kind) in [
        ("read already in progress", ErrorKind::Busy),
        ("port busy", ErrorKind::Busy),
        ("port closed", ErrorKind::Cancelled),
        ("weird", ErrorKind::Other),
    ] {
        let e = CoreError::from(Error::new(text));
        assert_eq!((e.kind(), e.message()), (kind, text.into()));
        assert!(e.source().is_none());
    }
    let e = CoreError::new(ErrorKind::Other, Error::String("context".into()))
        .with_source(io::Error::other("gone"));
    assert_eq!(e.source().unwrap().to_string(), "gone");
    assert!(Error::Io("x".into()).source().is_none());

    let mut info = SerialportInfo::new(Box::new(MockSerialPort::new()));
    assert_eq!(info.state.not_connected_reason(), "Port is connected");
    assert!(info.connected_port_mut().is_some());
    for (state, reason) in [
        (PortState::Closed, "Port is closed"),
        (PortState::Opening, "Port is still opening"),
    ] {
        info.state = state;
        assert_eq!(info.state.not_connected_reason(), reason);
        assert!(info.connected_port_mut().is_none());
    }
}
