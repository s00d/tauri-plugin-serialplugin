//! Android JNI/USB layer (Kotlin fd bridge + Rust drivers).

pub mod driver_host;
#[cfg(target_os = "android")]
pub mod enumerate;
pub mod fd_bridge;
pub mod jni;
pub mod registry;
#[cfg(all(debug_assertions, target_os = "android"))]
pub mod test_harness;
pub mod usb_path;
