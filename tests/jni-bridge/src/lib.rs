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

/// Given a JVM,
/// when a Java call throws, Rust fails, or an exception has no message,
/// then the caller gets the error, and no Java exception is left pending on its thread.
#[test]
fn a_failed_java_call_is_cleared_before_returning_to_its_caller() {
    use error::Error;
    use fd_bridge::{init_java_vm, with_env};
    use jni::objects::{JObject, JThrowable, JValue};
    use jni::JavaVM;

    let args = jni::InitArgsBuilder::new().build().unwrap();
    let vm = JavaVM::new(args).unwrap();
    let again = unsafe { JavaVM::from_raw(vm.get_java_vm_pointer()).unwrap() };
    init_java_vm(again);
    // Keep the thread attached so we can inspect the exception state after
    // with_env returns, before thread teardown could deliver it to Java.
    let env = vm.attach_current_thread().unwrap();
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
        env.throw(JThrowable::from(exception))?;
        Err(Error::new("JNI failure"))
    });
    assert!(result.is_err());
    assert!(!env.exception_check().unwrap());
}
