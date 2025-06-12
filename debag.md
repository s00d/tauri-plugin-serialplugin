# Android Debugging Guide for tauri-plugin-serialplugin

## Project Structure

```
tauri-plugin-serialplugin/
├── examples/
│   └── serialport-test/           # Example application
│       └── src-tauri/
│           └── gen/              # Generated Android project
│               └── android/      # Android project directory
│                   ├── app/      # Main application module
│                   └── tauri-plugin-serialplugin/  # Plugin module
├── android/                      # Plugin source code
│   └── src/
│       └── main/
│           ├── kotlin/
│           │   └── app/
│           │       └── tauri/
│           │           └── serialplugin/
│           │               ├── SerialPlugin.kt
│           │               ├── models/
│           │               │   └── SerialPortConfig.kt
│           │               └── manager/
│           │                   └── SerialPortManager.kt
│           └── AndroidManifest.xml
└── build.gradle
```

## Building and Debugging

### Building the Project

1. **From Project Root**
   ```bash
   # Build the entire project
   npm run tauri android build
   
   # Build with verbose output
   npm run tauri android build --verbose
   ```

2. **From Android Project Directory**
   ```bash
   # Navigate to the generated Android project
   cd examples/serialport-test/src-tauri/gen/android
   
   # Build the plugin module
   ./gradlew :tauri-plugin-serialplugin:compileReleaseKotlin --stacktrace --info
   
   # Clean build
   ./gradlew clean
   ```

### Debugging Tools

1. **Logcat for Plugin**
   ```bash
   # Filter logs for the plugin
   adb logcat -v time | grep -E "SerialPort|Tauri|tauri-plugin-serialplugin"
   ```

2. **USB Device Debugging**
   ```bash
   # List connected USB devices
   adb shell lsusb
   
   # Check USB permissions
   adb shell dumpsys usb
   ```

### Testing Your Device

1. **Check Device Support**
   ```bash
   # Get device information
   adb shell getprop | grep -i "ro.product"
   
   # Check USB support
   adb shell getprop | grep -i "usb"
   ```

2. **Test USB Connection**
   ```bash
   # Monitor USB events
   adb logcat -v time | grep -i "usb"
   
   # Check USB permissions
   adb shell dumpsys usb | grep -i "permission"
   ```

### Project-Specific Dependencies

```gradle
dependencies {
    implementation("androidx.core:core-ktx:1.9.0")
    implementation("androidx.appcompat:appcompat:1.6.0")
    implementation("com.github.mik3y:usb-serial-for-android:3.8.1")
    implementation(project(":tauri-android"))
    implementation("com.google.code.gson:gson:2.10.1")
}
```

### Known Issues

1. **USB Permissions**
    - Required permissions in `AndroidManifest.xml`:
      ```xml
      <uses-feature android:name="android.hardware.usb.host" />
      <uses-permission android:name="android.permission.USB_PERMISSION" />
      ```