#![cfg(test)]
#![allow(dead_code)]

#[path = "../../../crates/serialplugin-core/src/error.rs"]
mod error;
#[path = "../../../crates/serialplugin-core/src/android/fd_bridge.rs"]
mod fd_bridge;
#[path = "../../../crates/serialplugin-core/src/jni_ready.rs"]
mod jni_ready;

#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { eprintln!($($arg)*); };
}
