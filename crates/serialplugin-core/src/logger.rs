//! Centralized logging module for the serial plugin
//!
//! This module provides a unified logging interface with configurable log levels.
//! All logging in the plugin should use these macros to respect the global log level setting.
//!
//! On Android, and with the `log` feature anywhere, messages go through the `log`
//! crate (on Android to logcat under tag `SerialPlugin`, see `android_log`).
//! Otherwise they use stdout/stderr (the default).

/// Logs an error message if the current log level permits
#[cfg(any(feature = "log", target_os = "android"))]
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_error() {
            $crate::__log::error!($($arg)*);
        }
    };
}

/// Logs an error message if the current log level permits
#[cfg(not(any(feature = "log", target_os = "android")))]
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_error() {
            eprintln!($($arg)*);
        }
    };
}

/// Logs a warning message if the current log level permits
#[cfg(any(feature = "log", target_os = "android"))]
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_warn() {
            $crate::__log::warn!($($arg)*);
        }
    };
}

/// Logs a warning message if the current log level permits
#[cfg(not(any(feature = "log", target_os = "android")))]
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_warn() {
            println!($($arg)*);
        }
    };
}

/// Logs an info message if the current log level permits
#[cfg(any(feature = "log", target_os = "android"))]
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_info() {
            $crate::__log::info!($($arg)*);
        }
    };
}

/// Logs an info message if the current log level permits
#[cfg(not(any(feature = "log", target_os = "android")))]
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_info() {
            println!($($arg)*);
        }
    };
}

/// Logs a debug message if the current log level permits
#[cfg(any(feature = "log", target_os = "android"))]
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_debug() {
            $crate::__log::debug!($($arg)*);
        }
    };
}

/// Logs a debug message if the current log level permits
#[cfg(not(any(feature = "log", target_os = "android")))]
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_debug() {
            println!($($arg)*);
        }
    };
}
