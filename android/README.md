# Android serial plugin tests

```bash
cd android && ./gradlew test
```

## Layout

| File | Role |
|------|------|
| `UsbPortSession` | one port: SIOM → JNI `feedRx`, `write` → USB |
| `UsbBridge` | `path → session`, enumerate, permission, attach/detach |
| `SerialPlugin` | 6 `@Command` → bridge |
| `MobileBridge` | 4 JNI callbacks |

Logcat: `adb logcat -s UsbBridge UsbPort SerialPlugin`
