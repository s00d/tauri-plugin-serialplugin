# Android serial plugin tests

## JVM unit tests (Robolectric)

Requires **JDK 17** (Gradle 8.9 fails on JDK 26):

```bash
export JAVA_HOME=$(/usr/libexec/java_home -v 17)  # macOS
cd android && ./gradlew test
```

Shared fakes live in `android/src/sharedTest/` (`FakeUsbSerialPort`, `RecordingRxSink`).

## Kotlin ↔ Rust JNI integration tests (instrumented)

Full chain: `FakeUsb` → SIOM → `JniSerialRxSink` → `MobileBridge.feedRx` → Rust `MobileRxHub`.

Stable sources: [`examples/serialport-test/android-integration/`](../examples/serialport-test/android-integration/).

```bash
# Emulator or USB device required; JDK 17; NDK for Rust .so
cd examples/serialport-test
pnpm android:integration-test
```

Manual steps:

```bash
pnpm tauri android build --debug
../../scripts/sync-android-integration-tests.sh
cd src-tauri/gen/android
JAVA_HOME=$(/usr/libexec/java_home -v 17) ./gradlew connectedUniversalDebugAndroidTest
```

Instrumented `FakeUsbSerialPort` (Parcel stubs, no mockito): `examples/serialport-test/android-integration/fakes/`.

Debug harness JNI (Rust `test_harness.rs`): `testHarnessReset`, `testRegisterPort`, `testHubBufferedLen`, `testInvokeWrite`, `testRegistryHasPort`.

## Layout

| File | Role |
|------|------|
| `SerialPlugin` | Tauri `@Command` surface → `UsbBridge` |
| `UsbBridge` | `path → UsbPortSession`, enumerate, permission, attach/detach |
| `UsbPortSession` | one port: SIOM → JNI `feedRx`, `write` → USB |
| `MobileBridge` | 4 JNI callbacks (`feedRx`, `onUsbError`, …) |

Logcat: `adb logcat -s UsbBridge UsbPort SerialPlugin`

## CMUX virtual paths

* **`exchange` / `at` on `physical#dlci=N`:** Routed through the Rust CMUX session (same as desktop).
* **`cancel_exchange`:** Sets the virtual cancel flag, fails the active DLCI waiter, and clears the virtual TX queue.
* **`rx_prepare: drain`:** Uses the shared RX hub drain before write (idle bytes may still be replayed via `take_idle_bytes`).
