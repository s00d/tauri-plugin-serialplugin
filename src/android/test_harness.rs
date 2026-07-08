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

#[cfg(all(
    debug_assertions,
    target_os = "android",
    feature = "android-test-harness"
))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testOpenFakePort<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    device_name: JString<'local>,
) -> JString<'local> {
    let Some(device_name) = jstring_to_rust(&mut env, &device_name) else {
        return JString::default();
    };
    match test_harness::open_fake_port(&device_name) {
        Ok(path) => env.new_string(path).unwrap_or_default(),
        Err(e) => env.new_string(format!("ERR:{e}")).unwrap_or_default(),
    }
}

#[cfg(all(
    debug_assertions,
    target_os = "android",
    feature = "android-test-harness"
))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testFakeInjectRx(
    mut env: JNIEnv,
    _class: JClass,
    device_name: JString,
    data: JByteArray,
) -> jboolean {
    let Some(device_name) = jstring_to_rust(&mut env, &device_name) else {
        return 0;
    };
    let Ok(chunk) = env.convert_byte_array(&data) else {
        return 0;
    };
    test_harness::fake_inject_rx(&device_name, &chunk) as jboolean
}

#[cfg(all(
    debug_assertions,
    target_os = "android",
    feature = "android-test-harness"
))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testFakeTakeTx<'local>(
    mut env: JNIEnv<'local>,
    _class: JClass<'local>,
    device_name: JString<'local>,
) -> JByteArray<'local> {
    let Some(device_name) = jstring_to_rust(&mut env, &device_name) else {
        return env.byte_array_from_slice(&[]).unwrap_or_default();
    };
    let bytes = test_harness::fake_take_tx(&device_name);
    env.byte_array_from_slice(&bytes).unwrap_or_default()
}

#[cfg(all(
    debug_assertions,
    target_os = "android",
    feature = "android-test-harness"
))]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_testFakeInjectError(
    mut env: JNIEnv,
    _class: JClass,
    device_name: JString,
    reason: JString,
) -> jboolean {
    let Some(device_name) = jstring_to_rust(&mut env, &device_name) else {
        return 0;
    };
    let Some(reason) = jstring_to_rust(&mut env, &reason) else {
        return 0;
    };
    test_harness::fake_inject_error(&device_name, &reason) as jboolean
}
