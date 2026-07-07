//! Mutex helpers for background threads (recover from poison instead of panicking).

use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering the inner value if the mutex was poisoned.
pub fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

/// Serialize PTY integration tests (parallel `TTYPort::pair()` races on macOS).
#[cfg(all(test, unix, not(target_os = "android")))]
static PTY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[cfg(all(test, unix, not(target_os = "android")))]
pub fn pty_pair_locked() -> (
    MutexGuard<'static, ()>,
    serialport::TTYPort,
    serialport::TTYPort,
) {
    let guard = lock_or_recover(&PTY_TEST_LOCK);
    let (master, slave) = serialport::TTYPort::pair().expect("pty pair");
    (guard, master, slave)
}
