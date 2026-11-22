//! FIFO turnstile for read-until transactions on one port (desktop).

use crate::at::session::AtSessionConfig;
use crate::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

const CANCELLED_MSG: &str = "transaction queue cancelled";

/// Serializes exchange / AT jobs on one port. Concurrent callers wait in FIFO order.
pub struct PortTxQueue {
    inner: Mutex<Inner>,
    turn: Condvar,
    at_session: Mutex<AtSessionConfig>,
    /// Port-wide exchange cancel flag. Reset when a job *takes its turn*, not when
    /// each exchange starts, so a cancel landing between the phases of one job
    /// (`at_phases`, `send_sms_pdu`) is not wiped before the next phase.
    cancel_flag: Arc<AtomicBool>,
}

struct Inner {
    next_ticket: u64,
    now_serving: u64,
    /// Tickets below this value were cancelled and must not run.
    cancel_below: u64,
    /// A job holds the turn right now.
    running: bool,
}

impl Default for PortTxQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl PortTxQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_ticket: 0,
                now_serving: 0,
                cancel_below: 0,
                running: false,
            }),
            turn: Condvar::new(),
            at_session: Mutex::new(AtSessionConfig::default()),
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel_flag.clone()
    }

    pub fn configure_at_session(&self, session: AtSessionConfig) {
        *crate::sync_util::lock_or_recover(&self.at_session) = session;
    }

    pub fn at_session(&self) -> AtSessionConfig {
        crate::sync_util::lock_or_recover(&self.at_session).clone()
    }

    /// Reject every job queued so far. The running job (if any) is stopped through
    /// the port's cancel flag by the caller; jobs submitted after this call run normally.
    pub fn cancel_all(&self) {
        let mut inner = crate::sync_util::lock_or_recover(&self.inner);
        inner.cancel_below = inner.next_ticket;
        if !inner.running {
            inner.now_serving = inner.next_ticket;
        }
        self.turn.notify_all();
    }

    /// Run `f` when this caller's turn arrives (FIFO).
    pub fn run_serial<F, T>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce() -> Result<T, Error>,
    {
        let mut inner = crate::sync_util::lock_or_recover(&self.inner);
        let ticket = inner.next_ticket;
        inner.next_ticket += 1;
        while inner.now_serving != ticket {
            if ticket < inner.cancel_below {
                return Err(Error::String(CANCELLED_MSG.into()));
            }
            inner = self.turn.wait(inner).unwrap_or_else(|e| e.into_inner());
        }
        inner.running = true;
        self.cancel_flag.store(false, Ordering::SeqCst);
        drop(inner);

        // Release the turn even if `f` panics, so the queue never wedges.
        struct Turn<'a>(&'a PortTxQueue);
        impl Drop for Turn<'_> {
            fn drop(&mut self) {
                let mut inner = crate::sync_util::lock_or_recover(&self.0.inner);
                inner.running = false;
                inner.now_serving = (inner.now_serving + 1).max(inner.cancel_below);
                self.0.turn.notify_all();
            }
        }
        let _turn = Turn(self);
        f()
    }
}

#[cfg(test)]
mod model {
    //! In `src/`: it reads the private ticket counter to know a job is queued before the
    //! next step.
    //!
    //! Model test: jobs from several callers that return, fail or panic, releases and cancels
    //! in random order, checked against a FIFO model after every step (no two jobs at once, a
    //! job queued after a cancel waits for the running one, the cancel flag lasts for its job).
    use super::*;
    use proptest::collection::vec;
    use proptest::prelude::*;
    use proptest::test_runner::{Config, TestRunner};
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};

    #[derive(Clone, Debug)]
    pub(super) enum Step {
        /// A caller queues a job (it runs until released, then ends this way).
        Submit(End),
        /// The running job returns.
        Release,
        CancelAll,
        /// The port's cancel flag is set (`cancel_exchange`): it lasts for the running job.
        SetFlag,
    }

    #[derive(Clone, Copy, Debug)]
    pub(super) enum End {
        Ok,
        Err,
        Panic,
    }

    #[derive(Clone, Debug, Default)]
    pub(super) struct Model {
        next: u32,
        running: Option<u32>,
        waiting: VecDeque<u32>,
        ends: Vec<End>,
        started: Vec<u32>,
        flag: bool,
        /// Job id and result (`None`: rejected by a cancel, failed or panicked).
        done: Vec<(u32, Option<u32>)>,
    }

    fn step_strategy() -> impl Strategy<Value = Step> {
        let end = prop::sample::select(vec![End::Ok, End::Ok, End::Err, End::Panic]);
        prop_oneof![
            3 => end.prop_map(Step::Submit),
            2 => Just(Step::Release),
            1 => Just(Step::CancelAll),
            1 => Just(Step::SetFlag),
        ]
    }

    impl Model {
        /// A release needs a running job (else the runner skips the step).
        fn observable(&self, step: &Step) -> bool {
            !matches!(step, Step::Release) || self.running.is_some()
        }

        fn apply(&mut self, step: &Step) {
            let m = self;
            match step {
                Step::Submit(end) => {
                    m.next += 1;
                    m.ends.push(*end);
                    if m.running.is_none() {
                        m.running = Some(m.next);
                        m.started.push(m.next);
                        m.flag = false;
                    } else {
                        m.waiting.push_back(m.next);
                    }
                }
                Step::Release => {
                    let id = m.running.take().unwrap();
                    let ok = matches!(m.ends[id as usize - 1], End::Ok);
                    m.done.push((id, ok.then_some(id)));
                    m.running = m.waiting.pop_front();
                    m.started.extend(m.running);
                    if m.running.is_some() {
                        m.flag = false;
                    }
                }
                Step::CancelAll => {
                    let rejected: Vec<_> = m.waiting.drain(..).map(|id| (id, None)).collect();
                    m.done.extend(rejected);
                }
                Step::SetFlag => m.flag = true,
            }
        }
    }

    /// A job id and its result (`None`: rejected by a cancel).
    type Outcome = (u32, Option<u32>);

    pub(super) struct Sut {
        q: Arc<PortTxQueue>,
        gates: Vec<Sender<()>>,
        started: (Sender<u32>, Receiver<u32>),
        started_seen: Vec<u32>,
        done: (Sender<Outcome>, Receiver<Outcome>),
        done_seen: Vec<(u32, Option<u32>)>,
        active: Arc<AtomicUsize>,
        overlap: Arc<AtomicBool>,
        threads: Vec<JoinHandle<()>>,
    }

    /// Tickets handed out so far (grows when a job is queued).
    fn last_turn(q: &PortTxQueue) -> u64 {
        crate::sync_util::lock_or_recover(&q.inner).next_ticket
    }

    fn sorted<T: Ord + Clone>(v: &[T]) -> Vec<T> {
        let mut v = v.to_vec();
        v.sort();
        v
    }

    impl Sut {
        fn collect(&mut self, m: &Model) {
            let wait = Duration::from_secs(3);
            while self.started_seen.len() < m.started.len() {
                self.started_seen
                    .push(self.started.1.recv_timeout(wait).expect("job start"));
            }
            while self.done_seen.len() < m.done.len() {
                self.done_seen
                    .push(self.done.1.recv_timeout(wait).expect("job result"));
            }
        }
    }

    impl Sut {
        fn new() -> Sut {
            Sut {
                q: Arc::new(PortTxQueue::new()),
                gates: Vec::new(),
                started: mpsc::channel(),
                started_seen: Vec::new(),
                done: mpsc::channel(),
                done_seen: Vec::new(),
                active: Arc::default(),
                overlap: Arc::default(),
                threads: Vec::new(),
            }
        }

        fn apply(&mut self, m: &Model, step: Step) {
            let sut = self;
            match step {
                Step::Submit(end) => {
                    let id = m.next;
                    let (gate, gate_rx) = mpsc::channel::<()>();
                    sut.gates.push(gate);
                    let (q, started, done) =
                        (sut.q.clone(), sut.started.0.clone(), sut.done.0.clone());
                    let (active, overlap) = (sut.active.clone(), sut.overlap.clone());
                    let before = last_turn(&sut.q);
                    sut.threads.push(thread::spawn(move || {
                        let job = || {
                            if active.fetch_add(1, Ordering::SeqCst) != 0 {
                                overlap.store(true, Ordering::SeqCst);
                            }
                            started.send(id).unwrap();
                            let _ = gate_rx.recv();
                            active.fetch_sub(1, Ordering::SeqCst);
                            match end {
                                End::Ok => Ok(id),
                                End::Err => Err(Error::String("boom".into())),
                                End::Panic => std::panic::panic_any("expected by this test"),
                            }
                        };
                        let run = std::panic::AssertUnwindSafe(|| q.run_serial(job));
                        let r = std::panic::catch_unwind(run);
                        done.send((id, r.ok().and_then(Result::ok))).unwrap();
                    }));
                    // Registered before the next step, so FIFO order is the step order.
                    let deadline = Instant::now() + Duration::from_secs(3);
                    while last_turn(&sut.q) == before {
                        assert!(Instant::now() < deadline, "job {id} never queued");
                        thread::yield_now();
                    }
                }
                Step::Release => {
                    let id = m.done.last().unwrap().0;
                    sut.gates[id as usize - 1].send(()).unwrap();
                }
                Step::CancelAll => sut.q.cancel_all(),
                Step::SetFlag => sut.q.cancel_flag().store(true, Ordering::SeqCst),
            }
            sut.collect(m);
        }

        fn check(&self, m: &Model) {
            let sut = self;
            assert!(!sut.overlap.load(Ordering::SeqCst), "two jobs ran at once");
            assert_eq!(sut.started_seen, m.started, "start order");
            // Rejections arrive from several threads: compare as sets.
            assert_eq!(sorted(&sut.done_seen), sorted(&m.done), "results");
            assert_eq!(sut.q.cancel_flag().load(Ordering::SeqCst), m.flag, "flag");
        }

        fn teardown(mut self, mut m: Model) {
            let sut = &mut self;
            // Release every job still running or waiting, in turn.
            while m.running.is_some() {
                m.apply(&Step::Release);
                sut.apply(&m, Step::Release);
                sut.check(&m);
            }
            for t in sut.threads.drain(..) {
                t.join().unwrap();
            }
            assert!(!sut.overlap.load(Ordering::SeqCst), "two jobs ran at once");
        }
    }

    /// Random submits, releases and cancels against the FIFO model.
    #[test]
    fn jobs_run_one_at_a_time_in_fifo_order() {
        let mut runner = TestRunner::new(Config {
            cases: 32,
            failure_persistence: None,
            ..Config::default()
        });
        let result = runner.run(&vec(step_strategy(), 1..24), |steps| {
            let mut model = Model::default();
            let mut sut = Sut::new();
            for step in steps {
                if !model.observable(&step) {
                    continue;
                }
                model.apply(&step);
                sut.apply(&model, step);
                sut.check(&model);
            }
            sut.teardown(model);
            Ok(())
        });
        if let Err(e) = result {
            panic!("{e}");
        }
    }
}
