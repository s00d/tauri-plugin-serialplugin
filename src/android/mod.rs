//! Android JNI/USB layer (Kotlin SIOM bridge).

pub mod jni;
pub mod registry;
#[cfg(all(debug_assertions, target_os = "android"))]
pub mod test_harness;
pub mod usb_io;
pub mod usb_jni;
