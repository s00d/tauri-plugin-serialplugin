# serialplugin-testing

Fake serial devices for tests: no hardware, no timing. Works with anything that takes a
[`serialport::SerialPort`](https://docs.rs/serialport), with
[`serialplugin-core`](https://docs.rs/serialplugin-core) and with
[`tauri-plugin-serialplugin`](https://github.com/s00d/tauri-plugin-serialplugin).

- `FakeDevice`: an in-memory serial port. Script what the host reads, see what it wrote and
  set, fail or hold its calls, unplug it.
- `Modem`: an AT modem. Your replies, echo, numeric result codes, the SMS prompt, a 27.010
  multiplexer with the faults real modems have.
- `mux`: 27.010 frames for hand-written multiplexer tests.
- `Faults`, `Clock`: answers dropped, late, followed by a burst or cut into small reads; time
  that moves when the test says (`ManualClock`) or with `tokio::time` (`TokioClock`, feature
  `tokio`); an event per write (`subscribe_writes`).
- `peer`: the device side of a TCP serial bridge or (unix) a PTY, for tests that need a real
  file descriptor.

```toml
[dev-dependencies]
serialplugin-testing = "0.2"  # features = ["tokio"] for TokioClock
```

## A port without hardware

```rust
use serialplugin_testing::{Call, FakeDevice};
use std::io::{ErrorKind, Read, Write};

// The code under test: anything that uses a serial port.
fn read_id(port: &mut dyn serialport::SerialPort) -> std::io::Result<String> {
    port.write_data_terminal_ready(true)?;
    port.write_all(b"ID?\n")?;
    let mut id = [0; 4];
    port.read_exact(&mut id)?;
    Ok(String::from_utf8_lossy(&id).into_owned())
}

let device = FakeDevice::new();
device.respond(|_, written| if written == b"ID?\n" { b"A-17".to_vec() } else { Vec::new() });
let mut port = device.port();
assert_eq!(read_id(&mut port).unwrap(), "A-17");
assert_eq!(device.calls(), [Call::Dtr(true)]);

// The device is unplugged.
device.fail_reads(ErrorKind::BrokenPipe);
assert_eq!(read_id(&mut port).unwrap_err().kind(), ErrorKind::BrokenPipe);

// And plugged in again.
device.recover();
assert_eq!(read_id(&mut port).unwrap(), "A-17");
```

## An AT modem

```rust
use serialplugin_testing::{FakeDevice, Modem};
use std::io::{Read, Write};

let modem = Modem::new()
    .reply("AT+CPIN?", &["+CPIN: READY"])
    .fails("AT+COPS=2", "+CME ERROR: 30")
    .silent("AT+CFUN=1,1");
let log = modem.log();
let device = FakeDevice::with_modem(modem);
let mut port = device.port();

port.write_all(b"AT+CPIN?\r").unwrap();
let mut reply = [0; 64];
let n = port.read(&mut reply).unwrap();
assert_eq!(&reply[..n], b"\r\n+CPIN: READY\r\n\r\nOK\r\n");
assert_eq!(log.commands()[0].line, "AT+CPIN?");

// An unsolicited line.
device.send(b"\r\n+CMTI: \"SM\",1\r\n");
```

## With serialplugin-core

```rust
use serialplugin_core::SerialManager;
use serialplugin_testing::{FakeDevice, Modem};

let device = FakeDevice::with_modem(Modem::new());
let serial = SerialManager::new();
serial.insert_port("modem".into(), Box::new(device.port())).unwrap();

let reply = serial.at("modem".into(), "AT+CSQ".into(), None).unwrap();
assert_eq!(reply.solicited_body, ["+CSQ: 20,99"]);

serial.close("modem".into()).unwrap();
device.wait_closed();
```

With the Tauri plugin (through IPC, as a webview calls it) and with the multiplexer: the
[testing guide](https://s00d.github.io/tauri-plugin-serialplugin/guides/testing).

## Faults and time

`Faults` act on what the device answers and on how the host reads it; a `Clock` runs the
delays and the port timeout. `ManualClock` moves only on `advance`, `TokioClock` (feature
`tokio`) follows `tokio::time`, so a test with a paused clock waits for nothing.

```rust
use serialplugin_testing::{FakeDevice, Faults, ManualClock, Modem};
use std::io::Write;
use std::sync::Arc;
use std::time::Duration;

let device = FakeDevice::with_modem(Modem::new());
let clock = ManualClock::new();
device.clock(Arc::new(clock.clone()));
device.faults(
    Faults::default()
        .delay(Duration::from_secs(2)) // answers come 2 s late
        .burst(b"\r\n+CREG: 1\r\n", 3) // followed by three lines
        .chunk(4), // in reads of at most 4 bytes
);
let writes = device.subscribe_writes(); // one message per write, no polling
let mut port = device.port();

port.write_all(b"AT\r").unwrap();
assert_eq!(writes.recv().unwrap(), b"AT\r");
assert_eq!(device.unread(), 0);
clock.advance(Duration::from_secs(2));
assert_eq!(device.unread(), 6 + 3 * 12);
```

`Faults::default().silent(true)` drops the answers (the modem still sees the commands);
`device.send_after(d, bytes)` sends on the clock. With a clock set, a read that waits for data
times out when the clock reaches the port timeout. A paused tokio clock does not skip ahead
while a `spawn_blocking` task runs: read on a thread of your own, as a port reader does.

## No timing

No test depends on a delay: delays run on a test clock. Waits end on what they wait for (`wait_written`, `wait_read`,
`wait_handled`, `wait_held`, `wait_closed`, `ModemLog::wait_command`); `WAIT` (10 s) only
limits how long a failure takes. `eventually` and the peers, whose state no event announces,
poll (every millisecond, every 20 ms). To test code that must not block, hold the device
(`hold_reads`, `hold_writes`) instead of measuring time; a hold lasts until the test lets go
or drops its last `FakeDevice`, so a failing test does not hang.

`wait_read` means that the host took the bytes, not that it handled them. `wait_handled` does,
for a host with one reading thread: it asked for more.

Desktop only (Linux, macOS, Windows).

## License

Apache-2.0 OR MIT.
