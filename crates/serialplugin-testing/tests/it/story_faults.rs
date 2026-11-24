//! Faults of a port and its clock, as a test author uses them: a modem that does not answer,
//! a burst of lines, replies cut into small reads, late replies, timeouts on a test clock and an
//! event for every write.

use serialplugin_testing::{FakeDevice, Faults, ManualClock, Modem};
use std::io::{ErrorKind, Read, Write};
use std::sync::Arc;
use std::time::Duration;

const OK: &[u8] = b"\r\nOK\r\n";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Everything the host can read right now, read by reads of `size` bytes.
fn drain(port: &mut impl Read, size: usize, device: &FakeDevice) -> Vec<Vec<u8>> {
    let mut reads = Vec::new();
    while device.unread() > 0 {
        let mut buf = vec![0; size];
        let n = port.read(&mut buf).unwrap();
        reads.push(buf[..n].to_vec());
    }
    reads
}

/// Given a modem on a device whose faults make it silent, on a manual clock
/// When the host sends a command and waits for the answer
/// Then the modem saw the command, nothing comes back, and the read times out when the
/// clock reaches the port timeout, not before. The timeout is an hour: real time cannot end
/// the read, only the clock.
#[test]
fn a_silent_device_lets_a_read_time_out_on_its_clock() {
    let modem = Modem::new();
    let log = modem.log();
    let device = FakeDevice::with_modem(modem);
    let clock = ManualClock::new();
    device.clock(Arc::new(clock.clone()));
    device.faults(Faults::default().silent(true));
    let mut port = device.port();
    serialport::SerialPort::set_timeout(&mut port, Duration::from_secs(3_600)).unwrap();

    port.write_all(b"AT\r").unwrap();
    log.wait_command("AT");
    let reader = std::thread::spawn(move || {
        let mut buf = [0; 16];
        port.read(&mut buf).map_err(|e| e.kind())
    });
    device.wait_asked(16);
    clock.advance(Duration::from_secs(3_599));
    assert!(
        !reader.is_finished(),
        "timed out before the clock got there"
    );
    clock.advance(Duration::from_secs(1));
    assert_eq!(reader.join().unwrap(), Err(ErrorKind::TimedOut));
    assert_eq!(device.unread(), 0);
}

/// Given a device whose answers are followed by a burst of three unsolicited lines
/// When the host sends a command
/// Then it reads the answer, then the three lines.
#[test]
fn a_burst_follows_every_answer() {
    let device = FakeDevice::with_modem(Modem::new());
    let urc = b"\r\n+CREG: 1\r\n";
    device.faults(Faults::default().burst(urc, 3));
    let mut port = device.port();

    port.write_all(b"AT\r").unwrap();
    let mut expected = OK.to_vec();
    (0..3).for_each(|_| expected.extend(urc));
    let got: Vec<u8> = drain(&mut port, 256, &device).concat();
    assert_eq!(got, expected);
}

/// Given a device with a burst after every answer
/// When the host writes a command in two pieces, and a command the modem leaves unanswered
/// Then one burst follows the one answer: none after a write that got no answer.
#[test]
fn no_burst_without_an_answer() {
    let device = FakeDevice::with_modem(Modem::new().silent("AT+X"));
    let urc = b"\r\n+CREG: 1\r\n";
    device.faults(Faults::default().burst(urc, 1));
    let mut port = device.port();

    port.write_all(b"AT").unwrap();
    port.write_all(b"\r").unwrap();
    port.write_all(b"AT+X\r").unwrap();
    assert_eq!(drain(&mut port, 256, &device).concat(), [OK, urc].concat());
}

/// Given a device that hands out at most 5 bytes per read
/// When the host reads a reply with a large buffer
/// Then every read has 5 bytes or fewer, and together they are the reply.
#[test]
fn replies_are_cut_into_small_reads() {
    let device = FakeDevice::with_modem(Modem::new());
    device.faults(Faults::default().chunk(5));
    let mut port = device.port();

    port.write_all(b"AT+CSQ\r").unwrap();
    let reads = drain(&mut port, 256, &device);
    assert!(
        reads.iter().all(|r| !r.is_empty() && r.len() <= 5),
        "{reads:?}"
    );
    assert_eq!(reads.concat(), b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n");
    assert!(reads.len() > 1);
}

/// Given a device whose answers come 1 s late on a manual clock
/// When the host sends two commands
/// Then nothing arrives until the clock is 1 s past each, and the answers keep their order.
#[test]
fn late_answers_arrive_on_the_clock_in_order() {
    let device = FakeDevice::with_modem(Modem::new());
    let clock = ManualClock::new();
    device.clock(Arc::new(clock.clone()));
    device.faults(Faults::default().delay(Duration::from_secs(1)));
    let mut port = device.port();

    port.write_all(b"AT+CSQ\r").unwrap();
    clock.advance(ms(400));
    port.write_all(b"AT\r").unwrap();
    clock.advance(ms(599));
    assert_eq!(device.unread(), 0);
    clock.advance(ms(1));
    assert_eq!(
        drain(&mut port, 256, &device).concat(),
        b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n"
    );
    clock.advance(ms(400));
    assert_eq!(drain(&mut port, 256, &device).concat(), OK);
}

/// Given late answers on a manual clock
/// When the host sends a command, the faults end, and it sends another
/// Then the second answer still comes after the first: none overtakes an earlier one.
#[test]
fn a_cut_delay_does_not_reorder_answers() {
    let device = FakeDevice::with_modem(Modem::new());
    let clock = ManualClock::new();
    device.clock(Arc::new(clock.clone()));
    device.faults(Faults::default().delay(Duration::from_secs(1)));
    let mut port = device.port();

    port.write_all(b"AT+CSQ\r").unwrap();
    device.faults(Faults::default());
    port.write_all(b"AT\r").unwrap();
    assert_eq!(device.unread(), 0);
    clock.advance(Duration::from_secs(1));
    assert_eq!(
        drain(&mut port, 256, &device).concat(),
        b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\nOK\r\n"
    );
}

/// Given an answer late on one clock
/// When the device gets another clock and the host sends a second command without delay
/// Then the late answer comes at once, before the second: its clock is gone.
#[test]
fn a_new_clock_hands_over_the_late_answers() {
    let device = FakeDevice::with_modem(Modem::new());
    device.clock(Arc::new(ManualClock::new()));
    device.faults(Faults::default().delay(Duration::from_secs(1)));
    let mut port = device.port();

    port.write_all(b"AT+CSQ\r").unwrap();
    assert_eq!(device.unread(), 0);
    device.clock(Arc::new(ManualClock::new()));
    device.faults(Faults::default());
    port.write_all(b"AT\r").unwrap();
    assert_eq!(
        drain(&mut port, 256, &device).concat(),
        b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\nOK\r\n"
    );
}

/// A clock whose time is set by hand and whose jobs run only when the test says (a job due
/// already runs at once): its time can pass a job before the job ran.
#[derive(Default)]
struct Stalled {
    now: std::sync::Mutex<Duration>,
    jobs: std::sync::Mutex<Vec<serialplugin_testing::Job>>,
}

impl std::fmt::Debug for Stalled {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Stalled").finish_non_exhaustive()
    }
}

impl serialplugin_testing::Clock for Stalled {
    fn now(&self) -> Duration {
        *self.now.lock().unwrap()
    }

    fn at(&self, at: Duration, job: serialplugin_testing::Job) {
        if at <= self.now() {
            job();
        } else {
            self.jobs.lock().unwrap().push(job);
        }
    }
}

/// Given an answer late on a clock whose time has passed it before its job ran
/// When the delay ends and the host sends a second command
/// Then the second answer waits for the first, and they come in order once the job runs.
#[test]
fn a_later_answer_never_overtakes_a_late_one() {
    let clock = Arc::new(Stalled::default());
    let device = FakeDevice::with_modem(Modem::new());
    device.clock(clock.clone());
    device.faults(Faults::default().delay(Duration::from_secs(1)));
    let mut port = device.port();

    port.write_all(b"AT+CSQ\r").unwrap();
    *clock.now.lock().unwrap() = Duration::from_secs(1);
    device.faults(Faults::default());
    port.write_all(b"AT\r").unwrap();
    assert_eq!(device.unread(), 0, "the second answer overtook the first");
    let jobs = std::mem::take(&mut *clock.jobs.lock().unwrap());
    jobs.into_iter().for_each(|job| job());
    assert_eq!(
        drain(&mut port, 256, &device).concat(),
        b"\r\n+CSQ: 20,99\r\n\r\nOK\r\n\r\nOK\r\n"
    );
}

/// Given faults whose chunk is set to 0 by its field
/// When the host reads exactly the answer
/// Then it gets it, one byte per read, never an empty read that looks like the end.
#[test]
fn a_zero_chunk_still_reads() {
    let device = FakeDevice::with_modem(Modem::new());
    let mut faults = Faults::default();
    faults.chunk = Some(0);
    device.faults(faults);
    let mut port = device.port();

    port.write_all(b"AT\r").unwrap();
    let mut buf = [0; 6];
    port.read_exact(&mut buf).unwrap();
    assert_eq!(&buf, OK);
}

/// A clock that runs a job at once when it is due already, as the trait allows.
#[derive(Debug)]
struct Eager(ManualClock);

impl serialplugin_testing::Clock for Eager {
    fn now(&self) -> Duration {
        self.0.now()
    }

    fn at(&self, at: Duration, job: serialplugin_testing::Job) {
        if at <= self.0.now() {
            job();
        } else {
            self.0.at(at, job);
        }
    }
}

/// Given a device on a clock that runs due jobs at once, and a port timeout of zero
/// When the host reads with nothing to read
/// Then the read times out at once (the wake job runs inline without deadlocking the device).
#[test]
fn a_clock_that_runs_jobs_at_once_does_not_block_a_read() {
    let device = FakeDevice::new();
    device.clock(Arc::new(Eager(ManualClock::new())));
    let mut port = device.port();
    serialport::SerialPort::set_timeout(&mut port, Duration::ZERO).unwrap();

    let mut buf = [0; 4];
    assert_eq!(port.read(&mut buf).unwrap_err().kind(), ErrorKind::TimedOut);
}

/// Given a device on a manual clock
/// When the test schedules a line for 2 s later and the clock moves on
/// Then the host gets it only once the 2 s are over.
#[test]
fn a_scheduled_line_comes_at_its_time() {
    let device = FakeDevice::new();
    let clock = ManualClock::new();
    device.clock(Arc::new(clock.clone()));
    device.send_after(Duration::from_secs(2), b"\r\nRDY\r\n");
    let mut port = device.port();

    clock.advance(ms(1_999));
    assert_eq!(device.unread(), 0);
    clock.advance(ms(1));
    assert_eq!(drain(&mut port, 64, &device).concat(), b"\r\nRDY\r\n");
}

/// Given two subscribers to the writes of a device, one of which goes away
/// When the host writes three times
/// Then the one left gets each write once, in order, and the writes still succeed.
#[test]
fn every_write_is_an_event() {
    let device = FakeDevice::new();
    let writes = device.subscribe_writes();
    drop(device.subscribe_writes());
    let mut port = device.port();

    for chunk in [&b"AT"[..], b"+CSQ", b"\r"] {
        port.write_all(chunk).unwrap();
    }
    let got: Vec<Vec<u8>> = writes.try_iter().collect();
    assert_eq!(got, [b"AT".to_vec(), b"+CSQ".to_vec(), b"\r".to_vec()]);
}

/// Given faults that are set, then ended
/// When the host sends a command
/// Then the device answers as without faults.
#[test]
fn default_faults_end_the_faults() {
    let device = FakeDevice::with_modem(Modem::new());
    device.faults(Faults::default().silent(true).chunk(1));
    device.faults(Faults::default());
    let mut port = device.port();

    port.write_all(b"AT\r").unwrap();
    assert_eq!(drain(&mut port, 64, &device), [OK.to_vec()]);
}

#[cfg(feature = "tokio")]
mod tokio_clock {
    use super::*;
    use serialplugin_testing::TokioClock;

    /// Given a device on a paused tokio clock, answers 30 s late, port timeout 10 min
    /// When the host (on its own thread, as a port reader runs) sends a command, reads the
    /// answer, then reads again with nothing to come
    /// Then it gets the answer and then a timeout, and no real 30 s or 10 min pass.
    #[tokio::test(start_paused = true)]
    async fn a_paused_tokio_clock_drives_delays_and_timeouts() {
        let device = FakeDevice::with_modem(Modem::new());
        device.clock(Arc::new(TokioClock::new()));
        device.faults(Faults::default().delay(Duration::from_secs(30)));
        let mut writes = device.subscribe_writes_async();
        let mut port = device.port();
        serialport::SerialPort::set_timeout(&mut port, Duration::from_secs(600)).unwrap();
        let (tx, rx) = tokio::sync::oneshot::channel();
        // Not spawn_blocking: a running blocking task stops a paused clock from skipping ahead.
        std::thread::spawn(move || {
            port.write_all(b"AT\r").unwrap();
            let mut buf = [0; 64];
            let n = port.read(&mut buf).unwrap();
            let answer = buf[..n].to_vec();
            let next = port.read(&mut buf).map_err(|e| e.kind());
            tx.send((answer, next)).unwrap();
        });

        assert_eq!(writes.recv().await.unwrap(), b"AT\r");
        let (answer, next) = rx.await.unwrap();
        assert_eq!(answer, OK);
        assert_eq!(next, Err(ErrorKind::TimedOut));
    }
}
