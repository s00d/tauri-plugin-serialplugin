# Android Plugin Build Instructions

## Prerequisites

1. **Android Studio** (latest)
2. **Android SDK** (API 24+, compile 34)
3. **Kotlin** 1.9+
4. **Gradle** 8.5+
5. **Rust** 1.79+ with Android targets (`aarch64-linux-android`, etc.)
6. **Tauri CLI** 2.x

## Building the library module

```bash
cd android
export JAVA_HOME=$(/usr/libexec/java_home -v 17)  # macOS
./gradlew assembleDebug
```

Outputs: `build/outputs/aar/`

## Rust Android check

```bash
rustup target add aarch64-linux-android
cargo check -p android-usb-serial --target aarch64-linux-android
cargo check -p tauri-plugin-serialplugin --target aarch64-linux-android
```

## Example app (Tauri)

```bash
cd examples/serialport-test
pnpm install
pnpm tauri android dev
```

## USB on device

1. Add `device_filter.xml` entries for your VID/PID in the **app** manifest.
2. Grant USB permission when prompted.
3. Port paths look like `/dev/bus/usb/001/002` (multi-interface FTDI: `#1`, `#2`, …).

## Logs

```bash
adb logcat -v time -s UsbFdBridge SerialPlugin RustStdoutStderr
```

## Project structure

```text
android/
├── src/main/kotlin/app/tauri/serialplugin/
│   ├── SerialPlugin.kt
│   ├── UsbNative.kt
│   ├── MobileBridge.kt
│   └── manager/UsbFdBridge.kt
├── src/main/res/xml/device_filter.xml
└── build.gradle

crates/android-usb-serial/   # published-quality driver crate (nusb)
src/android/                 # fd_bridge, driver_host, registry JNI
```

## Attribution

Driver logic is ported from [usb-serial-for-android](https://github.com/mik3y/usb-serial-for-android) into Rust (`android-usb-serial`). See `crates/android-usb-serial/NOTICE`.
