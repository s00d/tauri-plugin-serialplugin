//! Time of a [`FakeDevice`](crate::FakeDevice): its late answers, scheduled bytes and port
//! timeouts run on a [`Clock`].

use std::collections::BTreeMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Work a clock runs when its time comes.
pub type Job = Box<dyn FnOnce() + Send>;

/// A source of time that runs jobs at a given moment.
///
/// Times are offsets from the clock's own start. Jobs due at the same moment run in the order
/// they were given.
pub trait Clock: Send + Sync + std::fmt::Debug {
    /// Time since the clock started.
    fn now(&self) -> Duration;

    /// Run `job` once [`now`](Self::now) reaches `at` (at once, or soon, when it already has).
    fn at(&self, at: Duration, job: Job);
}

/// Jobs by due time; the sequence number keeps the order of equal times.
#[derive(Default)]
struct Timers {
    jobs: BTreeMap<(Duration, u64), Job>,
    seq: u64,
}

impl Timers {
    fn add(&mut self, at: Duration, job: Job) {
        self.seq += 1;
        self.jobs.insert((at, self.seq), job);
    }

    /// The first job due by `now`.
    fn due(&mut self, now: Duration) -> Option<Job> {
        let first = *self.jobs.keys().next()?;
        (first.0 <= now).then(|| self.jobs.remove(&first)).flatten()
    }

    fn next(&self) -> Option<Duration> {
        self.jobs.keys().next().map(|k| k.0)
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Real time: [`std::time::Instant`]; jobs run on one timer thread per clock, started with
/// the first job. The thread ends when the clock is dropped.
///
/// What a [`FakeDevice`](crate::FakeDevice) without a clock of its own delays answers with.
#[derive(Debug, Clone)]
pub struct SystemClock(Arc<SystemInner>);

struct SystemInner {
    start: Instant,
    timers: Mutex<(Timers, bool)>,
    wake: Condvar,
}

impl std::fmt::Debug for SystemInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SystemClock").finish_non_exhaustive()
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemClock {
    /// A clock that starts now.
    pub fn new() -> Self {
        Self(Arc::new(SystemInner {
            start: Instant::now(),
            timers: Mutex::new((Timers::default(), false)),
            wake: Condvar::new(),
        }))
    }
}

impl Clock for SystemClock {
    fn now(&self) -> Duration {
        self.0.start.elapsed()
    }

    fn at(&self, at: Duration, job: Job) {
        let mut timers = lock(&self.0.timers);
        timers.0.add(at, job);
        if !timers.1 {
            timers.1 = true;
            // The thread holds the clock weakly: dropping the last clock ends it.
            let inner = Arc::downgrade(&self.0);
            std::thread::spawn(move || run_system(inner));
        }
        drop(timers);
        self.0.wake.notify_all();
    }
}

fn run_system(inner: std::sync::Weak<SystemInner>) {
    loop {
        let Some(clock) = inner.upgrade() else {
            return;
        };
        let mut timers = lock(&clock.timers);
        let now = clock.start.elapsed();
        if let Some(job) = timers.0.due(now) {
            drop(timers);
            // A panicking job must not end the thread: the jobs after it would never run. The
            // panic hook has reported it.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(job));
            continue;
        }
        // Wake up for the next job; with none, look again now and then, to see the clock go.
        let wait = timers
            .0
            .next()
            .map_or(Duration::from_millis(100), |next| next - now)
            .min(Duration::from_millis(100));
        let _woken = clock.wake.wait_timeout(timers, wait);
    }
}

/// Time that moves only when the test says so: [`advance`](Self::advance) runs the jobs that
/// became due, in order, on the caller's thread. Clones are the same clock.
///
/// ```
/// use serialplugin_testing::{Clock, ManualClock};
/// use std::sync::{Arc, Mutex};
/// use std::time::Duration;
///
/// let clock = ManualClock::new();
/// let fired = Arc::new(Mutex::new(Vec::new()));
/// let f = fired.clone();
/// clock.at(Duration::from_secs(2), Box::new(move || f.lock().unwrap().push(2)));
/// clock.advance(Duration::from_secs(1));
/// assert!(fired.lock().unwrap().is_empty());
/// clock.advance(Duration::from_secs(1));
/// assert_eq!(*fired.lock().unwrap(), [2]);
/// ```
#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<(Duration, Timers)>>);

impl std::fmt::Debug for ManualClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("ManualClock").field(&self.now()).finish()
    }
}

impl ManualClock {
    /// A clock at zero.
    pub fn new() -> Self {
        Self::default()
    }

    /// Move the time on by `by` and run every job due by then, in order (also the ones those
    /// jobs schedule within it).
    ///
    /// One thread at a time (a job may advance it too): two threads advancing at once could
    /// run jobs side by side.
    pub fn advance(&self, by: Duration) {
        let target = lock(&self.0).0 + by;
        loop {
            let mut state = lock(&self.0);
            let Some(next) = state.1.next().filter(|next| *next <= target) else {
                // A job (or another thread) may have moved it further: time never goes back.
                state.0 = state.0.max(target);
                return;
            };
            // A job sees the time it was due at.
            state.0 = state.0.max(next);
            let now = state.0;
            let job = state.1.due(now);
            drop(state);
            if let Some(job) = job {
                job();
            }
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        lock(&self.0).0
    }

    fn at(&self, at: Duration, job: Job) {
        // Due already: it runs with the next advance, also `advance(Duration::ZERO)`.
        lock(&self.0).1.add(at, job);
    }
}

/// Tokio time (feature `tokio`): [`tokio::time::Instant`], so a paused test clock
/// (`#[tokio::test(start_paused = true)]`) drives the device's delays and nothing waits in
/// real time. Jobs run on the runtime the clock was made in, in due order.
#[cfg(feature = "tokio")]
#[derive(Debug, Clone)]
pub struct TokioClock(Arc<TokioInner>);

#[cfg(feature = "tokio")]
struct TokioInner {
    start: tokio::time::Instant,
    runtime: tokio::runtime::Handle,
    timers: Mutex<Timers>,
    /// Held while jobs run: two wakeups never run jobs side by side or out of order.
    running: Mutex<()>,
}

#[cfg(feature = "tokio")]
impl std::fmt::Debug for TokioInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokioClock").finish_non_exhaustive()
    }
}

#[cfg(feature = "tokio")]
impl TokioClock {
    /// A clock that starts now, on the current runtime; panics outside one.
    // No `Default`: it needs a runtime, which `new` says.
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self(Arc::new(TokioInner {
            start: tokio::time::Instant::now(),
            runtime: tokio::runtime::Handle::current(),
            timers: Mutex::new(Timers::default()),
            running: Mutex::new(()),
        }))
    }
}

#[cfg(feature = "tokio")]
impl Clock for TokioClock {
    fn now(&self) -> Duration {
        // Outside the runtime tokio's `Instant` reads the real time: enter it, also on a port
        // reader's own thread.
        let _runtime = self.0.runtime.enter();
        self.0.start.elapsed()
    }

    fn at(&self, at: Duration, job: Job) {
        lock(&self.0.timers).add(at, job);
        let inner = self.0.clone();
        self.0.runtime.spawn(async move {
            tokio::time::sleep_until(inner.start + at).await;
            let _running = lock(&inner.running);
            loop {
                let job = lock(&inner.timers).due(inner.start.elapsed());
                let Some(job) = job else { break };
                job();
            }
        });
    }
}
