//! JNI bridge: Kotlin SIOM → Rust RX hub.

#[cfg(target_os = "android")]
use crate::android::registry::{feed_rx, on_app_destroy, on_port_list_change, on_usb_error};
#[cfg(target_os = "android")]
use jni::objects::{JByteArray, JClass, JString};
#[cfg(target_os = "android")]
use jni::JNIEnv;

#[cfg(target_os = "android")]
fn jstring_to_rust(env: &mut JNIEnv, s: &JString) -> Option<String> {
    env.get_string(s).ok().map(|j| j.into())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_feedRx(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    data: JByteArray,
) {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return;
    };
    let Ok(chunk) = env.convert_byte_array(&data) else {
        return;
    };
    feed_rx(&path, &chunk);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onUsbError(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    reason: JString,
) {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return;
    };
    let reason = jstring_to_rust(&mut env, &reason).unwrap_or_else(|| "USB error".into());
    on_usb_error(&path, &reason);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onPortListChange(
    _env: JNIEnv,
    _class: JClass,
) {
    on_port_list_change();
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onAppDestroy(
    _env: JNIEnv,
    _class: JClass,
) {
    on_app_destroy();
}
