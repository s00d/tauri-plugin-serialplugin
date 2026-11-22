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

/// Given both ends of a PTY
/// When the host writes to the slave and the device answers on the master
/// Then each reads the other's bytes
#[cfg(unix)]
#[test]
fn a_pty_carries_bytes_both_ways() {
    let (mut device, mut host, path) = peer::pty();
    assert!(path.starts_with("/dev/"), "{path}");
    host.write_all(b"ping").unwrap();
    assert_eq!(peer::recv_until(&mut device, |b| b.len() >= 4), b"ping");
    device.write_all(b"pong").unwrap();
    assert_eq!(peer::recv_until(&mut host, |b| b.len() >= 4), b"pong");
}
