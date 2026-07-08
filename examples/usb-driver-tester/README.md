# USB Driver Tester

Standalone Android app for on-device USB serial driver verification.

## Build

```bash
# Rust cdylib → jniLibs
cd rust
cargo ndk -t aarch64-linux-android -o ../app/src/main/jniLibs build --release

# APK
cd ..
./gradlew assembleDebug
```

## Features

- Lists attached USB devices with probed driver name
- **Self-test** runs in-process fake-transport matrix (FTDI, CP21xx, CH34x, PL2303, CDC)
- **Device test** tap a device → real `NusbTransport` via `UsbDeviceConnection` fd (probe → open → write/read → close)
- **Share log** exports step report via Android share sheet

## Hardware checklist

| Chip | VID:PID (example) |
|------|---------------------|
| FT232R | 0403:6001 |
| CP2102 | 10C4:EA60 |
| CH340 | 1A86:7523 |
| PL2303 | 067B:2303 |
| CDC Arduino | 2341:0043 |

Connect device → grant USB permission → run self-test → share log for bug reports.

## CH340 EPROTO / detach isolation

Use this app **before** debugging `serialport-test` when a CH340 (`1A86:7523`) drops off the bus right after open:

1. Install this tester on the same phone + same OTG cable.
2. Tap the device → **Device test** (real fd path, no Tauri/WebView).
3. If the log shows `FAIL open_port` or the device vanishes from the list → **power/cable/hub** issue.
4. If tester passes but `serialport-test` disconnects → compare `[SerialOpen]` / `start_reader` in plugin logcat (`SerialPlugin` tag).

Tester uses the same `android-usb-serial` + `from_raw_fd` stack but **no** background reader until you call `read()` (sync path only).
