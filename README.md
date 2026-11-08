[![npm version](https://img.shields.io/npm/v/tauri-plugin-serialplugin-api/latest?style=for-the-badge)](https://www.npmjs.com/package/tauri-plugin-serialplugin-api)
[![Crates.io](https://img.shields.io/crates/v/tauri-plugin-serialplugin?style=for-the-badge)](https://crates.io/crates/tauri-plugin-serialplugin)
[![Documentation](https://img.shields.io/badge/docs-docs.rs-blue?style=for-the-badge)](https://docs.rs/tauri-plugin-serialplugin/latest/tauri_plugin_serialplugin/all.html)
[![GitHub issues](https://img.shields.io/github/issues/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/issues)
[![GitHub stars](https://img.shields.io/github/stars/s00d/tauri-plugin-serialplugin?style=for-the-badge)](https://github.com/s00d/tauri-plugin-serialplugin/stargazers)
[![Donate](https://img.shields.io/badge/Donate-Donationalerts-ff4081?style=for-the-badge)](https://www.donationalerts.com/r/s00d88)

# Tauri Plugin — SerialPort

Serial ports for Tauri 2 apps: desktop COM/tty ports, USB-serial adapters on
Android, and serial over TCP or Bluetooth LE — including on iOS and Android.

- **One API everywhere:** open, stream with `watch()`, read/write, line settings,
  modem lines, hotplug, auto-reconnect.
- **Modems built in:** a native AT command queue with structured results, URCs,
  SMS (PDU) and GSM 07.10 / 27.010 CMUX.
- **Android without Java libraries:** USB-serial drivers in Rust (FTDI, CP210x,
  PL2303, CH34x, CDC-ACM, …).
- **Network and radio transports:** `tcp://host:port` to a serial bridge,
  `ble://<id>` to Nordic UART devices.

| | Local ports | `tcp://` | `ble://` |
|-|-------------|----------|----------|
| Windows · macOS · Linux | ✅ | ✅ | ✅ (feature `ble`) |
| Android | ✅ USB host | ✅ | ✅ (feature `ble`) |
| iOS | — (no OS API) | ✅ | ✅ (feature `ble`) |

## Install

```bash
pnpm tauri add serialplugin     # or: npm run tauri add serialplugin / cargo tauri add serialplugin
```

or by hand:

```bash
cargo add tauri-plugin-serialplugin --manifest-path src-tauri/Cargo.toml
pnpm add tauri-plugin-serialplugin-api
```

Register the plugin and allow it in a capability:

```rust
// src-tauri/src/lib.rs
tauri::Builder::default()
    .plugin(tauri_plugin_serialplugin::init())
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
```

```jsonc
// src-tauri/capabilities/default.json
{ "identifier": "default", "windows": ["main"], "permissions": ["core:default", "serialplugin:default"] }
```

Requires Rust 1.90+ and Tauri 2. Android needs no extra setup; for BLE see
[platforms](https://s00d.github.io/tauri-plugin-serialplugin/platforms/ble).

## Quick start

```ts
import { SerialPort } from "tauri-plugin-serialplugin-api";

const ports = await SerialPort.available_ports();   // { "/dev/ttyUSB0": { type: "USB", vid, pid, … }, … }

const port = new SerialPort({ path: Object.keys(ports)[0], baudRate: 115200 });
await port.open();

const handle = await port.watch({
  onData: (text) => console.log("rx", text),
  onDisconnect: (reason) => console.log("gone:", reason),
});

await port.write("hello\n");

await handle.unwatch();
await port.close();
```

Binary data: `writeBinary(new Uint8Array([…]))` and
`watch(handlers, { decode: false })` (or `readBinary()`).

Talking to a modem:

```ts
const r = await port.sendAt("AT+CSQ");   // queued, waits for OK / ERROR
console.log(r.status, r.solicitedBody);  // "ok", ["+CSQ: 27,0"]
```

Surviving unplugs:

```ts
port.enableAutoReconnect({ interval: 3000, maxAttempts: null });
// after onDisconnect the port is reopened and the watch restored
```

## Documentation

| | |
|-|-|
| [JavaScript API](https://s00d.github.io/tauri-plugin-serialplugin/guides/javascript-api) | Every method, option, event and error |
| [AT commands, exchange, CMUX](https://s00d.github.io/tauri-plugin-serialplugin/guides/at-commands) | Modem queue, SMS, URCs, multiplexing |
| [Platforms and transports](https://s00d.github.io/tauri-plugin-serialplugin/platforms/overview) | Android USB, iOS, `tcp://`, `ble://`, DTR/RTS on open |
| [Rust](https://s00d.github.io/tauri-plugin-serialplugin/guides/rust) | Using the plugin from your backend · [docs.rs](https://docs.rs/tauri-plugin-serialplugin/) |
| [Permissions](https://s00d.github.io/tauri-plugin-serialplugin/guides/permissions) | Capability identifiers · [full list](https://s00d.github.io/tauri-plugin-serialplugin/reference/permissions) |
| [API reference](https://s00d.github.io/tauri-plugin-serialplugin/reference/serial-port) | Generated from the sources: `SerialPort`, types, Tauri commands |
| [Migration guide](https://s00d.github.io/tauri-plugin-serialplugin/migration) | v3 → v4, v2 → v3 |
| [Changelog](https://s00d.github.io/tauri-plugin-serialplugin/changelog) | |

**Upgrading from v3?** Rust command functions are now `async`, `read()` decodes in
JS, `watch()` batches every ~20 ms; see the
[migration guide](https://s00d.github.io/tauri-plugin-serialplugin/migration).

## Example app

[`examples/serialport-test`](examples/serialport-test) is a terminal with port
picker, settings, AT console and hotplug, for desktop, Android and iOS:

```bash
pnpm install && pnpm build
pnpm playground            # desktop
```

## Development

```bash
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo test -p android-usb-serial --features fake-transport,serialport-compat
pnpm typecheck && pnpm test && pnpm build   # build also runs publint, attw and an ESM/CJS check
cd android && ./gradlew lintDebug test
```

Tests run without hardware: PTY pairs, a local TCP bridge, a fake 27.010 modem,
commands through the Tauri IPC path, and a fake USB transport for the Android
drivers. For manual testing, `socat -d -d pty,raw,echo=0 pty,raw,echo=0` gives a
virtual serial pair. See [CONTRIBUTING.md](./CONTRIBUTING.md) and
[PUBLISHING.md](./PUBLISHING.md).

## Security · Support · License

Report vulnerabilities privately as described in [SECURITY.md](./SECURITY.md).
If the plugin helps you, you can support it via
[DonationAlerts](https://www.donationalerts.com/r/s00d88).

Dual-licensed under MIT or Apache-2.0 ([LICENSE-MIT](./LICENSE-MIT),
[LICENSE-APACHE](./LICENSE-APACHE)).
The [`serial-codec`](crates/serial-codec) dependency (framing, AT/URC/SMS/USSD/CMUX
codecs) is [MPL-2.0](crates/serial-codec/LICENSE): usable in any app, changes to
its own files must be published.
