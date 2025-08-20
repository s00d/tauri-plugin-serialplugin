# Android Plugin Build Instructions

## Prerequisites

1. **Android Studio** (latest version)
2. **Android SDK** (API 24+)
3. **Kotlin** (1.8+)
4. **Gradle** (7.0+)
5. **Tauri CLI** (`npm install -g @tauri-apps/cli`)

## Building the Plugin

### 1. Environment Setup

```bash
# Make sure you have all dependencies installed
cd android
./gradlew clean
```

### 2. Building the Library

```bash
# Build AAR file
./gradlew assembleRelease

# Or for debugging
./gradlew assembleDebug
```

### 3. Build Verification

Built files will be located in:
- `build/outputs/aar/` - AAR library
- `build/intermediates/aar_main_jar/` - JAR files

## Testing

### 1. Running the Example Application

```bash
cd examples/serialport-test
npm install
npm run tauri android dev
```

### 2. Connecting USB Device

1. Connect USB Serial device to Android device
2. Allow USB access in the application
3. Device should appear in the list of available ports

### 3. Checking Logs

```bash
# View Android logs
adb logcat | grep -E "(SerialPlugin|SerialPortManager)"

# Or through Android Studio
# View -> Tool Windows -> Logcat
```

## Troubleshooting

### Build Issues

1. **Gradle Error**: Check Gradle and Android Gradle Plugin versions
2. **Kotlin Error**: Make sure Kotlin version is 1.8+
3. **Dependency Error**: Check repository availability

### Runtime Issues

1. **USB permissions not requested**: Check AndroidManifest.xml
2. **Device not detected**: Check device_filter.xml
3. **Connection errors**: Check logs and USB drivers

### Debugging

1. **Enable detailed logging** in SerialPortManager
2. **Check USB permissions** in Android settings
3. **Test with different devices** (FTDI, CH340, CP210x)

## Project Structure

```
android/
├── src/main/kotlin/
│   ├── SerialPlugin.kt          # Main plugin
│   ├── SerialPortManager.kt     # USB port manager
│   └── models/                  # Data models
├── src/main/AndroidManifest.xml # Application manifest
├── src/main/res/xml/
│   └── device_filter.xml        # USB device filter
├── build.gradle                 # Build configuration
└── README.md                    # Documentation
```

## Dependencies

- **usb-serial-for-android**: Library for USB Serial operations
- **tauri-android**: Tauri Android runtime
- **AndroidX**: Modern Android libraries

## License

MIT License - see LICENSE file in the project root.
