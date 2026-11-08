# Changelog

## [4.2.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v4.1.0...v4.2.0) (2026-11-08)

No breaking API changes: Rust and JS code written for 4.x keeps compiling and the
invoke payloads are unchanged. New things are new `serialplugin-core` (0.2.0) API.

### Behaviour notes (visible to 4.x code)

* `close()` now delivers what the hub still holds (the pending watch batch, a parser's remainder) to the watch before it returns, in order, with no `disconnect`; 4.x dropped the unflushed batch.
* Port-list events: within one poll all `removed` come before all `added` (4.x: `added` first), each sorted by path. A different device on the same path between two polls (type, vid, pid or serial number changed) is `removed` + `added`; late-filled manufacturer / product strings are not.
* CMUX: SABM is resent per the T1 / N2 given in the `AT+CMUX` command (the default `enableMux` command gives T1 = 10, N2 = 2); with neither, one SABM as in 4.x. During modem flow control (FCoff, MSC FC) channel writes wait, and fail after their write timeout.
* `ble://<name>` (desktop / iOS) waits 1.5 s after the first match before taking a name, and fails if two devices share it.
* After a disconnect the core releases the device handles (no hub restart on a dead fd), but the path keeps its 4.x behaviour until `close()`: `managedPorts()` lists it, a new `watch` gets the `disconnect`, other calls fail with the disconnect reason, `close()` succeeds once.
* Rust: `port::list_monitor::diff_ports` reports a path that now holds another device in both lists and returns them sorted.

### Features (serialplugin-core 0.2.0)

* `CoreError` with `ErrorKind` (`NotFound`, `PermissionDenied`, `Busy`, `Disconnected`, `Timeout`, `Cancelled`, `Other`; platform error as `source()`, converts to the 4.x `Error` with the same text) from new methods `open_core`, `close_core`, `write_core`, `exchange_core`, `at_core`. A port that is listed but cannot be opened (flock, Windows "Access is denied" in any language) is `Busy`. The 4.x methods and errors are unchanged.
* `watch_core` / `unwatch_core`: any number of `CoreEvent` streams per port, each with its own parser and `routeUrc`; URCs caught during `at()` go to every `routeUrc` stream. `subscribe_core` / `unsubscribe_core`: port opened / disconnected (kind `Disconnected` and the reason) / closed, CMUX channels included. Each core receiver runs on its own delivery thread behind a bounded queue (`set_core_queue_capacity`, default 4 MiB): a slow one never stalls RX, `at()` or `close()`; on overflow it gets `CoreEvent::Overflow` and is unsubscribed; a sink returning `false` is removed. `Disconnect` reaches each stream once (also on Android).
* `insert_port` (manage any `SerialPort`, e.g. a fake), `supports_modem_lines(path)` (`false` for `tcp://`, `ble://` and CMUX channels), `available_ports_detailed()` (typed `PortDetails`: u16 vid/pid, Linux `/dev/serial` links; USB interface number and location with the new feature `usb-port-details`, off by default). Features `log` (logs through the `log` crate) and `test-util`.
* The old `watch` keeps the 4.x timing: its sink runs on the RX side, nothing is lost and nothing arrives after `unwatch()` / `close()` returns. The watch registry belongs to each `SerialManager` (two managers may watch the same path; a watch id stays unique across managers); "one watch per port" applies to the old `watch` only, with the 4.x error text.
* Unix local serial ports: writes and control calls use a cloned handle (no wait for the RX hub's read); `open(timeout)` / `set_timeout` apply to writes, and a write longer on the wire gets twice its wire time.
* CMUX (27.010 basic option): MSC (RTC/RTR/DV) after each data DLC is established; known control commands echoed, unknown ones answered with NSC; DISC or DM from the modem on an open DLC closes that channel (`Disconnect` on its path, writes fail, re-open with `openMuxChannel`); DISC / DM on DLCI 0 or CLD from the modem ends the session and the port is plain again; FCoff and MSC FC hold writes as above; `disableMux` / closing the physical port waits up to T2 for the CLD response without holding the manager's lock; CMUX writes get a timeout from the wire time. Core: `CmuxSession::for_command`, `CmuxTimers`, `CmuxPhysicalIo::write_timeout`.
* TCP: keepalive (10 s idle, 5 s interval, 3 probes; Windows keeps its 10) on `tcp://` sockets, best effort, so a bridge that vanished without a FIN ends in `Disconnect`; on Unix a keepalive `ETIMEDOUT` read is a disconnect. Optional allowlist for webview `open()`: `plugins."serialplugin-config".tcpAllowlist` in `tauri.conf.json` (exact `host:port` strings); without the key nothing changes, Rust callers are not restricted.
* BLE (desktop / iOS): each write is bounded by 5 s (a timed-out write drops the link); a name shared by several devices is an error listing their ids.
* The port-list monitor sends events outside its lock and none after `unwatch_ports` / `unsubscribe` returns.

### Fixes

* The first `ExchangeWaiter` result wins (a late cancel no longer replaces a completed reply); `clear_buffer(Input)` also drops bytes buffered in the hub; `bytes_to_read` no longer reports 0 while the hub holds the port; `write` / `write_binary` on `path#dlci=N` go out as CMUX UIH frames.
* CMUX: `enableMux` waits 100 ms after the `AT+CMUX` OK before the first SABM (a frame inside the modem's switch window was lost and the Quectel M26 then ignored SABM for ~2 s, so `open` → `enableMux` failed with "no UA") and sends CLD if the control channel does not open; closing a channel waits up to T1 for the UA to its DISC, so `disableMux` gets the CLD response at once instead of waiting out T2.
* Windows: the full USB serial number is found among every device instance with that COM name.
* android-usb-serial 0.1.4: no panic when a driver is used before `open` or a control-IN reply is short (CP21xx modem status, CH34x status / break, PL2303 status, FTDI modem status).

## [4.1.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v4.0.0...v4.1.0) (2026-11-08)

No breaking API changes: Rust and JS code written for 4.0.0 keeps compiling and the
invoke payloads are unchanged; the API additions and the behaviour changes are below.

### Behaviour changes (AT parser)

* URCs are no longer de-duplicated: two identical URCs in one read are both reported.
* V.250 numeric result codes (`ATV0` mode: `0`, `3`, `4`, …) are recognised as final results;
  result codes match case-insensitively.
* Verbose `+CME ERROR:` / `+CMS ERROR:` texts are mapped to their numeric codes
  (`"+CME ERROR: SIM busy"` gives `code: 14`); `code` is `null` only for text the parser
  does not know.
* Trailing lines after the final result code no longer hide it and are reported as URCs; a final
  `OK` glued to a following `+CUSD:` is split; quoted multi-line `+CUSD` is one URC.
* Two-line `+CMT` / `+CDS` / `+CBM` URCs keep their PDU line; `+CMGS:` in the PDU phase
  of `send_sms_pdu` is treated as solicited; text-mode SMS bodies stay data.
* `ATV0` replies whose result code ends with CR only (V.250 §5.7.1) complete instead of
  timing out; echo and result are no longer glued into one line.
* Basic commands in front of an extended one (`ATE1+CSQ`, `AT&F+CSQ`) no longer turn the
  reply line into a URC; with `ATE0` the lines of a known command (`AT+CLAC`) are body.
* `CONNECT` or `OK` followed by data in the same read (PPP, CMUX) completes the exchange.
* A reply without a final result keeps its last line in `solicitedBody`.
* The non-standard `COMMAND NOT SUPPORT` line is no longer a final result (only V.250,
  27.007 and 27.005 codes are; `SEND OK` / `SEND FAIL` stay for compatibility).

### Features

* New `serialplugin-core` crate (`crates/serialplugin-core`): the plugin's whole serial core
  (RX hub, watch with parsers, exchange / AT / SMS PDU, CMUX, TCP, BLE, Android USB)
  without Tauri. `SerialManager` takes closure sinks (`EventSink`, `PortListSink`)
  instead of IPC channels, so headless Rust apps can drive many ports directly. The
  plugin is now a thin layer over it: `api::SerialPort<R>` keeps its 4.0.0 methods
  (delegating to `SerialManager`, which it also derefs to) and the moved modules are
  re-exported at their old paths (`state`, `events`, `error`, `at`, `hub`, …).
* New `serial-codec` crate (`crates/serial-codec`, MPL-2.0), usable without Tauri:
  `framing` (Node serialport parser parity: delimiter, readline, byteLength,
  interByteTimeout, packetLength, ready, regex, startEnd, SLIP, ccTalk), `at` (AT
  response parsing), `args` (V.250 / 27.007 argument splitting), `urc` (typed 27.007 /
  27.005 information lines and URCs, e.g. `+CSQ`, `+CREG`), `sms` (SMS PDU decode/encode,
  GSM 7-bit with national shift tables, UCS-2, concatenated SMS reassembly), `ussd`
  (`+CUSD` parsing, USSD 7-bit packing, CBS DCS) and `cmux` (27.010 basic-mode frames).
* `watch({ parser })` (Rust `SerialPort::watch_framed` / `SerialManager::watch_framed`;
  the parser is a top-level `parser` argument of the `watch` command): native framing, each complete
  frame is one `data` event with no batching; time-based parsers fire without further
  bytes; the buffered remainder is emitted on unwatch, close or disconnect. Works on
  every transport and on CMUX channels, and survives auto-reconnect. See the
  [parsers guide](https://s00d.github.io/tauri-plugin-serialplugin/guides/parsers).
* JS: new exported types `BlePortInfo` (`scanBlePorts()` results with `rssi`) and
  `ExchangeCompletionMode`.
* Documentation site: https://s00d.github.io/tauri-plugin-serialplugin/ (guides,
  platform notes, and an API reference generated from the sources).
* **Android BLE:** `ble://<MAC or name>` ports and `scanBlePorts()` now work on Android
  (feature `ble`) through a Kotlin `BluetoothGatt` bridge: Nordic UART Service, MTU 247,
  serialized chunked writes, notifications as read data, link loss as `onDisconnect`,
  one retry on GATT 133/62/129. Runtime permission is requested on first use
  (Nearby devices on Android 12+, location on 11 and older). GATT callbacks complete only
  the step that waits for them (a peripheral-initiated MTU exchange or a late callback no
  longer finishes the wrong step); a MAC not seen before is found by a short address scan
  (random static addresses); names are cached and Android's 5-scans-per-30 s limit is a
  clear error; Bluetooth switched off ends open ports with `onDisconnect`; Location
  services off (Android 11 and older) is a clear error; pairing-protected devices are
  paired and retried once; one 20 s budget covers the whole `open()`; partial writes
  return the bytes sent; the RX queue is capped at 1 MiB and counted by `bytesToRead()`.
  The Bluetooth permissions are **not** in the library manifest (its merge would have
  changed apps' own location / `BLUETOOTH_SCAN` declarations): with `ble`, `build.rs`
  writes them into the app's generated `AndroidManifest.xml`, leaving out any the app
  declares itself. Apps without `ble` get no new permissions (see
  [BLE](https://s00d.github.io/tauri-plugin-serialplugin/platforms/ble#android)).

### Bug Fixes

* **watch/URC:** URCs found while draining RX before an exchange were judged per read
  chunk, so a `+CMT:` / `+CDS:` header could go out without its PDU line; they are now
  emitted when the drain completes, over the whole drained buffer. A URC that arrives
  during the pre-exchange drain is therefore delayed by up to the drain window
  (`drainIdleMs` after the last byte, at most `drainMaxMs`, default 200 ms), and still
  reaches the watch when the drain ends with an error (read error, close, USB error).
* **watch:** `unwatch` / `close` racing the RX thread could drop frames the parser had
  already decoded and deliver the later remainder; events now keep their sink from the
  moment they are decoded, and the remainder always comes after them (also on CMUX
  channels, where it could arrive first).
* **core:** the watch sink is never called under a manager, hub, drain or CMUX lock, so
  a sink may call back into `SerialManager` (`unwatch`, the parser remainder on
  unwatch/close/re-watch and drain URCs used to run under them and could deadlock). The
  sink runs on the RX thread, except the last events of a watch (remainder on
  unwatch / close), which may run on the caller's thread; one call at a time, in order.
* **CMUX:** the receive limit is the N1 from the `AT+CMUX=` command (4th parameter),
  at least 1509 bytes; frames up to N1 bytes are no longer dropped as corrupt when N1 is
  above 1509 (the default command keeps N1 = 31).
* **CMUX:** a physical disconnect now flushes channel watches too: their parser
  remainder, then `disconnect` with the channel's path.
* **watch:** time-based parsers (`interByteTimeout`, ccTalk) and the batch timer kept
  running only outside a pre-exchange drain; they now fire during it.
* **core:** `watch` ids handed out by `SerialManager::watch` / `watch_ports` skip ids
  already taken by plugin channels, and a duplicate id is rejected instead of silently
  replacing the other watch. `force_close` joins the RX thread outside the port map lock.
* **watch:** unknown keys inside `watch({ parser })` fail `watch` (a typo would fall back
  to a default); the parsers are linear on input without delimiters (startEnd and regex
  rescanned their whole buffer per read), and `packetLength` gained `maxBufferSize` and
  rejects options that could overflow. Exchanges judge each reply line once (a long
  reply re-ran completion over the whole buffer per read).
* **BLE:** chunks are capped at 512 bytes (Android 14+ and some desktops negotiate
  MTU 517+); `ble://<name>` lookups scan without the service filter (boards that do not
  advertise the NUS UUID); every 8th chunk is written with response when the device
  allows it. Desktop / iOS: `open()` times out after 20 s instead of waiting forever on
  macOS, one Bluetooth adapter is shared and waited for until powered on, notifications
  sent right after subscribing are kept, Linux ports are `ble://<MAC>` and MACs match in
  any case, the RX queue is capped at 1 MiB like on Android. Android: bonded devices get
  1.6 s before service discovery, a stale GATT cache is refreshed once, Service Changed
  ends the port, high connection priority, indicate-only TX characteristics work.

## [4.0.0](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.9...v4.0.0) (2026-11-08)

Full audit release. See the [migration guide](https://s00d.github.io/tauri-plugin-serialplugin/migration) for the breaking changes.

### ⚠ Breaking changes

* Rust `commands::*` port I/O functions are `async` (they ran on the main thread and
  a `read` waiting for data froze the UI). Add `.await`. MSRV 1.79 → 1.90.
* npm `exports` split into `import`/`require` with separate types (`index.d.cts`);
  `guest-js/` sources and the `development` condition are no longer published.
* `read()` decodes bytes in JS (port `encoding`, streaming); `watch()` no longer
  uses the read timeout as its batch window (default ~20 ms, plus flush at `size`).
* Android `device_filter.xml` no longer matches every USB device.

### Features

* Serial over TCP: `tcp://host:port` on every platform (ser2net, ESP-Link, device servers).
* The plugin builds for iOS (it did not compile there at all); TCP is the transport.
* `dtr` / `rts` options on open (`new SerialPort({ dtr: false, … })`, Rust
  `commands::open(…, dtr, rts)` / `SerialPort::open_with_lines`): `dtr: false` stops
  Arduino-style boards resetting on every open. Android still asserts both by default.
* Serial over Bluetooth LE (Nordic UART Service): `ble://<id>` paths and
  `SerialPort.scanBlePorts()`, behind the `ble` cargo feature (desktop + iOS).

### Bug Fixes

* **exchange/AT:** `cancel_exchange` wedged the port's transaction queue for good (the
  next exchange waited forever).
* **exchange/AT:** a late reply left in the buffer completed the next default exchange,
  shifting all later responses by one.
* **exchange/AT:** SMS prompt `"\r\n> "` (no LF) was never detected, so `send_sms_pdu`
  always timed out; `+CMS ERROR`/`ERROR` after `AT+CMGS` waited for the timeout;
  a non-UTF-8 byte or a URC right after `OK` stalled completion; `"AT\r"` lost its CR.
* **exchange/AT:** a cancel between `at_phases` / `send_sms_pdu` phases was wiped and
  the SMS was sent anyway; URCs after the echo were swallowed; the URC demux of the
  previous exchange was reused.
* **CMUX:** frames used HDLC-style 0x7E stuffing with a CRC-16 instead of 27.010 basic
  mode, so no modem understood them; no SABM/UA or CLD; stream flush deadlocked the RX
  thread; DLCI was not range-checked; `enable_mux` ignored `ERROR`; virtual channels
  survived their physical port.
* **watch:** with `routeUrc` data lost its line terminators and lines split across reads
  were not joined; on Windows an unplugged adapter never produced `disconnect`; EINTR
  silently stopped the RX thread.
* **ports:** `unwatch_ports` could deadlock the app; re-opening a port (webview reload)
  left the old watch registered; a failed open could leave an `Opening` entry or a
  running hub thread; ports are closed on app exit; large or slow writes failed after a
  fixed 100 ms timeout.
* **guest-js:** `handle.unwatch()` did not free the port for a new `watch()`; `close()`
  after a disconnect did not stop auto-reconnect; `set*` changes were lost on reconnect;
  `open()` failing in `configureAtSession` left the port open; auto-reconnect gave up
  when the re-watch failed; concurrent `watch()` started two native watches;
  `sendAtPhases` sent binary phases as objects; `onReconnect` always reported attempt 0.
* **android:** PL2303 and FTDI chip detection read the wrong descriptor; FTDI baud 0
  panicked; `device_filter.xml` had unparsable ids (wildcards) and wrong STM32/Arduino
  ids; a 30 s USB permission wait held the device lock the detach handler needed;
  the fd leaked when an open failed part way; detach teardown ran on the UI thread.
* **android:** USB Ethernet / RNDIS devices (a CDC data interface without an ACM
  control interface) were listed as serial ports; an out-of-range CDC port index
  silently opened another port; a CDC notification error was reported only once;
  `clear(Input)` left chunks the reader had already queued; a write that timed out
  half-way (flow control) reported nothing sent, so `write_all` resent it; a USB
  chunk bigger than the read buffer lost its tail. FTDI latency timer is set to
  1 ms (chip default 16 ms per short reply). CH34x gains RTS/CTS flow control and
  the 307200 baud divisor; PL2303 no longer sends rates older chips reject;
  FT4232H counts 4 ports. (Compared against usb-serial-for-android 3.11 / master.)
* **windows:** full USB serial numbers come from the registry instead of `wmic`,
  which is gone from Windows 11 24H2+ and flashed a console window on every
  port-list poll.
* **docs:** six permission identifiers in the README did not exist.

### Build

* guest-js: Vite 8 library build (ESM + CJS), TypeScript 6, vitest 5; publint,
  arethetypeswrong and a CJS/ESM smoke check run on every build. Rollup, Jest and tslib removed.
* Android library: compileSdk 36, AGP 8.11, Kotlin 2.2 `compilerOptions`; unused
  appcompat/material/gson dependencies removed.
* CI on Linux, macOS and Windows (Windows test binaries embed a Common-Controls v6
  manifest), MSRV job, Android and iOS checks.
* android-usb-serial 0.1.3.


## [3.0.9](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.8...v3.0.9) (2026-10-14)

## [3.0.8](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.7...v3.0.8) (2026-10-14)

### Bug Fixes

* **android:** lint at build time and pick a Gradle-compatible JDK ([0cae84a](https://github.com/s00d/tauri-plugin-serialplugin/commit/0cae84a55aba35d61b3f67858aaac20eb8f7b8a1))
* **android:** prevent app crashes when USB devices disconnect ([#43](https://github.com/s00d/tauri-plugin-serialplugin/issues/43)) ([70371fc](https://github.com/s00d/tauri-plugin-serialplugin/commit/70371fc65b4a26a777e992d41dc67d925ed8fea2))
* **ci:** stop android-integration from installing removed SDK tools ([e032bbf](https://github.com/s00d/tauri-plugin-serialplugin/commit/e032bbf8d5ecbd24c9099c47a150f96e70df8e5e))

## Unreleased

- Clear pending Java exceptions on failed USB JNI operations before detaching
  native threads, preventing an Android crash when reconnecting an unplugged dongle.


## [3.0.7](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.6...v3.0.7) (2026-09-11)

### Bug Fixes

* **android:** cache UsbNative class in nativeInit ([#42](https://github.com/s00d/tauri-plugin-serialplugin/issues/42)) ([46797ec](https://github.com/s00d/tauri-plugin-serialplugin/commit/46797ec31b78535fbab5d510d52b90ac00d09b0b))
* **android:** clear aarch64 --all-targets clippy/test noise ([532b334](https://github.com/s00d/tauri-plugin-serialplugin/commit/532b334df3cb6a202004a7550398fca3f3363587))
* **android:** surface UsbNative class-cache init failures ([b331bc9](https://github.com/s00d/tauri-plugin-serialplugin/commit/b331bc9b7209b1b677884917d3027a605c3fc11d)), references [#42](https://github.com/s00d/tauri-plugin-serialplugin/issues/42)
* **android:** wire FakeTransport RX hub and AT modem sim ([7f19bf8](https://github.com/s00d/tauri-plugin-serialplugin/commit/7f19bf8c6c6ce291180dd8014be72b5557fd5414))

## [3.0.6](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.5...v3.0.6) (2026-09-10)

### Bug Fixes

* **ci:** align playground Tauri 2.6; skip Win node_modules cache ([4b09e59](https://github.com/s00d/tauri-plugin-serialplugin/commit/4b09e591483c009bd9883813eab6b6fa3ac115a1))
* **ci:** restore serialport-test Cargo.lock for Android ([2c4d1f9](https://github.com/s00d/tauri-plugin-serialplugin/commit/2c4d1f942ecc9dc0e87b2a6e09afa94535a2606f))
* **release:** publish via publish-branch=main ([56d1c8e](https://github.com/s00d/tauri-plugin-serialplugin/commit/56d1c8ebabdcb73169d91bd56bc39b3ee8c04538))
* **release:** stage Cargo.lock after version bump ([04e2362](https://github.com/s00d/tauri-plugin-serialplugin/commit/04e23621168af78ee0d9c4adc7d9e279ee6ec7f5))
* **watch:** keep idle RX bytes raw by default ([e5df565](https://github.com/s00d/tauri-plugin-serialplugin/commit/e5df565b297ce587d5b8fc6319260d15546b6e48)), closes [#41](https://github.com/s00d/tauri-plugin-serialplugin/issues/41)

## [3.0.5](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.0...v3.0.5) (2026-09-06)

### Bug Fixes

* ABBA deadlock between RxHubShared read/drain slot and its done mutex ([#38](https://github.com/s00d/tauri-plugin-serialplugin/issues/38)) ([e252491](https://github.com/s00d/tauri-plugin-serialplugin/commit/e2524915ed7c628ea87443c210aaeba316bf0bc1)), closes [#2](https://github.com/s00d/tauri-plugin-serialplugin/issues/2)
* **cdc-acm:** expose serial state notifications ([#37](https://github.com/s00d/tauri-plugin-serialplugin/issues/37)) ([0f5a944](https://github.com/s00d/tauri-plugin-serialplugin/commit/0f5a94483d781849c9170498233636a9f21d761c))
* **cdc-acm:** skip bad serial-state frames ([4a47cd6](https://github.com/s00d/tauri-plugin-serialplugin/commit/4a47cd607c48fd98b29b6c3a79c14c1eaf171fb5))
* **clippy:** drop redundant borrow in format! arg ([f15973f](https://github.com/s00d/tauri-plugin-serialplugin/commit/f15973f1c123f9288492da04a8e745dfd9174ccd))
* **hub:** take whole read/drain slot on complete ([#39](https://github.com/s00d/tauri-plugin-serialplugin/issues/39)) ([8851d6b](https://github.com/s00d/tauri-plugin-serialplugin/commit/8851d6b5edf83cf58878858f13852f84e7b0450a))
* **npm:** ship guest-js in published package ([0f1e3ff](https://github.com/s00d/tauri-plugin-serialplugin/commit/0f1e3ff254f672959b0e68b2a8c46401460f1aef)), closes [#36](https://github.com/s00d/tauri-plugin-serialplugin/issues/36)

All notable changes to this project will be documented in this file. See [standard-version](https://github.com/conventional-changelog/standard-version) for commit guidelines.


## [3.0.4](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.3...v3.0.4) (2026-09-05)

### Bug Fixes

* **hub:** take the whole read/drain slot on complete (no emptied-`Some` race with timeout reclaim); drain timeout reclaim mirrors read and preserves posted errors; do not drop RX when the drain slot is reclaimed mid-read ([#39](https://github.com/s00d/tauri-plugin-serialplugin/issues/39))

## [3.0.3](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.2...v3.0.3) (2026-09-05)

### Bug Fixes

* **hub:** fix ABBA deadlock between `RxHubShared` read/drain slot and its `done` mutex; recover a completion posted during the timeout reclaim race instead of reporting a spurious timeout ([#38](https://github.com/s00d/tauri-plugin-serialplugin/issues/38))

## [3.0.2](https://github.com/s00d/tauri-plugin-serialplugin/compare/v3.0.1...v3.0.2) (2026-07-31)

### Bug Fixes

* **android / cdc-acm:** expose serial-state notifications (DCD/DSR/RI) and skip malformed interrupt frames so the notification reader stays alive ([#37](https://github.com/s00d/tauri-plugin-serialplugin/issues/37))
* **deps:** bump `android-usb-serial` to `0.1.1`

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
