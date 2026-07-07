//! Mutex helpers for background threads (recover from poison instead of panicking).

use std::sync::{Mutex, MutexGuard};

/// Lock a mutex, recovering the inner value if the mutex was poisoned.
pub fn lock_or_recover<'a, T>(mutex: &'a Mutex<T>) -> MutexGuard<'a, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
