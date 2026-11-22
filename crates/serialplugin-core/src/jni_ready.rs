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
    //! In `src/`: the module is private and only Android uses it.
    use super::jni_not_ready_message;

    #[test]
    fn the_message_names_what_is_missing() {
        for (jvm, class, error, want) in [
            (
                false,
                false,
                None,
                "JNI not initialized (UsbNative.bind not called)",
            ),
            (
                true,
                false,
                Some("out of memory"),
                "UsbNative JNI class cache unavailable: out of memory",
            ),
            (
                true,
                false,
                None,
                "UsbNative JNI class not cached (nativeInit global ref failed or skipped)",
            ),
            (
                false,
                true,
                None,
                "JavaVM not cached (nativeInit get_java_vm failed)",
            ),
            (true, true, None, "JNI ready"),
        ] {
            assert_eq!(jni_not_ready_message(jvm, class, error), want);
        }
    }
}
