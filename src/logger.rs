//! Centralized logging module for the serial plugin
//! 
//! This module provides a unified logging interface with configurable log levels.
//! All logging in the plugin should use these macros to respect the global log level setting.

/// Logs an error message if the current log level permits
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_error() {
            eprintln!($($arg)*);
        }
    };
}

/// Logs a warning message if the current log level permits
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_warn() {
            println!($($arg)*);
        }
    };
}

/// Logs an info message if the current log level permits
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_info() {
            println!($($arg)*);
        }
    };
}

/// Logs a debug message if the current log level permits
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => {
        if $crate::state::get_log_level().should_log_debug() {
            println!($($arg)*);
        }
    };
}

#[cfg(test)]
mod tests {
    use crate::state::{set_log_level, LogLevel};

    #[test]
    fn test_log_level_none() {
        set_log_level(LogLevel::None);
        // These should not panic, just not print anything
        log_error!("This should not be printed");
        log_warn!("This should not be printed");
        log_info!("This should not be printed");
        log_debug!("This should not be printed");
    }

    #[test]
    fn test_log_level_error() {
        set_log_level(LogLevel::Error);
        log_error!("Error message");
        // Warn, Info, Debug should not print
    }

    #[test]
    fn test_log_level_debug() {
        set_log_level(LogLevel::Debug);
        log_error!("Error message");
        log_warn!("Warning message");
        log_info!("Info message");
        log_debug!("Debug message");
    }
}

