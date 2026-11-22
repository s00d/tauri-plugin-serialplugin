//! Stories: a host using a fake port, and a test author watching it.

use serialplugin_testing::{Call, FakeDevice, Lines};
use serialport::{ClearBuffer, Parity, SerialPort};
use std::io::{ErrorKind, Read, Write};
use std::time::Duration;

fn read_some(port: &mut dyn SerialPort) -> std::io::Result<Vec<u8>> {
    let mut buf = [0; 64];
    port.read(&mut buf).map(|n| buf[..n].to_vec())
}

/// Given an open port
/// When the host configures it, writes, reads in pieces and clears its input
/// Then the device saw every setting, byte and call in order, and the host reads exactly
/// what was sent
#[test]
fn a_session_is_recorded_and_served() {
    let device = FakeDevice::new();
    let mut port = device.port();
    port.set_baud_rate(9600).unwrap();
    port.set_parity(Parity::Even).unwrap();
    port.write_request_to_send(true).unwrap();
    port.set_break().unwrap();
    port.clear_break().unwrap();
    assert_eq!(port.baud_rate().unwrap(), 9600);
    assert_eq!(port.parity().unwrap(), Parity::Even);

    port.set_timeout(Duration::from_millis(70)).unwrap();
    port.write_all(b"one").unwrap();
    port.write_all(b"two").unwrap();
    assert_eq!(device.written(), b"onetwo");
    assert_eq!(device.write_timeouts(), [Duration::from_millis(70); 2]);

    device.send(b"abcdef");
    assert_eq!(port.bytes_to_read().unwrap(), 6);
    let mut buf = [0; 4];
    port.read_exact(&mut buf).unwrap();
    assert_eq!((&buf, device.unread()), (b"abcd", 2));
    assert_eq!(
        port.read(&mut []).unwrap(),
        0,
        "an empty read takes nothing"
    );
    port.clear(ClearBuffer::All).unwrap();
    device.wait_read();
    port.set_timeout(Duration::ZERO).unwrap();
    assert_eq!(
        read_some(&mut port).unwrap_err().kind(),
        ErrorKind::TimedOut
    );

    device.set_driver_pending(Some(7));
    device.set_out_pending(3);
    assert_eq!(
        (
            port.bytes_to_read().unwrap(),
            port.bytes_to_write().unwrap()
        ),
        (7, 3)
    );
    device.set_lines(Lines::default().cts(false).ring(true));
    assert!(!port.read_clear_to_send().unwrap() && port.read_ring_indicator().unwrap());
    assert!(port.read_data_set_ready().unwrap() && !port.read_carrier_detect().unwrap());
    assert_eq!(
        device.calls(),
        [
            Call::BaudRate(9600),
            Call::Parity(Parity::Even),
            Call::Rts(true),
            Call::Break(true),
            Call::Break(false),
            Call::ClearInput,
            Call::ClearOutput,
        ]
    );
}

/// Given a host with a reading and a writing handle of one port
/// When the device holds its read, then its write, then lets go
/// Then the other handle works meanwhile, the held calls finish with their data, and the
/// device sees the port closed once both handles are gone
#[test]
fn held_calls_and_clones() {
    let device = FakeDevice::new();
    let mut reader = device.port();
    let mut writer = reader.try_clone().unwrap();
    assert_eq!(device.open_ports(), 2);

    device.hold_reads(true);
    let reading = std::thread::spawn(move || read_some(&mut reader));
    device.wait_held(1);
    writer.write_all(b"while held").unwrap();
    device.send(b"late");
    device.hold_reads(false);
    assert_eq!(reading.join().unwrap().unwrap(), b"late");

    device.hold_writes(true);
    let writing = std::thread::spawn(move || writer.write_all(b"!"));
    device.wait_held(1);
    assert_eq!(device.written(), b"while held");
    device.hold_writes(false);
    writing.join().unwrap().unwrap();
    device.wait_written("the held write", |w| w.ends_with(b"!"));
    device.wait_closed();

    assert!(device.unclonable_port().try_clone().is_err());
}

/// Given a host whose read waits for data (and one that reads later)
/// When the device holds reads, sends, and the test lets go, by a call or by dropping its
/// last handle as a failing test does
/// Then the waiting read is held too and gets nothing meanwhile, and no read stays blocked
#[test]
fn a_hold_stops_a_waiting_read_and_ends_with_the_test() {
    let device = FakeDevice::new();
    let mut port = device.port();
    port.set_timeout(Duration::from_secs(60)).unwrap();
    let mut writer = port.try_clone().unwrap();
    let reading = std::thread::spawn(move || (read_some(&mut port), port));
    // Nothing unread and a read in progress: it waits.
    device.wait_handled();
    device.hold_reads(true);
    device.wait_held(1);
    device.send(b"held back");
    let other = device.clone();
    drop(device);
    assert_eq!(other.held(), 1, "another handle still holds");
    assert_eq!(other.unread(), 9);
    other.hold_reads(false);
    let (got, mut port) = reading.join().unwrap();
    assert_eq!(got.unwrap(), b"held back");

    other.hold_reads(true);
    other.hold_writes(true);
    let reading = std::thread::spawn(move || read_some(&mut port));
    let writing = std::thread::spawn(move || writer.write_all(b"x"));
    other.wait_held(2);
    other.send(b"!");
    drop(other);
    assert_eq!(reading.join().unwrap().unwrap(), b"!");
    writing.join().unwrap().unwrap();
}

/// Given a host with two handles and a device that answers every write
/// When the answer itself writes, the other handle writes meanwhile, the answer panics, and
/// it sets a new one
/// Then every write is answered, one at a time and in order, and no answer is lost
#[test]
fn every_write_is_answered_in_order() {
    let device = FakeDevice::new();
    let mut port = device.port();
    let mut inside = port.try_clone().unwrap();
    let mut outside = Some(port.try_clone().unwrap());
    device.respond(move |_, bytes| {
        if bytes == b"one" {
            inside.write_all(b"two").unwrap();
            let mut outside = outside.take().unwrap();
            // Returns although the answer to "one" is not out yet.
            std::thread::spawn(move || outside.write_all(b"three").unwrap())
                .join()
                .unwrap();
        }
        assert_ne!(bytes, b"bad", "expected by this test");
        [b"<", bytes, b">"].concat()
    });
    port.write_all(b"one").unwrap();
    assert_eq!(read_some(&mut port).unwrap(), b"<one><two><three>");
    assert_eq!(device.written(), b"onetwothree");

    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port.write(b"bad")));
    assert!(failed.is_err());
    port.write_all(b"four").unwrap();
    assert_eq!(read_some(&mut port).unwrap(), b"<four>");

    device.respond(|device, bytes| {
        device.respond(|_, bytes| [b"new ", bytes].concat());
        [b"old ", bytes].concat()
    });
    port.write_all(b"a ").unwrap();
    port.write_all(b"b").unwrap();
    assert_eq!(read_some(&mut port).unwrap(), b"old a new b");
}

/// Given a device answering every write, itself sending to the host from the answer
/// When it stalls, ends reads with EOF a few times, fails, and recovers
/// Then writes time out, the EOFs are counted, the failure lasts until the recovery, and data
/// flows again
#[test]
fn faults_come_and_go() {
    let device = FakeDevice::new();
    let mut port = device.port();
    device.respond(|device, bytes| {
        device.send(b"urc ");
        bytes.to_ascii_uppercase()
    });
    port.write_all(b"hi").unwrap();
    assert_eq!(read_some(&mut port).unwrap(), b"urc HI");

    device.stall_writes(true);
    assert_eq!(port.write(b"x").unwrap_err().kind(), ErrorKind::TimedOut);
    device.stall_writes(false);
    device.eof_times(2);
    device.send(b"after");
    assert_eq!(read_some(&mut port).unwrap(), b"");
    assert_eq!(read_some(&mut port).unwrap(), b"");
    assert_eq!(read_some(&mut port).unwrap(), b"after");
    assert_eq!(device.eof_reads(), 2);
    device.eof();
    assert_eq!(read_some(&mut port).unwrap(), b"");

    device.fail_writes(ErrorKind::BrokenPipe);
    device.fail_reads(ErrorKind::NotConnected);
    assert_eq!(port.write(b"x").unwrap_err().kind(), ErrorKind::BrokenPipe);
    assert_eq!(
        read_some(&mut port).unwrap_err().kind(),
        ErrorKind::NotConnected
    );
    assert_eq!(device.written(), b"hi");

    device.stall_writes(true);
    device.fail_reads_times(5, ErrorKind::Other);
    device.recover();
    port.write_all(b"ok").unwrap();
    assert_eq!(read_some(&mut port).unwrap(), b"urc OK");
}
