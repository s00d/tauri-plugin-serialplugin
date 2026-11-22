//! Stories: the device side of real file descriptors.

use serialplugin_testing::{peer, Modem};
use std::io::{Read, Write};
use std::net::TcpStream;

/// Given a host connected to a TCP serial bridge
/// When the bridge reads by hand, then a modem serves it, then the host leaves
/// Then both sides see each other's bytes, and the server ends with the connection
#[test]
fn a_tcp_bridge_with_a_modem() {
    let (listener, path) = peer::tcp_listener();
    let mut host = TcpStream::connect(path.strip_prefix("tcp://").unwrap()).unwrap();
    let mut bridge = peer::accept(&listener);
    host.write_all(b"hello").unwrap();
    assert_eq!(peer::recv_until(&mut bridge, |b| b.len() >= 5), b"hello");

    let modem = Modem::new();
    let log = modem.log();
    let server = peer::serve(Box::new(bridge), modem);
    host.write_all(b"AT\r").unwrap();
    let mut reply = [0; 6];
    host.read_exact(&mut reply).unwrap();
    assert_eq!(&reply, b"\r\nOK\r\n");
    log.wait_command("AT");
    drop(host);
    drop(server);
}

/// Given a bridge whose host hangs up
/// When the test still waits for bytes
/// Then it fails at once, and says that the host closed the connection
/// Given a listener that had accepted
/// When it accepts again
/// Then it blocks as before
#[test]
fn a_host_that_hangs_up_ends_the_wait() {
    let (listener, path) = peer::tcp_listener();
    let connect = || TcpStream::connect(path.strip_prefix("tcp://").unwrap()).unwrap();
    let mut host = connect();
    let mut bridge = peer::accept(&listener);
    host.write_all(b"half").unwrap();
    drop(host);
    let waiting = std::thread::spawn(move || peer::recv_until(&mut bridge, |b| b.len() >= 9));
    let panic = waiting.join().unwrap_err();
    let message = panic.downcast_ref::<String>().unwrap();
    assert!(message.contains("closed the connection"), "{message}");
    assert!(message.contains("half"), "{message}");

    let accepting = std::thread::spawn(move || listener.accept().map(|_| ()));
    let _host = connect();
    accepting.join().unwrap().expect("a blocking accept");
}

/// Given a modem serving a host that reads nothing, and one that says nothing
/// When the servers are dropped
/// Then they end, whatever the host does
#[test]
fn a_server_always_ends() {
    let (listener, path) = peer::tcp_listener();
    let connect = || TcpStream::connect(path.strip_prefix("tcp://").unwrap()).unwrap();
    let silent = connect();
    let server = peer::serve(Box::new(peer::accept(&listener)), Modem::new());
    drop(server);

    let mut deaf = connect();
    let reply = vec![b'x'; 16 << 20];
    let modem = Modem::new().raw("AT+MUCH", &reply);
    let log = modem.log();
    let server = peer::serve(Box::new(peer::accept(&listener)), modem);
    deaf.write_all(b"AT+MUCH\r").unwrap();
    log.wait_command("AT+MUCH");
    drop(server);
    drop((silent, deaf));
}

/// Given a modem serving a PTY
/// When the host closes the port and opens it again
/// Then the modem answers the new session too
#[cfg(unix)]
#[test]
fn a_pty_is_served_again_when_it_is_reopened() {
    let peer::Pty {
        device,
        mut host,
        path,
        ..
    } = peer::pty();
    let modem = Modem::new();
    let log = modem.log();
    let _server = peer::serve(Box::new(device), modem);
    for n in 1..=2 {
        host.write_all(b"AT\r").unwrap();
        let reply = peer::recv_until(&mut host, |b| b.ends_with(b"OK\r\n"));
        assert_eq!(reply, b"\r\nOK\r\n");
        assert_eq!(log.commands().len(), n);
        drop(host);
        // Baud 0: macOS sets none on a PTY.
        let baud = if cfg!(target_os = "macos") {
            0
        } else {
            115_200
        };
        host = serialport::new(&path, baud).open_native().unwrap();
    }
}

/// Given both ends of a PTY
/// When the host writes to the slave and the device answers on the master
/// Then each reads the other's bytes
#[cfg(unix)]
#[test]
fn a_pty_carries_bytes_both_ways() {
    let peer::Pty {
        mut device,
        mut host,
        path,
        ..
    } = peer::pty();
    assert!(path.starts_with("/dev/"), "{path}");
    host.write_all(b"ping").unwrap();
    assert_eq!(peer::recv_until(&mut device, |b| b.len() >= 4), b"ping");
    device.write_all(b"pong").unwrap();
    assert_eq!(peer::recv_until(&mut host, |b| b.len() >= 4), b"pong");
}
