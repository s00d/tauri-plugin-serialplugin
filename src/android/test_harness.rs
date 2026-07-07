//! Debug-only JNI introspection for Kotlin ↔ Rust integration tests.

#[cfg(all(debug_assertions, target_os = "android"))]
use crate::android::registry::test_harness;
#[cfg(all(debug_assertions, target_os = "android"))]
use jni::objects::{JByteArray, JClass, JString};
#[cfg(all(debug_assertions, target_os = "android"))]
use jni::sys::{jboolean, jlong};
#[cfg(all(debug_assertions, target_os = "android"))]
use jni::JNIEnv;

#[cfg(all(debug_assertions, target_os = "android"))]
fn jstring_to_rust(env: &mut JNIEnv, s: &JString) -> Option<String> {
    env.get_string(s).ok().map(|j| j.into())
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testHarnessReset(
    _env: JNIEnv,
    _class: JClass,
) {
    test_harness::reset();
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testRegisterPort(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return;
    };
    test_harness::register_port(&path);
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testHubBufferedLen(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jlong {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return -1;
    };
    test_harness::hub_buffered_len(&path)
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testHubTakeIdle<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    path: JString<'local>,
) -> JByteArray<'local> {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return env.byte_array_from_slice(&[]).unwrap_or_default();
    };
    let bytes = test_harness::hub_take_idle(&path);
    env.byte_array_from_slice(&bytes).unwrap_or_default()
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testRegistryHasPort(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
) -> jboolean {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return 0;
    };
    test_harness::registry_has_port(&path) as jboolean
}

#[cfg(all(debug_assertions, target_os = "android"))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testInvokeWrite(
    mut env: JNIEnv,
    _class: JClass,
    path: JString,
    data: JByteArray,
) -> jlong {
    let Some(path) = jstring_to_rust(&mut env, &path) else {
        return -1;
    };
    let Ok(chunk) = env.convert_byte_array(&data) else {
        return -1;
    };
    match test_harness::invoke_write(&path, &chunk) {
        Ok(n) => n as jlong,
        Err(_) => -1,
    }
}
