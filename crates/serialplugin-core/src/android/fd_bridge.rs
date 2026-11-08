//! JNI bridge: Kotlin UsbFdBridge → Rust fd API.

#[cfg(any(target_os = "android", test))]
use crate::error::Error;
#[cfg(any(target_os = "android", test))]
use crate::jni_ready::jni_not_ready_message;
#[cfg(any(target_os = "android", test))]
use jni::errors::Error as JniError;
#[cfg(any(target_os = "android", test))]
use jni::objects::{GlobalRef, JObject, JString, JValue};
#[cfg(any(target_os = "android", test))]
use jni::{JNIEnv, JavaVM};
#[cfg(any(target_os = "android", test))]
use std::sync::OnceLock;

#[cfg(any(target_os = "android", test))]
static JVM: OnceLock<JavaVM> = OnceLock::new();

#[cfg(any(target_os = "android", test))]
struct FdJniCache {
    class: GlobalRef,
}

#[cfg(any(target_os = "android", test))]
static CACHE: OnceLock<FdJniCache> = OnceLock::new();

/// Last `new_global_ref` failure from [`init_class`], if any.
#[cfg(any(target_os = "android", test))]
static CLASS_INIT_ERROR: OnceLock<String> = OnceLock::new();

#[cfg(any(target_os = "android", test))]
fn not_ready() -> Error {
    Error::new(jni_not_ready_message(
        JVM.get().is_some(),
        CACHE.get().is_some(),
        CLASS_INIT_ERROR.get().map(String::as_str),
    ))
}

#[cfg(any(target_os = "android", test))]
impl From<JniError> for Error {
    fn from(err: JniError) -> Self {
        Error::new(err.to_string())
    }
}

#[cfg(any(target_os = "android", test))]
fn with_env<T, F>(f: F) -> Result<T, Error>
where
    F: FnOnce(&mut JNIEnv) -> Result<T, Error>,
{
    let vm = JVM.get().ok_or_else(not_ready)?;
    let mut env = vm
        .attach_current_thread()
        .map_err(|e| Error::new(format!("JNI attach failed: {e}")))?;
    env.with_local_frame(32, |env| {
        // JNI errors can leave a Java exception pending. Clear/map it even when
        // the operation failed, before the attached native thread is detached.
        // Otherwise an expected USB unplug becomes an uncaught Java exception.
        let out = f(env);
        map_exception(env, "USB fd operation failed")?;
        out
    })
}

#[cfg(any(target_os = "android", test))]
fn map_exception(env: &mut JNIEnv, fallback: &str) -> Result<(), Error> {
    if !env
        .exception_check()
        .map_err(|e| Error::new(e.to_string()))?
    {
        return Ok(());
    }
    let exception = env.exception_occurred().ok();
    env.exception_clear().map_err(Error::from)?;
    let msg: String = exception
        .and_then(|exc| {
            let jmsg = env
                .call_method(&exc, "getMessage", "()Ljava/lang/String;", &[])
                .ok()
                .and_then(|v| v.l().ok());
            jmsg.and_then(|s| {
                let jstr = JString::from(s);
                env.get_string(&jstr).ok().map(|j| j.into())
            })
        })
        .unwrap_or_else(|| fallback.into());
    // Exception message retrieval itself can throw. Never let a secondary
    // exception escape this boundary either.
    env.exception_clear().map_err(Error::from)?;
    Err(Error::new(msg))
}

#[cfg(any(target_os = "android", test))]
fn cache(_env: &mut JNIEnv) -> Result<&'static FdJniCache, Error> {
    CACHE.get().ok_or_else(not_ready)
}

/// Called from `UsbNative.nativeInit` on a Java thread, where the app class loader is in scope.
#[cfg(any(target_os = "android", test))]
pub fn init_class(env: &mut JNIEnv, class: &JObject) {
    if CACHE.get().is_some() {
        return;
    }
    match env.new_global_ref(class) {
        Ok(global) => {
            let _ = CACHE.set(FdJniCache { class: global });
        }
        Err(e) => {
            let msg = e.to_string();
            crate::log_error!("UsbNative init_class new_global_ref failed: {msg}");
            let _ = CLASS_INIT_ERROR.set(msg);
        }
    }
}

#[cfg(any(target_os = "android", test))]
pub fn init_java_vm(vm: JavaVM) {
    let _ = JVM.set(vm);
}

#[cfg(any(target_os = "android", test))]
pub fn call_enumerate_json() -> Result<String, Error> {
    with_env(|env| {
        let cache = cache(env)?;
        let s = env
            .call_static_method(&cache.class, "enumerateJson", "()Ljava/lang/String;", &[])
            .map_err(|e| Error::new(e.to_string()))?;
        let obj = s.l().map_err(|e| Error::new(e.to_string()))?;
        let jstr = unsafe { JString::from_raw(obj.into_raw()) };
        let out: String = env.get_string(&jstr)?.into();
        Ok(out)
    })
}

#[cfg(any(target_os = "android", test))]
pub fn call_open_device_fd(device_name: &str) -> Result<i32, Error> {
    with_env(|env| {
        let cache = cache(env)?;
        let name = env.new_string(device_name)?;
        let v = env.call_static_method(
            &cache.class,
            "openDeviceFd",
            "(Ljava/lang/String;)I",
            &[JValue::Object(&JObject::from(name))],
        )?;
        v.i().map_err(|e| Error::new(e.to_string()))
    })
}

#[cfg(any(target_os = "android", test))]
pub fn call_close_device_fd(device_name: &str) -> Result<(), Error> {
    with_env(|env| {
        let cache = cache(env)?;
        let name = env.new_string(device_name)?;
        env.call_static_method(
            &cache.class,
            "closeDeviceFd",
            "(Ljava/lang/String;)V",
            &[JValue::Object(&JObject::from(name))],
        )?;
        Ok(())
    })
}

#[cfg(all(test, not(any(target_os = "android", target_os = "ios"))))]
mod tests {
    use super::*;

    #[test]
    fn failed_java_call_is_cleared_before_returning_to_caller() {
        let args = jni::InitArgsBuilder::new().build().unwrap();
        init_java_vm(JavaVM::new(args).unwrap());
        // Keep the thread attached so we can inspect the exception state after
        // with_env returns, before thread teardown could deliver it to Java.
        let env = JVM.get().unwrap().attach_current_thread().unwrap();
        let result: Result<(), Error> = with_env(|env| {
            let invalid = env.new_string("not-a-number")?;
            env.call_static_method(
                "java/lang/Integer",
                "parseInt",
                "(Ljava/lang/String;)I",
                &[JValue::Object(&JObject::from(invalid))],
            )?;
            Ok(())
        });
        let pending = env.exception_check().unwrap();
        // Also clear on test failure so the failing regression cannot kill the JVM.
        env.exception_clear().unwrap();
        assert!(!pending, "failed JNI call leaked a pending Java exception");
        assert!(result.unwrap_err().to_string().contains("not-a-number"));
        assert_eq!(with_env(|_| Ok(42)).unwrap(), 42);
        assert_eq!(
            with_env::<(), _>(|_| Err(Error::new("native failure")))
                .unwrap_err()
                .to_string(),
            "native failure"
        );
        // A Throwable with a null message must still be cleared.
        let result: Result<(), Error> = with_env(|env| {
            let exception = env.new_object("java/io/IOException", "()V", &[])?;
            env.throw(jni::objects::JThrowable::from(exception))?;
            Err(Error::new("JNI failure"))
        });
        assert!(result.is_err());
        assert!(!env.exception_check().unwrap());
    }
}
