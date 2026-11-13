# Android serial plugin

## Architecture (fd + Rust drivers)

```text
SerialPlugin → UsbFdBridge (Kotlin) → openDeviceFd / enumerateJson
UsbNative (JNI) ↔ fd_bridge.rs ↔ driver_host.rs ↔ android-usb-serial (nusb)
RX: Rust reader thread → PortRxHub poll-loop (same as desktop)
```

Kotlin keeps `UsbDeviceConnection` and the raw fd. Rust duplicates the fd (`dup`), opens `nusb::Device::from_fd`, claims interfaces, and runs vendor drivers from the `android-usb-serial` crate. There is **no** vendored `usb-serial-for-android` Java tree in this repo.

## Consumer app requirements

None. The plugin's library manifest brings the USB host feature and the
`device_filter.xml`, its `build.rs` adds the `USB_DEVICE_ATTACHED` intent filter to
the app activity, and `open()` asks the user for USB permission
(`PendingIntent.FLAG_MUTABLE` + `setPackage()`). Keep
[`device_filter.xml`](src/main/res/xml/device_filter.xml) in sync with
`ProbeTable::default_table()` (a test enforces it).

## JVM unit tests + Android Lint

Gradle 8.14 / AGP 8.11 only **run** on JDK 17–21 (JDK 22+ dies with
`Unsupported class file major version`). Prefer 17. The helper picks a
compatible install even if your shell `JAVA_HOME` is 27:

```bash
export JAVA_HOME="$(./scripts/java-home-for-gradle.sh)"
cd android && ./gradlew lintDebug test
```

Lint is the Android-side analogue of clippy (API misuse, permissions, …) and
runs in CI on Ubuntu together with Robolectric.

## Kotlin ↔ Rust JNI integration tests (instrumented)

Stable sources: [`examples/serialport-test/android-integration/`](../examples/serialport-test/android-integration/).

Uses `FakeTransport` via `android-test-harness` (no Kotlin USB fakes). This is the supported way to exercise the JNI/fd path on an **emulator**: macOS cannot pass a host USB serial adapter into the AVD, so real Quectel/WCH sticks stay on desktop (`SERIALPLUGIN_MODEM`) or a physical phone with USB host.

```bash
# Emulator (arm64 AVD) or device — FakeTransport, no real USB:
ANDROID_INTEGRATION_NDK_ARCH=arm64-v8a ./scripts/android-integration-ci.sh all

# Desktop Mac smoke against a real AT modem (e.g. Quectel on /dev/cu.wchusbserial*):
SERIALPLUGIN_MODEM=/dev/cu.wchusbserial210 cargo nextest run -p tauri-plugin-serialplugin modem_live --no-capture
```

Debug harness JNI (`test_harness.rs`): `testHarnessReset`, `testOpenFakePort`, `testFakeInjectRx`, `testFakeTakeTx`, `testFakeInjectError`, `testFakeEnableAtModem`, `testHubBufferedLen`, `testInvokeWrite`, `testRegistryHasPort`, `testEnumerateJsonFromWorkerThread`.

Optional AT modem sim on FakeTransport (`enable_at_modem` / `testFakeEnableAtModem`): scripted Quectel-like replies (`AT`→`OK`, `AT+CSQ` with `+CREG` URC, unknown `AT*`→`ERROR`).

### What we do **not** rely on

| Approach | Why skip here |
|----------|----------------|
| AVD `-usb-passthrough` on macOS | Emulator accepts the flag but attaches nothing (`Command line USB devices: ()`); host keeps `/dev/cu.*` |
| Robolectric `ShadowUsbManager` | Useful for Kotlin-only USB glue; drivers run in Rust via fd + FakeTransport already |
| Genymotion USB | Not on Apple Silicon |

### Heavier e2e options (not in default CI)

- **Linux CI + Bliss OS / QEMU + USB:** when you need real `UsbManager` in automation — see [Depau’s GH Actions + KVM write-up](https://blog.depau.eu/2025/04/05/android-usb-testing-with-qemu-kvm/). QEMU `usb-serial` or `-device usb-host`.
- **Mac → Linux USB/IP:** export WCH with [pyusbip](https://github.com/jenish-rudani/pyusbip) / [usbipd-mac](https://github.com/jenish-rudani/usbipd-mac), attach in Waydroid/Bliss on Linux. Heavy; phone OTG is usually simpler.

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
