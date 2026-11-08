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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn serializes_concurrent_jobs() {
        let q = Arc::new(PortTxQueue::new());
        let max_active = Arc::new(AtomicU32::new(0));
        let peak = Arc::new(AtomicU32::new(0));
        let completed = Arc::new(AtomicU32::new(0));
        let mut handles = vec![];
        for _ in 0..3u32 {
            let q = q.clone();
            let max_active = max_active.clone();
            let peak = peak.clone();
            let completed = completed.clone();
            handles.push(thread::spawn(move || {
                q.run_serial(|| {
                    let active = max_active.fetch_add(1, Ordering::SeqCst) + 1;
                    peak.fetch_max(active, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(10));
                    max_active.fetch_sub(1, Ordering::SeqCst);
                    completed.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(completed.load(Ordering::SeqCst), 3);
        assert_eq!(peak.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cancel_drains_waiters() {
        let q = Arc::new(PortTxQueue::new());
        let gate = Arc::new(Mutex::new(false));
        let q1 = q.clone();
        let gate1 = gate.clone();
        let t1 = thread::spawn(move || {
            q1.run_serial(|| {
                *gate1.lock().unwrap() = true;
                thread::sleep(Duration::from_millis(200));
                Ok(())
            })
        });
        while !*gate.lock().unwrap() {
            thread::sleep(Duration::from_millis(5));
        }
        let q2 = q.clone();
        let t2 = thread::spawn(move || q2.run_serial(|| Ok(())));
        thread::sleep(Duration::from_millis(20));
        q.cancel_all();
        let r2 = t2.join().unwrap();
        assert!(r2.is_err());
        let _ = t1.join();
    }

    #[test]
    fn cancel_all_then_next_job_runs() {
        let q = Arc::new(PortTxQueue::new());
        q.cancel_all();
        let result = q.run_serial(|| Ok(42));
        assert_eq!(result.unwrap(), 42);
    }

    /// Regression: cancelling while a job runs used to push `now_serving` past every
    /// future ticket, so the next job on the port waited forever.
    #[test]
    fn cancel_during_running_job_does_not_wedge_queue() {
        let q = Arc::new(PortTxQueue::new());
        let started = Arc::new(std::sync::Barrier::new(2));
        let (q1, s1) = (q.clone(), started.clone());
        let t1 = thread::spawn(move || {
            q1.run_serial(|| {
                s1.wait();
                thread::sleep(Duration::from_millis(50));
                Ok(())
            })
        });
        started.wait();
        q.cancel_all();
        t1.join().unwrap().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let q2 = q.clone();
        thread::spawn(move || tx.send(q2.run_serial(|| Ok(7))).unwrap());
        let r = rx
            .recv_timeout(Duration::from_secs(2))
            .expect("next job never ran");
        assert_eq!(r.unwrap(), 7);
    }

    /// A cancel during one job stays visible for the rest of that job (later
    /// phases) and is cleared only when the next job takes its turn.
    #[test]
    fn cancel_flag_survives_within_job_and_resets_for_next() {
        let q = PortTxQueue::new();
        let flag = q.cancel_flag();
        q.run_serial(|| {
            flag.store(true, Ordering::SeqCst);
            assert!(flag.load(Ordering::SeqCst));
            Ok(())
        })
        .unwrap();
        q.run_serial(|| {
            assert!(!flag.load(Ordering::SeqCst));
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn error_in_job_does_not_halt_queue() {
        let q = Arc::new(PortTxQueue::new());
        let r1: Result<i32, Error> = q.run_serial(|| Err(Error::String("boom".into())));
        assert!(r1.is_err());
        let r2 = q.run_serial(|| Ok(99));
        assert_eq!(r2.unwrap(), 99);
    }
}
