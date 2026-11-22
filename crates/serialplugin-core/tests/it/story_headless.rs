//! Stories: plain Rust, no Tauri, real file descriptors: a TCP serial bridge and (unix) a
//! PTY, opened by path.

use crate::common::*;
use serialplugin_core::at::parse::AtParseStatus;
use serialplugin_core::error::ErrorKind;
use serialplugin_core::events::{CoreEvent, PortStatus, SerialEvent, WatchOptions};
use serialplugin_core::state::StopBits;
use serialplugin_core::SerialManager;
use std::io::Write;

/// Watch, talk, stop: what any app does on the open `path`.
fn use_modem(m: &SerialManager, path: &str) {
    let (events, rx) = sink::<SerialEvent>();
    let crlf = parser(serde_json::json!({"type": "readline", "delimiter": "\r\n"}));
    let watch = m
        .watch_framed(path.into(), WatchOptions::default(), crlf, events)
        .unwrap();
    m.write(path.into(), "go\r".into()).unwrap();
    assert_eq!(next_data(&rx), b"hello");
    assert_eq!(next_data(&rx), b"world");
    let reply = m.at(path.into(), "AT+CSQ".into(), None).unwrap();
    assert_eq!(reply.status, AtParseStatus::Ok);
    assert_eq!(reply.solicited_body, ["+CSQ: 20,99"]);
    m.unwatch(watch).unwrap();
}

fn modem() -> Modem {
    Modem::new().raw("go", b"hello\r\nworld\r\n")
}

/// Given a modem behind a TCP serial bridge,
/// when an app opens it, watches it with a parser, talks AT and closes it,
/// then every step works, settings need the open port, and nothing stays managed.
#[test]
fn a_tcp_bridge_from_open_to_close() {
    let (listener, path) = peer::tcp_listener();
    let m = SerialManager::new();
    let settings = || m.set_stop_bits(path.clone(), StopBits::Two);
    assert_eq!(kind(settings().unwrap_err()), ErrorKind::NotFound);
    assert_eq!(open(&m, &path).unwrap(), path);
    let _server = peer::serve(Box::new(peer::accept(&listener)), modem());
    assert_eq!(m.managed_ports().unwrap(), [path.as_str()]);
    settings().unwrap();
    use_modem(&m, &path);
    m.close(path).unwrap();
    assert!(m.managed_ports().unwrap().is_empty());
}

/// Given the same modem on a PTY (macOS opens none by path: it has no baud rate),
/// when the app hands its port to the manager,
/// then it works the same.
#[cfg(unix)]
#[test]
fn a_pty_from_open_to_close() {
    let peer::Pty {
        device, host, path, ..
    } = peer::pty();
    let _server = peer::serve(Box::new(device), modem());
    let m = SerialManager::new();
    m.insert_port(path.clone(), Box::new(host)).unwrap();
    use_modem(&m, &path);
    m.close(path).unwrap();
}

/// Given a bridge that greets as soon as it accepts, and then hangs up,
/// when `open_watch_core` opens it,
/// then the stream gets the whole greeting and the Disconnect, and the path opens again:
/// reopening an open path closes it first.
#[test]
fn a_bridge_that_greets_and_hangs_up() {
    let (listener, path) = peer::tcp_listener();
    let m = SerialManager::new();
    let (states, states_rx) = sink::<CoreEvent>();
    m.subscribe_core(states);
    let greeter = std::thread::spawn(move || {
        let mut bridge = peer::accept(&listener);
        bridge.write_all(b"RDY\r\n+QIND: 1\r\n").unwrap();
        listener
    });
    let (events, rx) = sink::<CoreEvent>();
    let (session, _) = open_watch(&m, &path, events).expect("open succeeds");
    assert_eq!(session, path);
    assert_eq!(next_data(&rx), b"RDY\r");
    assert_eq!(next_data(&rx), b"+QIND: 1\r");
    let (gone, reason) = next_disconnect(&rx);
    assert_eq!(gone, path);
    assert!(reason.contains("closed the connection"), "{reason}");
    assert_eq!(next_state(&states_rx, &path), PortStatus::Opened);
    assert!(matches!(
        next_state(&states_rx, &path),
        PortStatus::Disconnected {
            kind: ErrorKind::Disconnected,
            ..
        }
    ));

    let listener = greeter.join().unwrap();
    open(&m, &path).expect("a released path opens again");
    assert_eq!(next_state(&states_rx, &path), PortStatus::Opened);
    open(&m, &path).expect("so does an open one");
    assert_eq!(next_state(&states_rx, &path), PortStatus::Closed);
    assert_eq!(next_state(&states_rx, &path), PortStatus::Opened);
    m.close(path).unwrap();
    drop(listener);
}
