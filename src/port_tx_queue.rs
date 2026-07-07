//! FIFO turnstile for read-until transactions on one port (desktop).

use crate::at_session::AtSessionConfig;
use crate::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex};

const CANCELLED_MSG: &str = "transaction queue cancelled";

/// Serializes exchange / AT jobs on one port. Concurrent callers wait in FIFO order.
pub struct PortTxQueue {
    inner: Mutex<Inner>,
    turn: Condvar,
    halted: AtomicBool,
    at_session: Mutex<AtSessionConfig>,
}

struct Inner {
    next_ticket: u64,
    now_serving: u64,
    /// When set, waiters receive cancelled error.
    drain_waiters: bool,
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
                drain_waiters: false,
            }),
            turn: Condvar::new(),
            halted: AtomicBool::new(false),
            at_session: Mutex::new(AtSessionConfig::default()),
        }
    }

    pub fn configure_at_session(&self, session: AtSessionConfig) {
        *self.at_session.lock().unwrap() = session;
    }

    pub fn at_session(&self) -> AtSessionConfig {
        self.at_session.lock().unwrap().clone()
    }

    pub fn at_session_mut<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&mut AtSessionConfig) -> R,
    {
        f(&mut self.at_session.lock().unwrap())
    }

    /// Cancel in-flight exchange (via port flag) and reject all queued waiters.
    pub fn cancel_all(&self) {
        let mut inner = self.inner.lock().unwrap();
        inner.drain_waiters = true;
        inner.now_serving = inner.next_ticket;
        self.turn.notify_all();
    }

    pub fn clear_halt(&self) {
        self.halted.store(false, Ordering::SeqCst);
        self.inner.lock().unwrap().drain_waiters = false;
    }

    /// Run `f` when this caller's turn arrives (FIFO).
    pub fn run_serial<F, T>(&self, f: F) -> Result<T, Error>
    where
        F: FnOnce() -> Result<T, Error>,
    {
        let ticket = {
            let mut inner = self.inner.lock().map_err(lock_err)?;
            if inner.drain_waiters {
                return Err(Error::String(CANCELLED_MSG.into()));
            }
            let t = inner.next_ticket;
            inner.next_ticket += 1;
            t
        };

        let mut inner = self.inner.lock().map_err(lock_err)?;
        while inner.now_serving != ticket {
            if inner.drain_waiters || self.halted.load(Ordering::SeqCst) {
                return Err(Error::String(CANCELLED_MSG.into()));
            }
            inner = self
                .turn
                .wait(inner)
                .map_err(|e| Error::String(format!("queue wait failed: {e}")))?;
        }
        drop(inner);

        let session = self.at_session();
        let result = f();

        if result.is_err() && session.stop_on_error() {
            self.halted.store(true, Ordering::SeqCst);
            let mut inner = self.inner.lock().map_err(lock_err)?;
            inner.drain_waiters = true;
        }

        {
            let mut inner = self.inner.lock().map_err(lock_err)?;
            inner.now_serving += 1;
            if inner.drain_waiters {
                inner.now_serving = inner.next_ticket;
                if !self.halted.load(Ordering::SeqCst) {
                    inner.drain_waiters = false;
                }
            }
        }
        self.turn.notify_all();

        result
    }
}

fn lock_err<T>(e: std::sync::PoisonError<T>) -> Error {
    Error::String(format!("Mutex lock failed: {e}"))
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
    fn cancel_all_then_clear_halt_allows_next_job() {
        let q = Arc::new(PortTxQueue::new());
        q.cancel_all();
        q.clear_halt();
        let result = q.run_serial(|| Ok(42));
        assert_eq!(result.unwrap(), 42);
    }
}
