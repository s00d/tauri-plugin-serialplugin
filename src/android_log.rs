//! Android logcat sink. Tag: `SerialPlugin` (visible with `adb logcat -s SerialPlugin`).

use std::ffi::CString;
use std::os::raw::{c_char, c_int};

use log::{Level, LevelFilter, Log, Metadata, Record};

const TAG: &str = "SerialPlugin";

const ANDROID_LOG_ERROR: c_int = 6;
const ANDROID_LOG_WARN: c_int = 5;
const ANDROID_LOG_INFO: c_int = 4;
const ANDROID_LOG_DEBUG: c_int = 3;

extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

struct AndroidLogger;

impl Log for AndroidLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= Level::Debug
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let prio = match record.level() {
            Level::Error => ANDROID_LOG_ERROR,
            Level::Warn => ANDROID_LOG_WARN,
            Level::Info => ANDROID_LOG_INFO,
            Level::Debug | Level::Trace => ANDROID_LOG_DEBUG,
        };
        let Ok(tag) = CString::new(TAG) else {
            return;
        };
        let msg = format!("{}", record.args()).replace('\0', "");
        let Ok(text) = CString::new(msg) else {
            return;
        };
        unsafe {
            __android_log_write(prio, tag.as_ptr(), text.as_ptr());
        }
    }

    fn flush(&self) {}
}

/// Install once; subsequent calls are no-ops.
pub fn init() {
    static INIT: std::sync::Once = std::sync::Once::new();
    INIT.call_once(|| {
        let _ = log::set_boxed_logger(Box::new(AndroidLogger));
        log::set_max_level(LevelFilter::Debug);
        log::info!("android logcat sink ready (tag={TAG})");
    });
}
