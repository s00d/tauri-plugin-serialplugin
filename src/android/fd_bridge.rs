//! JNI bridge: Kotlin UsbFdBridge → Rust fd API.

#[cfg(target_os = "android")]
use crate::error::Error;
#[cfg(target_os = "android")]
use jni::errors::Error as JniError;
#[cfg(target_os = "android")]
use jni::objects::{GlobalRef, JObject, JString, JValue};
#[cfg(target_os = "android")]
use jni::{JNIEnv, JavaVM};
#[cfg(target_os = "android")]
use std::sync::OnceLock;

#[cfg(target_os = "android")]
static JVM: OnceLock<JavaVM> = OnceLock::new();

#[cfg(target_os = "android")]
struct FdJniCache {
    class: GlobalRef,
}

#[cfg(target_os = "android")]
static CACHE: OnceLock<FdJniCache> = OnceLock::new();

#[cfg(target_os = "android")]
fn not_init() -> Error {
    Error::new("JNI not initialized (UsbNative.bind not called)")
}

#[cfg(target_os = "android")]
impl From<JniError> for Error {
    fn from(err: JniError) -> Self {
        Error::new(err.to_string())
    }
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
    env.with_local_frame(32, |env| {
        let out = f(env)?;
        map_exception(env, "USB fd operation failed")?;
        Ok(out)
    })
}

#[cfg(target_os = "android")]
fn map_exception(env: &mut JNIEnv, fallback: &str) -> Result<(), Error> {
    if !env
        .exception_check()
        .map_err(|e| Error::new(e.to_string()))?
    {
        return Ok(());
    }
    let msg: String = env
        .exception_occurred()
        .ok()
        .and_then(|exc| {
            let _ = env.exception_clear();
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
    Err(Error::new(msg))
}

#[cfg(target_os = "android")]
fn cache(env: &mut JNIEnv) -> Result<&'static FdJniCache, Error> {
    if let Some(c) = CACHE.get() {
        return Ok(c);
    }
    let class = env
        .find_class("app/tauri/serialplugin/UsbNative")
        .map_err(|e| Error::new(e.to_string()))?;
    let global = env
        .new_global_ref(class)
        .map_err(|e| Error::new(e.to_string()))?;
    let _ = CACHE.set(FdJniCache { class: global });
    CACHE.get().ok_or_else(not_init)
}

#[cfg(target_os = "android")]
pub fn init_java_vm(vm: JavaVM) {
    let _ = JVM.set(vm);
}

#[cfg(target_os = "android")]
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

#[cfg(target_os = "android")]
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

#[cfg(target_os = "android")]
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
