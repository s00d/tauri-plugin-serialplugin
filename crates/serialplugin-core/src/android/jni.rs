//! JNI bridge: Kotlin lifecycle/USB events → Rust hub.
//! RX path: Rust USB bulk-IN reader → [`serialport::SerialPort`] ring → [`PortRxHub`] poll.

#[cfg(target_os = "android")]
use crate::android::registry::{
    on_app_destroy, on_device_detached, on_port_list_change, on_usb_error,
};
#[cfg(target_os = "android")]
use jni::objects::{JClass, JString};
#[cfg(target_os = "android")]
use jni::JNIEnv;

#[cfg(target_os = "android")]
fn jstring_to_rust(env: &mut JNIEnv, s: &JString) -> Option<String> {
    env.get_string(s).ok().map(|j| j.into())
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onDeviceDetached(
    mut env: JNIEnv,
    _class: JClass,
    device_name: JString,
) {
    let Some(name) = jstring_to_rust(&mut env, &device_name) else {
        return;
    };
    // Runs on the Android main thread (broadcast receiver). Mark the ports dead
    // right away; the teardown joins reader threads and takes port locks, so it
    // runs off the UI thread to avoid ANRs.
    crate::android::driver_host::global_host().mark_device_detached(&name);
    std::thread::spawn(move || {
        crate::android::driver_host::global_host().on_device_detached(&name);
        on_device_detached(&name);
    });
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_UsbNative_nativeInit(
    mut env: JNIEnv,
    class: JClass,
) {
    // `class` is UsbNative itself. Cache it here: FindClass on a Rust-attached
    // thread uses the system class loader and cannot see app classes.
    crate::android::fd_bridge::init_class(&mut env, &class);
    match env.get_java_vm() {
        Ok(vm) => crate::android::fd_bridge::init_java_vm(vm),
        Err(e) => {
            crate::log_error!("UsbNative nativeInit get_java_vm failed: {e}");
        }
    }
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

// --- BLE (Kotlin BleGattBridge). Exported without the `ble` feature too: Kotlin
// always binds BleNative, so the symbols must exist; they are no-ops then.

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_BleNative_nativeInit(
    mut env: JNIEnv,
    class: JClass,
) {
    #[cfg(feature = "ble")]
    {
        crate::android::ble_bridge::init_class(&mut env, &class);
        if let Ok(vm) = env.get_java_vm() {
            crate::android::fd_bridge::init_java_vm(vm);
        }
    }
    #[cfg(not(feature = "ble"))]
    let _ = (&mut env, class);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onBleData(
    env: JNIEnv,
    _class: JClass,
    handle: jni::sys::jlong,
    data: jni::objects::JByteArray,
) {
    #[cfg(feature = "ble")]
    if let Ok(bytes) = env.convert_byte_array(&data) {
        crate::port::ble_android::on_data(handle, bytes);
    }
    #[cfg(not(feature = "ble"))]
    let _ = (env, handle, data);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_MobileBridge_onBleDisconnected(
    mut env: JNIEnv,
    _class: JClass,
    handle: jni::sys::jlong,
    reason: JString,
) {
    #[cfg(feature = "ble")]
    {
        let reason = jstring_to_rust(&mut env, &reason).unwrap_or_else(|| "disconnected".into());
        crate::port::ble_android::on_disconnected(handle, &reason);
    }
    #[cfg(not(feature = "ble"))]
    let _ = (&mut env, handle, reason);
}
