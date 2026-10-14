#![cfg(test)]
#![allow(dead_code)]

#[path = "../../../src/error.rs"]
mod error;
#[path = "../../../src/android/fd_bridge.rs"]
mod fd_bridge;
#[path = "../../../src/jni_ready.rs"]
mod jni_ready;

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { eprintln!($($arg)*); };
}
