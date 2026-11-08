//! Shared wording for Android JNI init failures (host-testable).

/// Wording when JVM / UsbNative class cache is incomplete.
pub(crate) fn jni_not_ready_message(
    has_jvm: bool,
    has_class: bool,
    class_init_error: Option<&str>,
) -> String {
    if has_jvm && has_class {
        return "JNI ready".into();
    }
    if let Some(err) = class_init_error {
        return format!("UsbNative JNI class cache unavailable: {err}");
    }
    if has_jvm && !has_class {
        return "UsbNative JNI class not cached (nativeInit global ref failed or skipped)".into();
    }
    if has_class && !has_jvm {
        return "JavaVM not cached (nativeInit get_java_vm failed)".into();
    }
    "JNI not initialized (UsbNative.bind not called)".into()
}

#[cfg(test)]
mod tests {
    use super::jni_not_ready_message;

    #[test]
    fn message_when_bind_never_called() {
        assert_eq!(
            jni_not_ready_message(false, false, None),
            "JNI not initialized (UsbNative.bind not called)"
        );
    }

    #[test]
    fn message_when_global_ref_failed() {
        let msg = jni_not_ready_message(true, false, Some("out of memory"));
        assert!(msg.contains("class cache unavailable"));
        assert!(msg.contains("out of memory"));
    }

    #[test]
    fn message_when_jvm_set_but_class_missing() {
        assert!(jni_not_ready_message(true, false, None).contains("class not cached"));
    }

    #[test]
    fn message_when_class_set_but_jvm_missing() {
        assert!(jni_not_ready_message(false, true, None).contains("JavaVM not cached"));
    }
}
