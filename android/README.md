# Android serial plugin

## Architecture (fd + Rust drivers)

```text
SerialPlugin → UsbFdBridge (Kotlin) → openDeviceFd / enumerateJson
UsbNative (JNI) ↔ fd_bridge.rs ↔ driver_host.rs ↔ android-usb-serial (nusb)
RX: Rust reader thread → PortRxHub poll-loop (same as desktop)
```

Kotlin keeps `UsbDeviceConnection` and the raw fd. Rust duplicates the fd (`dup`), opens `nusb::Device::from_fd`, claims interfaces, and runs vendor drivers from the `android-usb-serial` crate. There is **no** vendored `usb-serial-for-android` Java tree in this repo.

## Consumer app requirements

1. Declare USB host in the app manifest (`uses-feature android.hardware.usb.host` optional).
2. Ship a `device_filter.xml` aligned with [`device_filter.xml`](src/main/res/xml/device_filter.xml) / `ProbeTable::default_table()`.
3. Request runtime USB permission before open (the plugin uses `PendingIntent.FLAG_MUTABLE` + `setPackage()`).

## JVM unit tests (Robolectric)

Requires **JDK 17**:

```bash
export JAVA_HOME=$(/usr/libexec/java_home -v 17)  # macOS
cd android && ./gradlew test
```

## Kotlin ↔ Rust JNI integration tests (instrumented)

Stable sources: [`examples/serialport-test/android-integration/`](../examples/serialport-test/android-integration/).

Uses `FakeTransport` via `android-test-harness` (no Kotlin USB fakes).

```bash
cd examples/serialport-test
pnpm android:integration-test
```

Debug harness JNI (`test_harness.rs`): `testHarnessReset`, `testOpenFakePort`, `testFakeInjectRx`, `testFakeTakeTx`, `testFakeInjectError`, `testHubBufferedLen`, `testInvokeWrite`, `testRegistryHasPort`.

## Layout

| File | Role |
|------|------|
| `SerialPlugin` | Tauri plugin load → `UsbFdBridge` |
| `UsbFdBridge` | enumerate, permission, fd open/close, attach/detach |
| `UsbNative` | JNI for enumerate + fd lifecycle |
| `MobileBridge` | JNI callbacks (`onUsbError`, `onDeviceDetached`, port list change, …) |

## Golden fixture regen

Rust drivers are verified against frozen JSON fixtures (`crates/android-usb-serial/tests/fixtures/`).

```bash
cargo run -p android-usb-serial --features fake-transport --bin golden_record
cargo test -p android-usb-serial --features fake-transport --test golden_parity
```

See [`docs/golden-recorder-archive/README.md`](../docs/golden-recorder-archive/README.md) for the retired JVM recorder workflow.

Logcat: `adb logcat -s UsbFdBridge SerialPlugin RustStdoutStderr`

## CMUX virtual paths

* **`exchange` / `at` on `physical#dlci=N`:** Routed through the Rust CMUX session (same as desktop).
* **`cancel_exchange`:** Sets the virtual cancel flag, fails the active DLCI waiter, and clears the virtual TX queue.
* **`rx_prepare: drain`:** Uses the shared RX hub drain before write (idle bytes may still be replayed via `take_idle_bytes`).
