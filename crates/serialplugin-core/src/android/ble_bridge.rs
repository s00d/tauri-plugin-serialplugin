//! JNI calls into Kotlin `BleNative` (static methods), feature `ble`.
//! The class is cached by `BleNative.nativeInit` on a Java thread, where the app
//! class loader is in scope (FindClass on a Rust thread would not see it).

use crate::android::fd_bridge::with_env;
use crate::error::Error;
use crate::port::ble_android::BleOps;
use jni::objects::{GlobalRef, JObject, JString, JValue};
use jni::JNIEnv;
use std::io;
use std::sync::OnceLock;

static CLASS: OnceLock<GlobalRef> = OnceLock::new();

pub(crate) fn init_class(env: &mut JNIEnv, class: &JObject) {
    if CLASS.get().is_some() {
        return;
    }
    match env.new_global_ref(class) {
        Ok(g) => {
            let _ = CLASS.set(g);
        }
        Err(e) => crate::log_error!("BleNative init_class new_global_ref failed: {e}"),
    }
}

fn class() -> Result<&'static GlobalRef, Error> {
    CLASS
        .get()
        .ok_or_else(|| Error::new("BLE: BleNative not bound (SerialPlugin.load not run)"))
}

fn io_err(e: Error) -> io::Error {
    io::Error::other(format!("BLE: {e}"))
}

pub fn call_scan_json(timeout_ms: u64) -> Result<String, Error> {
    with_env(|env| {
        let ms = i64::try_from(timeout_ms).unwrap_or(i64::MAX);
        let v =
            env.call_static_method(class()?, "scanJson", "(J)Ljava/lang/String;", &[ms.into()])?;
        let jstr = JString::from(v.l()?);
        let out: String = env.get_string(&jstr)?.into();
        Ok(out)
    })
}

fn call_connect(target: &str, handle: i64, timeout_ms: u64) -> io::Result<()> {
    with_env(|env| {
        let t = env.new_string(target)?;
        let ms = i64::try_from(timeout_ms).unwrap_or(i64::MAX);
        env.call_static_method(
            class()?,
            "connect",
            "(Ljava/lang/String;JJ)V",
            &[JValue::Object(&JObject::from(t)), handle.into(), ms.into()],
        )?;
        Ok(())
    })
    .map_err(io_err)
}

fn call_write(handle: i64, data: &[u8]) -> io::Result<usize> {
    with_env(|env| {
        let arr = env.byte_array_from_slice(data)?;
        let v = env.call_static_method(
            class()?,
            "write",
            "(J[B)I",
            &[handle.into(), JValue::Object(&JObject::from(arr))],
        )?;
        Ok(usize::try_from(v.i()?).unwrap_or(0))
    })
    .map_err(|e| io::Error::new(io::ErrorKind::BrokenPipe, format!("BLE: {e}")))
}

fn call_close(handle: i64) {
    let r = with_env(|env| {
        env.call_static_method(class()?, "close", "(J)V", &[handle.into()])?;
        Ok(())
    });
    if let Err(e) = r {
        crate::log_warn!("[BLE] close {handle}: {e}");
    }
}

pub(crate) static JNI_OPS: BleOps = BleOps {
    connect: call_connect,
    write: call_write,
    close: call_close,
};
