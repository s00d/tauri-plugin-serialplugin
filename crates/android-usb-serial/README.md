# android-usb-serial

Pure Rust USB serial drivers for **Android** (and Linux), built on [nusb](https://docs.rs/nusb).

Ported from [usb-serial-for-android](https://github.com/mik3y/usb-serial-for-android) driver
protocol logic — verified byte-for-byte against **567 golden JSON fixtures**.

[![Crates.io](https://img.shields.io/crates/v/android-usb-serial.svg)](https://crates.io/crates/android-usb-serial)
[![Docs.rs](https://docs.rs/android-usb-serial/badge.svg)](https://docs.rs/android-usb-serial)
[![License](https://img.shields.io/crates/l/android-usb-serial.svg)](LICENSE-APACHE)

## Supported chips

| Driver | Examples (VID:PID) |
|--------|--------------------|
| FTDI | FT232R `0403:6001`, FT2232H `0403:6010` |
| Silicon Labs CP21xx | CP2102 `10C4:EA60`, CP2105 `10C4:EA70` |
| WCH CH34x | CH340 `1A86:7523` |
| Prolific PL2303 | HX / HXN / type 0x01 / TA |
| CDC ACM | Castrated single-iface, IAD, multi-port |
| GSM modem | Fibocom-style vendor ports |
| Chrome CCD / CR50 | `18D1:5014` (3 ports) |

## Install

```toml
[dependencies]
android-usb-serial = "0.1"

# Optional: serialport::SerialPort facade (enabled by default)
# android-usb-serial = { version = "0.1", default-features = false }

# Host / instrumentation tests
# android-usb-serial = { version = "0.1", features = ["fake-transport"] }
```

Requires **Rust 1.79+**. Real USB needs `target_os = "android"` or `"linux"` (pulls in `nusb` + `libc`).

## Quick start (Android fd)

Kotlin/Java keeps `UsbDeviceConnection` and grants USB permission. Pass the raw fd into Rust:

```rust
use android_usb_serial::{
    from_raw_fd, open_port, DataBits, LineConfig, NusbTransport, Parity, StopBits, Transport,
};
use std::os::fd::RawFd;
use std::sync::Arc;

fn open_android_port(fd: RawFd) -> android_usb_serial::Result<()> {
    let device = from_raw_fd(fd)?;
    let transport = Arc::new(NusbTransport::from_device(device)?) as Arc<dyn Transport>;
    let mut port = open_port(transport, 0)?;
    port.set_line_config(LineConfig {
        baud_rate: 115_200,
        data_bits: DataBits::Eight,
        parity: Parity::None,
        stop_bits: StopBits::One,
    })?;
    port.set_dtr(true)?;
    port.write(b"AT\r\n")?;
    Ok(())
}
```

`from_raw_fd` **`dup`s** the fd so Java can keep owning `UsbDeviceConnection`.

## Using on Android

This crate talks USB only through a **raw file descriptor** from Android's
`UsbDeviceConnection`. It does **not** call `UsbManager`, does **not** show
permission dialogs, and does **not** register broadcast receivers — your app
(or a thin Kotlin bridge) owns all of that.

```text
App (Kotlin)                         android-usb-serial (Rust)
─────────────────────────────────────────────────────────────
UsbManager.deviceList                ProbeTable::find(vid, pid, ifaces)
enumerateJson + interfaces[]    →    expand to port keys (device / device#N)
requestPermission + openDevice  →    from_raw_fd → NusbTransport → open_port
keep UsbDeviceConnection alive       dup(fd); claim interfaces inside nusb
closeDeviceFd on detach/close        driver I/O (write, reader, modem lines)
```

### Manifest

Declare USB host support and (optionally) attach a device filter so Android
can launch your activity when a known adapter is plugged in:

```xml
<uses-feature android:name="android.hardware.usb.host" android:required="false" />

<application>
    <activity android:name=".MainActivity" android:exported="true">
        <intent-filter>
            <action android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED" />
        </intent-filter>
        <meta-data
            android:name="android.hardware.usb.action.USB_DEVICE_ATTACHED"
            android:resource="@xml/device_filter" />
    </activity>
</application>
```

`device_filter.xml` should list VID/PID pairs you care about. Align entries with
[`ProbeTable::default_table()`](src/probe.rs) — a full filter ships with
[tauri-plugin-serialplugin](../../../android/src/main/res/xml/device_filter.xml).
A catch-all `<usb-device />` at the end also matches generic CDC ACM devices.

### USB permission

Android requires **runtime permission per device** before `openDevice()`:

1. Check `usbManager.hasPermission(device)`.
2. If false, call `usbManager.requestPermission(device, pendingIntent)`.
3. Handle the result in a `BroadcastReceiver` (`EXTRA_PERMISSION_GRANTED`).
4. On API 31+, use `PendingIntent.FLAG_MUTABLE` on the permission intent.
5. Set `intent.setPackage(packageName)` when building the permission `Intent`
   (see [UsbFdBridge](../../../android/src/main/kotlin/app/tauri/serialplugin/manager/UsbFdBridge.kt)).

Minimal pattern (standalone app):

```kotlin
private const val ACTION_USB_PERMISSION = "your.app.USB_PERMISSION"

fun openWithPermission(device: UsbDevice) {
    val mgr = getSystemService(USB_SERVICE) as UsbManager
    if (!mgr.hasPermission(device)) {
        val pi = PendingIntent.getBroadcast(
            this, 0,
            Intent(ACTION_USB_PERMISSION),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_MUTABLE,
        )
        mgr.requestPermission(device, pi)
        return
    }
    val conn = mgr.openDevice(device) ?: return
    val fd = conn.fileDescriptor
    // pass fd to Rust (JNI) — keep `conn` alive until close
    nativeOpenPort(fd, device.vendorId, device.productId)
}
```

Register for `UsbManager.ACTION_USB_DEVICE_ATTACHED` / `DETACHED` to refresh the
port list and close fds when the cable is unplugged.

### Open fd — do **not** pre-claim interfaces

After permission is granted:

```kotlin
val conn = usbManager.openDevice(device) ?: throw IOException("open failed")
// Do NOT call conn.claimInterface() here.
val fd = conn.fileDescriptor
```

Rust/nusb calls `detach_and_claim` on the duplicated fd. If Kotlin claims the
interface first, open fails with **`io interface is busy`**. The reference
implementation lives in
[`UsbFdBridge.openDeviceFd`](../../../android/src/main/kotlin/app/tauri/serialplugin/manager/UsbFdBridge.kt).

Keep the `UsbDeviceConnection` open for the whole session; call `close()` only
after Rust has closed the port and released endpoints.

### Enumerate (no fd, no claim)

List attached devices from Kotlin and pass metadata to Rust for driver probing.
Include **USB interface descriptors** so multi-port chips (FT2232, dual CDC,
etc.) expand to separate port keys:

```json
{
  "ports": {
    "/dev/bus/usb/001/002": {
      "type": "Usb",
      "vid": "0x1A86",
      "pid": "0x7523",
      "manufacturer": "",
      "product": "",
      "serial_number": "",
      "interfaces": [
        { "id": 0, "class": 255, "subclass": 0, "protocol": 0 }
      ]
    }
  }
}
```

Rust maps each entry to one or more paths: `deviceName` or `deviceName#N`
(port index). `manufacturer` / `product` / `serial_number` may be empty when
Android denies string reads (`SecurityException`) — probing uses VID/PID +
`interfaces[]` only.

### JNI / Rust entry

Typical sequence after you have `fd`:

```rust
let device = from_raw_fd(fd as RawFd)?;
let transport = Arc::new(NusbTransport::from_device(device)?);
let mut port = open_port(transport, port_index)?; // 0 for single-port CH340
port.set_line_config(LineConfig { baud_rate: 115_200, .. })?;
port.set_dtr(true)?;
port.start_reader()?; // optional background bulk IN
port.write(b"AT\r\n")?;
```

- `write()` opens **bulk OUT** only; bulk IN belongs to the reader (do not
  reopen IN in `write` — see `EndpointPair::write`).
- Prefer `start_reader()` **after** line config and DTR/RTS — some chips (CH340)
  misbehave if bulk IN starts too early.
- On detach, expect errors like `USB device detached (unplug, power loss, or
  protocol error on bulk IN)`.

Build the native library for Android:

```bash
cargo ndk -t arm64-v8a -o app/src/main/jniLibs build --release
# or: aarch64-linux-android, armeabi-v7a, x86_64 as needed
```

Link `liblog` if you emit Android logcat from Rust (`__android_log_write`).

### Examples in this repository

| Example | Purpose |
|---------|---------|
| [`examples/usb-driver-tester`](../../../examples/usb-driver-tester/) | Minimal standalone app: permission → fd → `from_raw_fd` → probe/open/write/read. **Self-test** runs fake-transport matrix; **Device test** hits real hardware. Use to isolate CH340 power vs plugin bugs. |
| [`examples/serialport-test`](../../../examples/serialport-test/) | Full Tauri Android app (`pnpm tauri android build`). See [`ANDROID.md`](../../../examples/serialport-test/ANDROID.md) for wireless ADB, install, and logcat tags. |
| [`android/`](../../../android/) | Tauri plugin Kotlin bridge (`UsbFdBridge`, `UsbNative`, `SerialPlugin`). |
| [`examples/serialport-test/android-integration/`](../../../examples/serialport-test/android-integration/) | Instrumented JNI tests with `FakeTransport` (no real USB). |

**usb-driver-tester** build:

```bash
cd examples/usb-driver-tester/rust
cargo ndk -t arm64-v8a -o ../app/src/main/jniLibs build --release
cd .. && gradle assembleDebug
```

**serialport-test** on device:

```bash
cd examples/serialport-test
pnpm tauri android build --debug
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
adb logcat -s UsbFdBridge SerialPlugin Console
```

### Debugging checklist

| Symptom | Likely cause |
|---------|----------------|
| `io interface is busy` | Kotlin called `claimInterface` before Rust |
| `endpoint already in use` | bulk IN opened twice (writer + reader) |
| `USB permission denied` | user declined dialog or `PendingIntent` not mutable |
| Device listed, UI empty | enumerate JSON missing `interfaces[]` or wrong port key |
| CH340 vanishes after open (`URB ep 82 status=-71`) | OTG power / cable — confirm with **usb-driver-tester** on same phone + cable |
| `enumerateJson: 0` after disconnect | normal after detach; replug or powered USB hub |

### Tauri plugin consumers

If you use [tauri-plugin-serialplugin](https://github.com/s00d/tauri-plugin-serialplugin)
instead of wiring JNI yourself, merge the plugin's `device_filter.xml`, ensure
USB host feature in your app manifest, and let the plugin handle enumerate /
permission / fd. Your frontend calls the same serial API as on desktop.

## Fake transport (tests)

```rust
#[cfg(feature = "fake-transport")]
fn example() {
    use android_usb_serial::{open_port, FakeTransport, Transport};
    use std::sync::Arc;

    let fake = FakeTransport::cdc_single_iface();
    let transport: Arc<dyn Transport> = Arc::new(fake.clone());
    let mut port = open_port(transport, 0).unwrap();
    port.write(b"PING").unwrap();
    assert_eq!(fake.take_tx(), b"PING");
}
```

## Features

| Feature | Default | Description |
|---------|---------|-------------|
| `serialport-compat` | yes | Implements `serialport::SerialPort` |
| `fake-transport` | no | `FakeTransport` + `golden_record` binary |

## Architecture

```text
UsbManager (Kotlin) → permission → UsbDeviceConnection → fd (unclaimed)
        ↓
   from_raw_fd / NusbTransport (this crate)
        ↓
   ProbeTable → Ftdi / Cp21xx / Ch34x / … drivers
        ↓
   SerialPortHandle (write / reader / modem / purge)
```

See **[Using on Android](#using-on-android)** for manifest, permissions, enumerate
JSON, and example apps. Kotlin owns USB policy; Rust owns protocol + bulk I/O.

## Golden fixtures

```bash
# Full parity gate (Java-sourced controls + bulkOut + rx_filter + probe)
cargo test -p android-usb-serial --features fake-transport --test golden_parity

# All crate tests
cargo test -p android-usb-serial --features fake-transport

# Optional: regen Rust-only driver fixtures (never overwrites source=java)
cargo run -p android-usb-serial --features fake-transport --bin golden_record
```

Breakdown: **560** Java control sequences, **6** RX-filter pairs, **1** probe table → **567** JSON files under `tests/fixtures/`.

## Hardware spike

```bash
# On device: ANDROID_USB_SPIKE_FD=<UsbDeviceConnection fd>
cargo test -p android-usb-serial --features fake-transport spike_fd_hardware -- --ignored
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Driver protocols include logic derived from usb-serial-for-android (MIT); see [NOTICE](NOTICE).

## Publish (maintainers)

```bash
# From workspace root
cargo test -p android-usb-serial --features fake-transport
cargo package -p android-usb-serial --allow-dirty   # review pack list
cargo publish -p android-usb-serial --dry-run
cargo publish -p android-usb-serial
```

Version bumps: edit `version` in this crate’s `Cargo.toml` and the `android-usb-serial = { …, version = "…" }` entry in the workspace plugin `Cargo.toml`.

## Related

- [tauri-plugin-serialplugin](https://github.com/s00d/tauri-plugin-serialplugin) — Tauri integration (`UsbFdBridge` → this crate)
- [examples/usb-driver-tester](../../../examples/usb-driver-tester/) — standalone hardware tester
- [examples/serialport-test/ANDROID.md](../../../examples/serialport-test/ANDROID.md) — Tauri Android dev workflow
- Standalone apps can depend on this crate alone + a thin Kotlin fd bridge
