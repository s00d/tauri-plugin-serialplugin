# Changelog

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.


## [3.0.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.0...v3.0.1) (2026-07-17)

### Bug Fixes

* include `guest-js` in the published npm package so Vite can resolve the `development` export ([#36](https://github.com/s00d/tauri-plugin-serialplugin/issues/36))

## [3.0.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.24.1...v3.0.0) (2026-07-08)

Major v3 release: Channel-based **`watch()`** API, native AT **`exchange`** / FIFO queue, unified **RX hub** on desktop and Android, and a **hard cut** from vendored Java `usb-serial-for-android` to the pure-Rust **`android-usb-serial`** crate (nusb).

### Breaking Changes

* **Streaming API:** `startListening()` / `listen()` / `disconnected()` / `cancelListen()` removed. Use **`watch({ onData, onDisconnect?, onError?, onUrc? })`** → `SerialEvent` over Tauri [`Channel`](https://v2.tauri.app/develop/calling-frontend/) (desktop + Android).
* **Capabilities:** `SerialPort.getCapabilities()` (`transport`, `platform`, `version`); commands `capabilities`, `watch`, `unwatch` replace `start_listening` / `stop_listening`.
* **Removed:** `available_ports_direct` — use `available_ports()`.
* **Removed `AtCommandQueue` / `port.at`:** Use **`sendAt()`**, **`sendAtPhases()`**, **`sendSmsPdu()`**, **`cancelAt()`**, **`configureAtSession()`**.
* **`onUrc` removed from `AtSessionOptions`:** Use **`watch({ onUrc })`** for live URC lines.
* **`exchange()`** returns **`ExchangeResponse`**, not `Uint8Array`. Use `.raw` for bytes.
* **`clearRx: true`** maps to **`purge`**; default is now **`drain`** when unset.
* **Android USB stack:** Vendored `android/usbserial` Java tree **removed**. Kotlin no longer runs serial I/O commands on the main thread — only **`UsbFdBridge`** (enumerate, permission, fd). All drivers and bulk I/O live in Rust (`android-usb-serial` + nusb).
* **Rust module layout:** Removed compat top-level modules (`desktop_api`, `mobile_api`, `port_rx_hub`, `exchange_runtime`, …). Use `api::serial`, `hub`, `exchange`, `port`, `state`, `android`.
* **guest-js:** `AtCommandResult.raw` / `ExchangeResponse.raw` are `Uint8Array`; `timedOut` is a native `boolean`.
* **`rust-version`:** **1.79** minimum.

### Migration (v2 → v3)

| v2 | v3 |
|----|-----|
| `startListening()` + `listen(fn)` | `watch({ onData: fn })` |
| `disconnected(fn)` | pass `onDisconnect: fn` into the same `watch({ ... })` |
| `cancelListen()` / `stopListening()` | `handle.unwatch()` |
| `port.at.enable()` / `enqueue()` | `sendAt()` / `sendAtPhases()` |
| Kotlin `UsbBridge` + Java drivers | `UsbFdBridge` fd → Rust `driver_host` |

**Android app requirements:** `uses-feature android.hardware.usb.host`, `device_filter.xml`, runtime USB permission before `openDevice()`. Do **not** `claimInterface()` in Kotlin before handing fd to Rust. See [`crates/android-usb-serial/README.md`](crates/android-usb-serial/README.md).

### Features

* **`android-usb-serial` crate:** Pure Rust USB serial on Android via nusb; **567** golden parity fixtures; drivers for FTDI, CP21xx, CH34x, PL2303, CDC-ACM, GSM modem, Chrome CCD ([20b54c0](https://github.com/s00d/tauri-plugin-serialplugin/commit/20b54c0bbc17227465da71728333cf93d88103aa)).
* **`UsbFdBridge`:** Kotlin provides USB fd only; Rust `driver_host` owns probe, claim, drivers, RX reader, and I/O.
* **Unified `api::serial`:** Single `serialport::SerialPort` facade on desktop and Android; `PortRxHub` poll-loop on both platforms ([4071ff6](https://github.com/s00d/tauri-plugin-serialplugin/commit/4071ff6)).
* **Rust-first Android JNI** ([29faa29](https://github.com/s00d/tauri-plugin-serialplugin/commit/29faa29)): bulk-IN reader thread, chunked write, CMUX virtual paths through Rust session.
* **Unified RX hub (desktop + Android):** Single consumer per port; `watch`, `exchange`, `read`, and drain share one hub ([7024fc0](https://github.com/s00d/tauri-plugin-serialplugin/commit/7024fc0), [caa5700](https://github.com/s00d/tauri-plugin-serialplugin/commit/caa5700)).
* **`take_idle_bytes`:** Stale RX in the hub idle buffer is replayed into the next `exchange` after write.
* **Native FIFO queue (Rust + Android):** All `exchange` / AT jobs on one port serialize in FIFO order; parallel invokes **wait** instead of `"Exchange already in progress"` ([e1eb63f](https://github.com/s00d/tauri-plugin-serialplugin/commit/e1eb63f)).
* **Native `exchange` / `cancel_exchange`:** Write + read-until terminators, idle silence, wall timeout, max response size; structured `ExchangeResponse` (`status`, `lines`, `solicitedBody`, `urcLines`, `raw`).
* **Line-framed AT completion:** Final line `OK` / `ERROR` / `+CME ERROR` / `+CMS ERROR`; `completionMode: 'substring'` for legacy/binary.
* **`rxPrepare`:** Default **`drain`**; **`purge`** opt-in; **`none`** unchanged.
* **`watchAvailablePorts()` / `watch_ports`:** Hotplug via Channel — `snapshot`, then `added` / `removed` (desktop poll; Android USB attach/detach).
* **`open()` canonical path:** Returns session key (Android device path / `device#N` for multi-port).
* **CMUX virtual `exchange`:** Paths `physical#dlci=N` routed through Rust CMUX session like desktop.
* **`usb-driver-tester`:** Standalone hardware self-test app under `examples/usb-driver-tester/`.
* **guest-js:** Modular v3 SDK; auto-reconnect restores **`open()` + `watch()`** after disconnect.
* **macOS:** `available_ports({ singlePortPerDevice: true })` — one path per device (prefers `/dev/cu.*`).
* **Extended AT grammar:** Vendor prefixes, V.250 finals, `derive_solicited_prefixes(command)`; `ExchangeDemux` for live URC before echo.
* **`pauseWatch` default `false`** — watch stays on during AT; pass `pauseWatch: true` for legacy behavior.

### Bug Fixes

* **Android CH340 / weak OTG:** Bulk IN reader starts after line/DTR setup; in-flight URBs reduced to 2; clearer detach reason in logs.
* **Android enumerate:** Kotlin exports `interfaces[]`; Rust expands multi-port paths (`device#N`) via `ProbeTable` without opening fd.
* **Android write after listen:** `EndpointPair::write` no longer re-opens bulk IN owned by `SerialReader` (`endpoint already in use`).
* **Android Kotlin fd bridge:** Removed pre-`claimInterface` (fixes `io interface is busy` with nusb `detach_and_claim`).
* **Android logcat:** Rust plugin logs use tag `SerialPlugin` via `__android_log_write`.
* **Android:** USB permission `PendingIntent` uses `FLAG_MUTABLE` on API 31+; detach + IO errors → `SerialEvent::Disconnect` on Channel ([#27](https://github.com/s00d/tauri-plugin-serialplugin/issues/27)).
* **Android:** `cancel_exchange` wakes hub waiter; CMUX virtual cancel clears DLCI TX queue ([c6c94dd](https://github.com/s00d/tauri-plugin-serialplugin/commit/c6c94dd)).
* **Android:** JNI exceptions, session path re-key, teardown leaks ([e9fc307](https://github.com/s00d/tauri-plugin-serialplugin/commit/e9fc307)).
* **Android:** Serialize fail/shutdown on dedicated usb-io thread ([cd74b84](https://github.com/s00d/tauri-plugin-serialplugin/commit/cd74b84)).
* **desktop:** `write` / `write_binary` flush via `write_all` ([#29](https://github.com/s00d/tauri-plugin-serialplugin/issues/29)).
* **desktop:** Unblock hub drain when watch is active ([55f1cb3](https://github.com/s00d/tauri-plugin-serialplugin/commit/55f1cb3)).
* **desktop:** Lock order, Opening-state open, async `enable_mux` ([3a4bd88](https://github.com/s00d/tauri-plugin-serialplugin/commit/3a4bd88)).
* **desktop (Windows):** Enrich truncated USB `serial_number` from WMI ([#23](https://github.com/s00d/tauri-plugin-serialplugin/issues/23)).
* **Hub / watch:** `lock_or_recover` in watch_registry and hub channel paths ([2071e15](https://github.com/s00d/tauri-plugin-serialplugin/commit/2071e15)); panic/poison hardening ([8672de9](https://github.com/s00d/tauri-plugin-serialplugin/commit/8672de9)).
* **TX queue:** Errors no longer halt the port queue until reopen ([e1eb63f](https://github.com/s00d/tauri-plugin-serialplugin/commit/e1eb63f)).
* **guest-js:** `readBinary` requires open port; watch-preserving `change()` ([9b22af7](https://github.com/s00d/tauri-plugin-serialplugin/commit/9b22af7)).

### Build / CI / Docs

* **CI:** `cargo fmt`, `clippy`, `pnpm check` / `build`, Windows `cargo check`, Android integration emulator job ([1aa1682](https://github.com/s00d/tauri-plugin-serialplugin/commit/1aa1682), [2e70ecf](https://github.com/s00d/tauri-plugin-serialplugin/commit/2e70ecf)).
* **Tests:** Kotlin↔JNI↔Rust instrumented integration tests with `FakeTransport` ([412b4a5](https://github.com/s00d/tauri-plugin-serialplugin/commit/412b4a5), [be77e2a](https://github.com/s00d/tauri-plugin-serialplugin/commit/be77e2a)); Jest v3 lifecycle + Rust contract tests.
* **Docs:** v3 migration guide, Android bridge docs, [`examples/serialport-test/ANDROID.md`](examples/serialport-test/ANDROID.md), [`android-usb-serial` Android usage](crates/android-usb-serial/README.md#using-on-android) ([c2d1e23](https://github.com/s00d/tauri-plugin-serialplugin/commit/c2d1e23), [b3cc7d2](https://github.com/s00d/tauri-plugin-serialplugin/commit/b3cc7d2)).
* **Workspace:** `android-test-harness` feature wires `FakeTransport` for instrumented tests.


### [2.24.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.24.0...v2.24.1) (2026-07-08)

## [2.24.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.21.1...v2.24.0) (2026-07-08)


### Features

* **android:** buffered emit, USB serial hardening, Gradle tests ([efa35ec](https://github.com/s00d/tauri-plugin-serialplugin/commit/efa35ecd0527f8c1b1e241c920b8880ec1e13761))
* **js:** open/close guards, mocks; update Jest tests ([2e56cad](https://github.com/s00d/tauri-plugin-serialplugin/commit/2e56cad0c169ee7df4d60dc3dab10ef51240f542))


### Bug Fixes

* **android:** run port IO commands off the main thread ([#34](https://github.com/s00d/tauri-plugin-serialplugin/issues/34)) ([bcd1aa2](https://github.com/s00d/tauri-plugin-serialplugin/commit/bcd1aa20d55a57463c818968a9b2c388dc931d17))
* **desktop:** write_all, timeout, Android serialport split ([f33ea91](https://github.com/s00d/tauri-plugin-serialplugin/commit/f33ea91289ab1a6dc11d1ccda46ddf5e7671be13))

## [2.23.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.22.0...v2.23.0) (2026-07-02)

### Bug Fixes

* **desktop:** `write` / `write_binary` now flush the full buffer via `write_all` (retries partial kernel writes instead of returning after the first `write` syscall). Mitigates truncated sends on Windows and slow links ([#29](https://github.com/s00d/tauri-plugin-serialplugin/issues/29)).
* **desktop:** Default open/read/write timeout raised from **200 ms** to **1000 ms** (`DEFAULT_SERIAL_TIMEOUT_MS` in JS and Rust).
* **desktop:** `start_listening` listener read-poll timeout fixed: was capped at **1 ms** (`.min(1)` regression); now clamped to **1–100 ms** while the user timeout still controls buffer coalescing.

### Features

* **guest-js:** Export `DEFAULT_SERIAL_TIMEOUT_MS`; document `timeout` on `SerialportOptions`; clarify `write` / `writeBinary` return value vs payload length.

### Build / Android

* **Rust:** `serialport` is a **desktop-only** dependency (`cfg(not(android/ios))`); Android builds no longer link the unused crate. Desktop-only state (`ConnectedPort`, `PortState`, …) and `From<serialport::Error>` are gated accordingly.

## [2.22.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.21.1...v2.22.0) (2026-03-21)

### Android / USB

* **Lifecycle:** `SerialPlugin` registers `Application.ActivityLifecycleCallbacks` and runs `SerialPortManager.cleanup()` when the host `Activity` is destroyed (close USB ports, unregister permission receiver, shut down IO executor).
* **Listening:** Incoming data is coalesced in `BufferedEmitter` / `SerialByteAccumulator` before `serialData` events; flush interval via `serialDataFlushIntervalMs` (native clamp typically 10–2000 ms).
* **USB serial (usb-serial-for-android):** Read/write use configured timeouts; `clearBuffer` maps to `purgeHwBuffers` when supported; `setFlowControl` (RTS/CTS, XON/XOFF); `SerialInputOutputManager` errors trigger `serialError` and port cleanup.
* **`bytesToRead` / `bytesToWrite` (Android):** With active **`watch`**, `bytesToRead` is the plugin-side buffer before the next Channel flush; `bytesToWrite` is typically `0`.
* **Tooling:** Gradle wrapper and `android/` settings for local `./gradlew test`; JVM unit tests use a real `org.json` artifact (Android JSON stubs break `JSONObject.put` in tests). Kotlin tests cover models, JSON helpers, emit pipeline, `BufferedEmitter`.

### Features

* **guest-js:** Stricter `SerialportOptions` / `Options` typing (removed open index signatures); `Record<string, PortInfo>` for `available_ports` maps.

### [2.21.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.21.0...v2.21.1) (2025-11-05)

## [2.21.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.20.0...v2.21.0) (2025-10-11)


### Features

* **serial:** emit disconnection events on serial port closure ([9870d1f](https://github.com/s00d/tauri-plugin-serialplugin/commit/9870d1f398c03c516b9d61149ddb99facd663517))

## [2.20.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.19.0...v2.20.0) (2025-10-11)


### Features

* **logger:** add centralized logging functionality for the serial plugin ([887dcf9](https://github.com/s00d/tauri-plugin-serialplugin/commit/887dcf9f30e6be059952ceb27c8c508be153b323))
* **logging:** add centralized logging module with adjustable log levels ([6cb8a51](https://github.com/s00d/tauri-plugin-serialplugin/commit/6cb8a51425680a52ec65832ea4a94c61f187e312))
* **logging:** add centralized logging module with adjustable log levels ([7a7eacd](https://github.com/s00d/tauri-plugin-serialplugin/commit/7a7eacd398e72f4237c8a4ed97aaec7aa2cbac7e))


### Bug Fixes

* **readme:** correct indentation and enhance serial port listening example ([6e997b5](https://github.com/s00d/tauri-plugin-serialplugin/commit/6e997b58c8c8641c0626c7bfb5d9243f191d26e6))
* **readme:** correct indentation and enhance serial port listening example ([df46965](https://github.com/s00d/tauri-plugin-serialplugin/commit/df46965af6fec40ef8452d423fbdea3dbcd3a600))

## [2.19.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.17.1...v2.19.0) (2025-08-26)


### Features

* **serialplugin:** add logging for serial port operations and cleanup ([e474046](https://github.com/s00d/tauri-plugin-serialplugin/commit/e47404639249b2246a4a3acc9b07765fdf188f0c))
* **serialport-manager:** add custom prober for unknown USB devices ([74b3915](https://github.com/s00d/tauri-plugin-serialplugin/commit/74b3915fd84ae5ec7a7d63ade01c757272dc6d14))


### Bug Fixes

* **android:** require USB host feature in AndroidManifest.xml ([2227513](https://github.com/s00d/tauri-plugin-serialplugin/commit/22275138b6da65fd8eabfac25a6f241d4018ab76))
* **serial:** correct minimum timeout value for serial port ([dd83ded](https://github.com/s00d/tauri-plugin-serialplugin/commit/dd83ded0b68a5f09a06645cc96a3eaf218e9fc06))

### [2.17.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.17.0...v2.17.1) (2025-07-02)

## [2.17.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.16.0...v2.17.0) (2025-07-02)


### Features

* **serial:** update listen method to return unlisten function ([98e881d](https://github.com/s00d/tauri-plugin-serialplugin/commit/98e881d3f5f8e5594ade050fac20e7887ae5f44e))


### Bug Fixes

* **listener-manager:** handle errors in unlisten functions ([30c5445](https://github.com/s00d/tauri-plugin-serialplugin/commit/30c54455cd8bfc2b46d372272642eb7c19aeb935))

## [2.16.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.15.0...v2.16.0) (2025-07-01)


### Features

* **serial:** update listen method to return unlisten function ([f7090e0](https://github.com/s00d/tauri-plugin-serialplugin/commit/f7090e04f4d74792cbac47de2c3457e685d80ef0))
* **serial:** update listen method to return unlisten function ([0ac67ac](https://github.com/s00d/tauri-plugin-serialplugin/commit/0ac67acf1ad41fc33d7f17e61bc420295cfe296b))

## [2.15.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.14.0...v2.15.0) (2025-07-01)


### Features

* **listener-manager:** implement listener management for serial events ([b32b01c](https://github.com/s00d/tauri-plugin-serialplugin/commit/b32b01c063ddba33e9440acd1be5c4eddafdd9e9))

## [2.14.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.13.0...v2.14.0) (2025-07-01)


### Features

* **serial:** enhance error handling in serial port functions ([1403b77](https://github.com/s00d/tauri-plugin-serialplugin/commit/1403b777361eec135b1287434b9cee0452977948))

## [2.13.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.12.1...v2.13.0) (2025-07-01)


### Features

* **api-iife:** update event listener management in serial plugin ([81f4522](https://github.com/s00d/tauri-plugin-serialplugin/commit/81f452229b7a34e2b4d6e3cad28e40ce9780d021))

### [2.12.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.12.0...v2.12.1) (2025-06-19)


### Bug Fixes

* **deps:** update tauri-plugin-serialplugin to version 2.12.0 ([07e7526](https://github.com/s00d/tauri-plugin-serialplugin/commit/07e75261713db36fa9743c0230bd7dbe620a61b3))
* **mobile_api:** correct port opening error handling logic ([33f43c7](https://github.com/s00d/tauri-plugin-serialplugin/commit/33f43c7604c38baa6652662a16b28319d49563f7))

## [2.12.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.11.1...v2.12.0) (2025-06-12)


### Features

* **serialport:** add test mode for simulating serial port behavior ([22356b5](https://github.com/s00d/tauri-plugin-serialplugin/commit/22356b5cd229acb34d688226455673ec2ae96afd))
* **serialport:** add test mode for simulating serial port behavior ([287ab53](https://github.com/s00d/tauri-plugin-serialplugin/commit/287ab538db2022f27dae8c933f566d7ab052f77c))
* **serialport:** improve error handling and add port configuration ([5844e2d](https://github.com/s00d/tauri-plugin-serialplugin/commit/5844e2d2b715560f22dde9f499c316b338bf0410))
* **tests:** add comprehensive tests for serial port functionality ([614901e](https://github.com/s00d/tauri-plugin-serialplugin/commit/614901e6cdabcac22befba01debe47c8b599a94a))

### [2.11.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.11.0...v2.11.1) (2025-05-25)

## [2.11.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.2...v2.11.0) (2025-04-04)


### Features

* **serial:** set a default timeout for serial port connection ([c252883](https://github.com/s00d/tauri-plugin-serialplugin/commit/c2528839755ed1c6296b1eb5240e3e1548e3afc3))


### Bug Fixes

* **deps:** update dependencies to latest versions ([0d857bb](https://github.com/s00d/tauri-plugin-serialplugin/commit/0d857bbc5440081d3cf21683af2de2942fccbe9b))
* **deps:** update dependencies to latest versions ([c8dc8f1](https://github.com/s00d/tauri-plugin-serialplugin/commit/c8dc8f1e514c8b166c18895067e1fbc824831659))

### [2.10.2](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.1...v2.10.2) (2025-03-27)

### [2.10.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.10.0...v2.10.1) (2025-03-13)


### Bug Fixes

* **serialport:** update USB permission handling for Android 33+ ([b9b0e70](https://github.com/s00d/tauri-plugin-serialplugin/commit/b9b0e70de553b21ada8d1f66b33b7686f9d8ea8e))
* **serialport:** update USB permission handling for Android 33+ ([8034668](https://github.com/s00d/tauri-plugin-serialplugin/commit/80346681df190f98ed90fc8c17325cd111f31c45))

## [2.10.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.9.0...v2.10.0) (2025-03-02)


### Features

* **serialport-test:** refactor app structure and enhance port management ([22b0e7a](https://github.com/s00d/tauri-plugin-serialplugin/commit/22b0e7a93e0fefb91766c56ffe630024804f42a2))


### Bug Fixes

* **serialport:** update USB permission handling for Android 33+ ([e71cca9](https://github.com/s00d/tauri-plugin-serialplugin/commit/e71cca9508af4acea55deee432300263435a74d8))

## [2.9.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.2...v2.9.0) (2025-02-05)


### Features

* **android:** enable hardware acceleration in AndroidManifest.xml ([2f16b06](https://github.com/s00d/tauri-plugin-serialplugin/commit/2f16b066e4e8c53d5c1ba7b225d2c0b6eeb69fe0))
* **serialport:** register USB receiver in SerialPortManager ([5f32352](https://github.com/s00d/tauri-plugin-serialplugin/commit/5f323529b2481fcf0fe54f5264fcd0f0b61c95a9))


### Bug Fixes

* **device_filter:** remove outdated USB device entries ([3236183](https://github.com/s00d/tauri-plugin-serialplugin/commit/3236183c449758d30fabf339bba203721bbefc5b))

### [2.8.2](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.1...v2.8.2) (2025-02-02)


### Bug Fixes

* **serialport:** improve timeout handling and error messages ([de2233c](https://github.com/s00d/tauri-plugin-serialplugin/commit/de2233c0d2503b37e75401656e16842a54ca5a08))

### [2.8.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.8.0...v2.8.1) (2025-02-02)


### Bug Fixes

* **permissions:** add read-binary command permissions ([76262b1](https://github.com/s00d/tauri-plugin-serialplugin/commit/76262b10f3f75878a8d5f6226a60a27b06bdcdb8))

## [2.8.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.7.0...v2.8.0) (2025-01-30)


### Features

* **serial:** add `read_binary` command to read binary data from serial port ([1a17f99](https://github.com/s00d/tauri-plugin-serialplugin/commit/1a17f99dff08c430ba29bda0c0df8a883746e65e))
* **serial:** add `read_binary` command to read binary data from serial port ([83a873d](https://github.com/s00d/tauri-plugin-serialplugin/commit/83a873d6a02a3c146698bd651fa67dcb7dd75acd))
* **serial:** add `read_binary` command to read binary data from serial port ([09e6a32](https://github.com/s00d/tauri-plugin-serialplugin/commit/09e6a329833281759fbe993602b7d23fda2c2f86))
* **serial:** add readBinary method for reading binary data ([5a62544](https://github.com/s00d/tauri-plugin-serialplugin/commit/5a62544e554ac3098e9ca99f3211ba0a5097aa70))

## [2.7.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.6.1...v2.7.0) (2025-01-27)


### Features

* **mobile_api:** update managed_ports to return a list of port names ([77e6799](https://github.com/s00d/tauri-plugin-serialplugin/commit/77e67998b294ee2599c8a765798b67815f4bbd84))

### [2.6.1](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.6.0...v2.6.1) (2025-01-27)

## [2.6.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.13...v2.6.0) (2025-01-14)


### Features

* **app:** add managed ports functionality and UI integration ([a3465ce](https://github.com/s00d/tauri-plugin-serialplugin/commit/a3465ced1ca053d31768600ef40ab4b853c68728))
* **build:** add managed_ports command to the serial plugin ([6a79bbb](https://github.com/s00d/tauri-plugin-serialplugin/commit/6a79bbb20d1eda64600b2609f21838cb8a1fbc14))
* **permissions:** add managed_ports command permissions ([634b713](https://github.com/s00d/tauri-plugin-serialplugin/commit/634b713d551295bad502746a2c80157eb0ce4d63))
* **permissions:** add managed_ports permissions and update documentation ([9714453](https://github.com/s00d/tauri-plugin-serialplugin/commit/97144537f153759d9ba9f2bb6efe370eaa6b726b))
* **README:** add documentation for managed ports feature ([b241c13](https://github.com/s00d/tauri-plugin-serialplugin/commit/b241c1317c9a591c54d9020eb955b06cc990d8ac))
* **schemas:** add commands for webview and window background color ([67b9670](https://github.com/s00d/tauri-plugin-serialplugin/commit/67b9670b9df0fe6036363aec6141438625a7ecc3))
* **serial:** add managed_ports command to list open serial ports ([7eb727d](https://github.com/s00d/tauri-plugin-serialplugin/commit/7eb727dbfa19234f8c4a4749f518ac38463c61ac))
* **serialplugin:** add managedPorts command to retrieve active ports ([378ee2a](https://github.com/s00d/tauri-plugin-serialplugin/commit/378ee2a9534f9dead7ca6f7b4ad59126996282af))
* **serialplugin:** add method to list all managed serial ports ([d04b991](https://github.com/s00d/tauri-plugin-serialplugin/commit/d04b9913914c6a9af889c895a28a532a79d160de))

### [2.4.13](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.11...v2.4.13) (2024-12-24)


### Bug Fixes

* **package:** update version numbers in package files to 2.4.12 ([323a1f9](https://github.com/s00d/tauri-plugin-serialplugin/commit/323a1f98c4ce4ec035d3594210250622ba869823))

### [2.4.11](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.10...v2.4.11) (2024-11-29)

### [2.4.10](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.9...v2.4.10) (2024-11-29)

### [2.4.9](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.7...v2.4.9) (2024-11-29)

### [2.4.7](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.6...v2.4.7) (2024-11-29)

### [2.4.6](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.5...v2.4.6) (2024-11-29)

### [2.4.4](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.5...v2.4.4) (2024-11-29)

### [2.4.5](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.4.4...v2.4.5) (2024-11-29)

### [2.4.4](https://github.com/s00d/tauri-plugin-serialplugin/compare/v2.1.0...v2.4.4) (2024-11-29)


### Bug Fixes

* size and timeout settings not working. ([e49197d](https://github.com/s00d/tauri-plugin-serialplugin/commit/e49197d876f88e1a5b5f6f6e15dfd7d2a90e3617))

## [2.3.1] - 2024-11-10

### Added
- Automatic cleanup of existing listeners before starting new ones

## [2.3.0] - 2024-11-10

### Added
- New `startListening` command for explicit port monitoring control
- New `stopListening` command for manual monitoring termination

## [2.2.0] - 2024-11-10

### Added
- Automatic port listening on connection
- Background thread management for port monitoring

### Changed
- Refactored read operation to be synchronous instead of event-based
- Improved port cleanup on close
- Modified TypeScript interface to return string data directly from read operation
- Changed port reading logic to use direct synchronous reads
- Added automatic port monitoring on connection

## [2.1.0] - 2024-11-01

### Added
- New serial port control methods:
    - `set_baud_rate`: Set the baud rate
    - `set_data_bits`: Set the data bits configuration
    - `set_flow_control`: Set the flow control mode
    - `set_parity`: Set the parity checking mode
    - `set_stop_bits`: Set the stop bits configuration
    - `set_timeout`: Set the timeout duration
    - `write_request_to_send`: Set RTS control signal
    - `write_data_terminal_ready`: Set DTR control signal
    - `read_clear_to_send`: Read CTS signal state
    - `read_data_set_ready`: Read DSR signal state
    - `read_ring_indicator`: Read RI signal state
    - `read_carrier_detect`: Read CD signal state
    - `bytes_to_read`: Get available bytes to read
    - `bytes_to_write`: Get bytes waiting to be written
    - `clear_buffer`: Clear input/output buffers
    - `set_break`: Start break signal
    - `clear_break`: Stop break signal
- New permissions for all added methods
- Enhanced error handling for serial port operations

### Changed
- Improved error handling system
- Enhanced documentation for all methods
- Updated TypeScript definitions with JSDoc comments

### Fixed
- Error conversion between serialport and internal errors
- Type conversion issues in serial port operations

## [2.0.2] - 2023-12-20

### Added
- Support for direct port scanning on Windows, Linux, and macOS

### Changed
- Updated dependencies to latest versions
- Improved error messages

### Fixed
- Port detection issues on various platforms

## [2.0.1] - 2023-12-10

### Changed
- Updated dependencies
- Improved available_ports_direct logic
- Updated test UI

## [2.0.0-rc.3] - 2023-12-01

### Added
- Initial implementation of available_ports_direct
