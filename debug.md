# Android Debugging Guide for tauri-plugin-serialplugin

## Recent Fixes Applied

Based on the analysis of commits from [niftitech/tauri-plugin-serialplugin](https://github.com/niftitech/tauri-plugin-serialplugin), the following fixes have been applied:

### 1. Enhanced Enum Support (Commit 7d64208)
- Added `companion object` with `fromValue` methods to all enum classes
- Supports both string and numeric values for all port configuration parameters
- Improved type safety and error handling

### 2. Improved Error Handling (Commit b9267e1)
- Enhanced logging throughout the plugin
- Better error messages and debugging information
- Fixed response handling in Rust mobile API

### 3. Type Flexibility
- All port configuration parameters now accept both String and Number types
- Automatic conversion between different value formats
- Backward compatibility maintained

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
   
   # More specific filtering
   adb logcat -v time | grep -E "SerialPlugin|SerialPortManager"
   ```

2. **USB Device Debugging**
   ```bash
   # List connected USB devices
   adb shell lsusb
   
   # Check USB permissions
   adb shell dumpsys usb
   
   # Monitor USB events
   adb logcat -v time | grep -i "usb"
   ```

## Key Improvements Made

### 1. Enhanced Type Support
All enum classes now support both string and numeric values:

```kotlin
// DataBits enum
enum class DataBits(val value: Int) {
    FIVE(5), SIX(6), SEVEN(7), EIGHT(8);
    
    companion object {
        fun fromValue(value: Int): DataBits {
            return values().find { it.value == value } ?: EIGHT
        }
    }
}
```

### 2. Flexible Parameter Handling
Port configuration now accepts multiple value types:

```kotlin
@InvokeArg
class PortConfigArgs {
    lateinit var path: String
    var baudRate: Int = 9600
    var dataBits: Any? = null      // String or Number
    var flowControl: Any? = null   // String or Number
    var parity: Any? = null        // String or Number
    var stopBits: Any? = null      // String or Number
    var timeout: Int = 1000
}
```

### 3. Improved Error Handling
Enhanced logging and error messages:

```kotlin
Log.d("SerialPlugin", "Opening port: ${args.path}")
Log.d("SerialPortManager", "Setting port parameters: baudRate=${config.baudRate}")
Log.e("SerialPlugin", "Failed to open port: ${e.message}", e)
```

### 4. Fixed Response Handling
Rust mobile API now properly handles plugin responses:

```rust
match self.0.run_mobile_plugin("open", params) {
    Ok(Value::Bool(true)) => Ok(()),
    Ok(_) => Ok(()), // invoke.resolve() returns Ok(_)
    Err(e) => Err(Error::new(format!("Plugin error: {}", e))),
}
```

## Common Issues and Solutions

### 1. DataBits Type Mismatch (FIXED)
- **Issue**: `Type mismatch: inferred type is Any but String was expected`
- **Solution**: Enhanced type handling with `when` expressions
- **Status**: ✅ Resolved

### 2. USB Device Detection
- **Check**: Verify device appears in `getAvailablePorts()`
- **Debug**: Monitor logs with tag "SerialPortManager"
- **Solution**: Ensure proper USB permissions and device support

### 3. Port Configuration
- **Default settings**:
  ```kotlin
  data class SerialPortConfig(
      val path: String,
      val baudRate: Int = 9600,
      val dataBits: DataBits = DataBits.EIGHT,
      val flowControl: FlowControl = FlowControl.NONE,
      val parity: Parity = Parity.NONE,
      val stopBits: StopBits = StopBits.ONE,
      val timeout: Int = 1000
  )
  ```

## Testing Your Device

### 1. Check Device Support
```bash
# Get device information
adb shell getprop | grep -i "ro.product"

# Check USB support
adb shell getprop | grep -i "usb"

# List USB devices
adb shell lsusb
```

### 2. Test USB Connection
```bash
# Monitor USB events
adb logcat -v time | grep -i "usb"

# Check USB permissions
adb shell dumpsys usb | grep -i "permission"
```

### 3. Monitor Plugin Logs
```bash
# Filter plugin logs
adb logcat -v time | grep -E "SerialPlugin|SerialPortManager"

# Monitor specific operations
adb logcat -v time | grep -E "Opening port|Port opened|Failed to open"
```

## Development Workflow

### 1. Making Changes
- Edit files in `android/src/main/kotlin/app/tauri/serialplugin/`
- Test changes in example app: `examples/serialport-test/`
- Build and verify: `npm run tauri android build`

### 2. Testing Changes
- Use example app for testing
- Monitor logs with logcat
- Test with different USB devices
- Verify all port configurations

### 3. Debugging Process
- Enable verbose logging
- Check USB device detection
- Verify port configuration
- Monitor data transmission
- Check error handling

## Known Working Configurations

### CP2104 USB-to-UART Bridge
- **Vendor ID**: 4292 (0x10C4)
- **Product ID**: 60000 (0xEA60)
- **Default settings**: 115200 baud, 8 data bits, 1 stop bit, no parity
- **Status**: ✅ Tested and working

### Common USB-to-Serial Adapters
- **FTDI**: VID 0x0403, PID 0x6001
- **CP210x**: VID 0x10C4, PID 0xEA60
- **CH340**: VID 0x1A86, PID 0x7523
- **PL2303**: VID 0x067B, PID 0x2303

## Resources

### 1. Project Documentation
- [Tauri Android Guide](https://tauri.app/v1/guides/building/android)
- [USB Serial for Android](https://github.com/mik3y/usb-serial-for-android)
- [Android USB Host API](https://developer.android.com/guide/topics/connectivity/usb/host)

### 2. Project Files
- Main plugin: `android/src/main/kotlin/app/tauri/serialplugin/SerialPlugin.kt`
- Port configuration: `android/src/main/kotlin/app/tauri/serialplugin/models/SerialPortConfig.kt`
- Port management: `android/src/main/kotlin/app/tauri/serialplugin/manager/SerialPortManager.kt`
- Example app: `examples/serialport-test/`

### 3. Support
- GitHub Issues: [tauri-plugin-serialplugin](https://github.com/s00d/tauri-plugin-serialplugin)
- Fork with Android fixes: [niftitech/tauri-plugin-serialplugin](https://github.com/niftitech/tauri-plugin-serialplugin)
- Tauri Discord: [Tauri Discord Server](https://discord.gg/tauri)

## Troubleshooting Checklist

- [ ] USB device properly connected
- [ ] USB permissions granted
- [ ] Device appears in `available_ports()`
- [ ] Correct port path used
- [ ] Valid baud rate and port settings
- [ ] No conflicting applications using the port
- [ ] Device drivers installed (if needed)
- [ ] USB debugging enabled
- [ ] Logs show successful port opening
- [ ] Data transmission working 