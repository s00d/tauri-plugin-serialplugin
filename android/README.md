# Android serial plugin tests

```bash
cd android && ./gradlew test
```

## Layout

| File | Role |
|------|------|
| `SerialPlugin` | Tauri `@Command` surface → `UsbBridge` |
| `UsbBridge` | `path → UsbPortSession`, enumerate, permission, attach/detach |
| `UsbPortSession` | one port: SIOM → JNI `feedRx`, `write` → USB |
| `MobileBridge` | 4 JNI callbacks (`feedRx`, `onUsbError`, …) |

Logcat: `adb logcat -s UsbBridge UsbPort SerialPlugin`
