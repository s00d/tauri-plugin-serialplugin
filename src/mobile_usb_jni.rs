//! JNI bridge: Rust USB I/O → Kotlin [UsbNative].

#[cfg(target_os = "android")]
use crate::error::Error;
#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JByteArray, JClass, JObject, JString, JValue, JValueOwned};
#[cfg(target_os = "android")]
use jni::sys::{jboolean, jint};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
#[cfg(target_os = "android")]
use std::convert::TryFrom;
#[cfg(target_os = "android")]
use std::sync::OnceLock;

#[cfg(target_os = "android")]
static JVM: OnceLock<JavaVM> = OnceLock::new();

#[cfg(target_os = "android")]
struct UsbJniCache {
    class: GlobalRef,
}

#[cfg(target_os = "android")]
static CACHE: OnceLock<UsbJniCache> = OnceLock::new();

#[cfg(target_os = "android")]
fn not_init() -> Error {
    Error::new("JNI not initialized (UsbNative.bind not called)")
}

#[cfg(target_os = "android")]
fn with_env<T, F>(f: F) -> Result<T, Error>
where
    F: FnOnce(&mut JNIEnv) -> Result<T, Error>,
{
    let vm = JVM.get().ok_or_else(not_init)?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| Error::new(format!("JNI attach failed: {e}")))?;
    let out = f(&mut env)?;
    map_exception(&mut env, "USB operation failed")?;
    Ok(out)
}

#[cfg(target_os = "android")]
fn map_exception(env: &mut JNIEnv, fallback: &str) -> Result<(), Error> {
    if !env.exception_check().map_err(|e| Error::new(e.to_string()))? {
        return Ok(());
    }
    let msg = env
        .exception_occurred()
        .ok()
        .and_then(|exc| {
            let _ = env.exception_clear();
            let jmsg = env
                .call_method(&exc, "getMessage", "()Ljava/lang/String;", &[])
                .ok()?;
            match jmsg {
                JValueOwned::Object(obj) => {
                    if obj.is_null() {
                        return Some(fallback.to_string());
                    }
                    let jstr: &JString = obj.as_ref().into();
                    env.get_string(jstr).ok().map(|s| s.into())
                }
                _ => None,
            }
        })
        .unwrap_or_else(|| fallback.to_string());
    Err(Error::new(msg))
}

#[cfg(target_os = "android")]
fn cache() -> Result<&'static UsbJniCache, Error> {
    CACHE.get().ok_or_else(not_init)
}

#[cfg(target_os = "android")]
fn usb_class(cache: &'static UsbJniCache) -> &'static JClass<'static> {
    cache.class.as_obj().into()
}

#[cfg(target_os = "android")]
fn jstring<'a>(env: &mut JNIEnv<'a>, s: &str) -> Result<JString<'a>, Error> {
    env.new_string(s)
        .map_err(|e| Error::new(format!("JNI new_string: {e}")))
}

#[cfg(target_os = "android")]
fn jint_value(value: JValueOwned<'_>, label: &str) -> Result<jint, Error> {
    jint::try_from(value).map_err(|e| Error::new(format!("{label}: {e}")))
}

#[cfg(target_os = "android")]
fn bool_value(value: JValueOwned<'_>, label: &str) -> Result<bool, Error> {
    let v: jboolean = jboolean::try_from(value)
        .map_err(|e| Error::new(format!("{label}: {e}")))?;
    Ok(v != 0)
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_UsbNative_nativeInit(
    env: JNIEnv,
    class: JClass,
) {
    let Ok(vm) = env.get_java_vm() else {
        return;
    };
    let _ = JVM.set(vm);
    let Ok(global) = env.new_global_ref(class) else {
        return;
    };
    let _ = CACHE.set(UsbJniCache { class: global });
}

#[cfg(target_os = "android")]
pub fn call_enumerate_json() -> Result<String, Error> {
    with_env(|env| {
        let cache = cache()?;
        let json = env
            .call_static_method(usb_class(cache), "enumerateJson", "()Ljava/lang/String;", &[])
            .map_err(|e| Error::new(format!("enumerateJson: {e}")))?;
        match json {
            JValueOwned::Object(obj) => {
                if obj.is_null() {
                    return Err(Error::new("enumerateJson returned null"));
                }
                let jstr: &JString = obj.as_ref().into();
                env.get_string(jstr)
                    .map(|s| s.into())
                    .map_err(|e| Error::new(format!("enumerateJson decode: {e}")))
            }
            _ => Err(Error::new("enumerateJson returned non-string")),
        }
    })
}

#[cfg(target_os = "android")]
pub fn call_open(
    path: &str,
    baud_rate: u32,
    data_bits: u8,
    flow_control: u8,
    parity: u8,
    stop_bits: u8,
    timeout: u64,
) -> Result<(), Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        env.call_static_method(
            usb_class(cache),
            "open",
            "(Ljava/lang/String;IIIIII)V",
            &[
                JValue::Object(&jpath),
                JValue::Int(baud_rate as i32),
                JValue::Int(data_bits as i32),
                JValue::Int(flow_control as i32),
                JValue::Int(parity as i32),
                JValue::Int(stop_bits as i32),
                JValue::Int(timeout as i32),
            ],
        )
        .map_err(|e| Error::new(format!("open: {e}")))?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
pub fn call_close(path: Option<&str>) -> Result<(), Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath: JObject = match path {
            Some(p) => jstring(env, p)?.into(),
            None => JObject::null(),
        };
        env.call_static_method(
            usb_class(cache),
            "close",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&jpath)],
        )
        .map_err(|e| Error::new(format!("close: {e}")))?;
        Ok(())
    })
}

#[cfg(target_os = "android")]
pub fn call_write(path: &str, data: &[u8]) -> Result<usize, Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        let jdata = env
            .byte_array_from_slice(data)
            .map_err(|e| Error::new(format!("write byte array: {e}")))?;
        let n = env
            .call_static_method(
                usb_class(cache),
                "write",
                "(Ljava/lang/String;[B)I",
                &[JValue::Object(&jpath), JValue::Object(&jdata)],
            )
            .map_err(|e| Error::new(format!("write: {e}")))?;
        let v = jint_value(n, "write")?;
        if v >= 0 {
            Ok(v as usize)
        } else {
            Err(Error::new("write returned invalid count"))
        }
    })
}

#[cfg(target_os = "android")]
pub fn call_read(path: &str, timeout_ms: u64, size: usize) -> Result<Vec<u8>, Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        let arr = env
            .call_static_method(
                usb_class(cache),
                "read",
                "(Ljava/lang/String;II)[B",
                &[
                    JValue::Object(&jpath),
                    JValue::Int(timeout_ms as i32),
                    JValue::Int(size as i32),
                ],
            )
            .map_err(|e| Error::new(format!("read: {e}")))?;
        match arr {
            JValueOwned::Object(obj) => {
                if obj.is_null() {
                    return Err(Error::new("read returned null"));
                }
                let jarr = JByteArray::from(obj);
                env.convert_byte_array(&jarr)
                    .map_err(|e| Error::new(format!("read decode: {e}")))
            }
            _ => Err(Error::new("read returned non-array")),
        }
    })
}

#[cfg(target_os = "android")]
#[allow(clippy::too_many_arguments)]
pub fn call_ctl(
    path: &str,
    op: &str,
    baud_rate: u32,
    timeout_ms: u64,
    data_bits: u8,
    flow_control: u8,
    parity: u8,
    stop_bits: u8,
    buffer_type: i32,
) -> Result<bool, Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        let jop = jstring(env, op)?;
        let ok = env
            .call_static_method(
                usb_class(cache),
                "ctl",
                "(Ljava/lang/String;Ljava/lang/String;IIIIIII)Z",
                &[
                    JValue::Object(&jpath),
                    JValue::Object(&jop),
                    JValue::Int(baud_rate as i32),
                    JValue::Int(timeout_ms as i32),
                    JValue::Int(data_bits as i32),
                    JValue::Int(flow_control as i32),
                    JValue::Int(parity as i32),
                    JValue::Int(stop_bits as i32),
                    JValue::Int(buffer_type),
                ],
            )
            .map_err(|e| Error::new(format!("ctl {op}: {e}")))?;
        bool_value(ok, op)
    })
}

#[cfg(target_os = "android")]
pub fn call_ctl_bytes_to_write(path: &str) -> Result<u32, Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        let n = env
            .call_static_method(
                usb_class(cache),
                "ctlBytesToWrite",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&jpath)],
            )
            .map_err(|e| Error::new(format!("ctlBytesToWrite: {e}")))?;
        let v = jint_value(n, "ctlBytesToWrite")?;
        if v >= 0 {
            Ok(v as u32)
        } else {
            Err(Error::new("ctlBytesToWrite returned invalid count"))
        }
    })
}

#[cfg(target_os = "android")]
pub fn call_signal(path: &str, op: &str, level: bool) -> Result<bool, Error> {
    with_env(|env| {
        let cache = cache()?;
        let jpath = jstring(env, path)?;
        let jop = jstring(env, op)?;
        let ok = env
            .call_static_method(
                usb_class(cache),
                "signal",
                "(Ljava/lang/String;Ljava/lang/String;Z)Z",
                &[
                    JValue::Object(&jpath),
                    JValue::Object(&jop),
                    JValue::Bool(level as u8),
                ],
            )
            .map_err(|e| Error::new(format!("signal {op}: {e}")))?;
        bool_value(ok, op)
    })
}
