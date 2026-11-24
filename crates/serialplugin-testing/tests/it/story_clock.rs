//! Clocks: jobs run when their time comes, in due order; a manual clock moves only on
//! `advance`, a paused tokio clock skips ahead without real waiting.

use serialplugin_testing::{Clock, ManualClock, SystemClock, WAIT};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// Given jobs at 30, 10, 10 and 20 ms, one of which schedules another within the window
/// When the manual clock moves on in steps
/// Then each step runs exactly the jobs due by then, earliest first, equal times in the order
/// they were given, and a job sees the time it was due at.
#[test]
fn a_manual_clock_runs_due_jobs_in_order() {
    let clock = ManualClock::new();
    let ran = Arc::new(Mutex::new(Vec::new()));
    let job = |name: &'static str| {
        let (ran, clock) = (ran.clone(), clock.clone());
        Box::new(move || ran.lock().unwrap().push((name, clock.now())))
    };
    clock.at(ms(30), job("c"));
    clock.at(ms(10), job("a1"));
    clock.at(ms(10), job("a2"));
    {
        let (ran, inner) = (ran.clone(), clock.clone());
        clock.at(
            ms(20),
            Box::new(move || {
                ran.lock().unwrap().push(("b", inner.now()));
                let (ran, at) = (ran.clone(), inner.clone());
                inner.at(
                    ms(25),
                    Box::new(move || ran.lock().unwrap().push(("b+", at.now()))),
                );
            }),
        );
    }

    clock.advance(ms(9));
    assert!(ran.lock().unwrap().is_empty());
    clock.advance(ms(16));
    assert_eq!(
        *ran.lock().unwrap(),
        [
            ("a1", ms(10)),
            ("a2", ms(10)),
            ("b", ms(20)),
            ("b+", ms(25))
        ]
    );
    assert_eq!(clock.now(), ms(25));
    clock.advance(ms(5));
    assert_eq!(ran.lock().unwrap().last(), Some(&("c", ms(30))));
}

/// Given a job already due on a manual clock
/// When the clock is advanced by nothing
/// Then it runs.
#[test]
fn a_due_job_runs_on_a_zero_advance() {
    let clock = ManualClock::new();
    clock.advance(ms(5));
    let (tx, rx) = mpsc::channel();
    clock.at(ms(1), Box::new(move || tx.send(()).unwrap()));
    assert!(rx.try_recv().is_err());
    clock.advance(Duration::ZERO);
    rx.try_recv().unwrap();
}

/// Given the real clock, held by a first job until two more are given in reverse due order
/// When their time comes
/// Then they run, earliest first. The gate makes both due when the thread looks, so the order
/// does not hang on how fast the test thread is.
#[test]
fn the_system_clock_runs_jobs_in_due_order() {
    let clock = SystemClock::new();
    let (tx, rx) = mpsc::channel();
    let (gate_tx, gate) = mpsc::channel::<()>();
    let (started_tx, started) = mpsc::channel();
    clock.at(
        Duration::ZERO,
        Box::new(move || {
            started_tx.send(()).unwrap();
            gate.recv_timeout(WAIT).unwrap();
        }),
    );
    started.recv_timeout(WAIT).unwrap();
    let (late, early) = (tx.clone(), tx);
    clock.at(ms(2), Box::new(move || late.send("late").unwrap()));
    clock.at(ms(1), Box::new(move || early.send("early").unwrap()));
    // Both due before the gate opens.
    while clock.now() < ms(2) {
        std::thread::yield_now();
    }
    gate_tx.send(()).unwrap();
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), "early");
    assert_eq!(rx.recv_timeout(WAIT).unwrap(), "late");
}

/// Given the real clock and a job that panics
/// When a later job comes due
/// Then it still runs: the panic does not end the clock's thread.
#[test]
fn the_system_clock_survives_a_panicking_job() {
    let clock = SystemClock::new();
    let (tx, rx) = mpsc::channel();
    clock.at(
        Duration::ZERO,
        Box::new(|| panic!("a job of the test panics")),
    );
    clock.at(Duration::ZERO, Box::new(move || tx.send(()).unwrap()));
    rx.recv_timeout(WAIT).unwrap();
}

/// Given a job that advances the manual clock it runs on further than the advance it runs in
/// When that outer advance ends
/// Then the clock stays at the later time: it never goes back.
#[test]
fn a_manual_clock_never_goes_back() {
    let clock = ManualClock::new();
    let inner = clock.clone();
    clock.at(
        Duration::from_secs(1),
        Box::new(move || inner.advance(Duration::from_secs(100))),
    );
    clock.advance(Duration::from_secs(10));
    assert_eq!(clock.now(), Duration::from_secs(101));
}

#[cfg(feature = "tokio")]
mod tokio_clock {
    use super::*;
    use serialplugin_testing::TokioClock;

    /// Given a paused tokio clock and jobs an hour away, two at the same time
    /// When the test awaits them
    /// Then they run in due order once the clock skips there, no real hour passes.
    #[tokio::test(start_paused = true)]
    async fn a_paused_tokio_clock_skips_to_the_jobs() {
        let clock = TokioClock::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        for (at, name) in [(3_600_001, "c"), (3_600_000, "a"), (3_600_000, "b")] {
            let tx = tx.clone();
            clock.at(ms(at), Box::new(move || tx.send(name).unwrap()));
        }
        drop(tx);
        let mut got = Vec::new();
        while let Some(name) = rx.recv().await {
            got.push(name);
        }
        assert_eq!(got, ["a", "b", "c"]);
        assert!(clock.now() >= ms(3_600_001));
    }
}
