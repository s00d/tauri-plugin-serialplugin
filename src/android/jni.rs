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
    crate::android::driver_host::global_host().on_device_detached(&name);
    on_device_detached(&name);
}

#[cfg(target_os = "android")]
#[no_mangle]
pub extern "system" fn Java_app_tauri_serialplugin_UsbNative_nativeInit(
    env: JNIEnv,
    _class: JClass,
) {
    if let Ok(vm) = env.get_java_vm() {
        crate::android::fd_bridge::init_java_vm(vm);
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
