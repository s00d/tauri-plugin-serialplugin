# Tests

The desktop JNI exception regression requires a JDK (`JAVA_HOME`):
`cargo test --manifest-path tests/jni-bridge/Cargo.toml`. It compiles the actual
JNI bridge without desktop Tauri/WebView dependencies, invokes a throwing Java method and
checks that the USB bridge returns the error with no pending Java exception.

## Trust matrix

| Layer | What it proves | Where | CI job |
|-------|----------------|-------|--------|
| Jest | JS wrapper → `invoke` payload/shape | `tests/*.test.ts` | `test` (Ubuntu/macOS), `windows` |
| Rust integration | Plugin through Tauri IPC (mock runtime): models + stories on PTY / TCP devices | `tests/it/` (root); `crates/*/tests/it/`, see [TESTING.md](../TESTING.md) | `test` (Ubuntu/macOS/Windows, `cargo test --workspace`; PTY stories are `cfg(unix)`, the root binary is desktop only) |
| golden | USB chipset byte parity | `crates/android-usb-serial` | `test` (Ubuntu/macOS) |
| Desktop JNI bridge | Clears pending Java exceptions at the JNI boundary | `tests/jni-bridge` | `test` (Ubuntu only, requires JDK 17) |
| Android Lint + Robolectric | API/static checks + fd/permission unit tests | `android/` (`lintDebug test`) | `test` Ubuntu only — JDK 11–21 via `scripts/java-home-for-gradle.sh` |
| JNI prebuild | NDK + sync androidTest sources | `android-integration-ci.sh prebuild` | `android-integration` |
| JNI connected | FakeTransport Rx/Tx/Detach/Error + AT modem sim | `android-integration-ci.sh test` | **local only** (device/emulator) |
| Desktop hardware | Real AT modem via plugin API (query commands only) | `SERIALPLUGIN_MODEM=/dev/cu.… cargo nextest run -p tauri-plugin-serialplugin story_modem` (prints SKIP and passes when unset) | local only |

Connected/device JNI tests are **not** on every PR — run them locally when touching Android I/O.
The host-side desktop JNI exception regression runs on Ubuntu in the `test` CI job.

**Not pursued for default Mac/AVD workflow:** USB passthrough into Google AVD, Robolectric `ShadowUsb*` for driver I/O, Genymotion on Apple Silicon. For Linux-only `UsbManager` e2e see `android/README.md` (Bliss/QEMU, USB/IP).

Local fast gate: `./scripts/ci-fast.sh`  
Full migration gate: `./scripts/verify-android-usb-migration.sh`

## Jest (guest-js)

```bash
pnpm test
```

Key suites:

- `watch-lifecycle.test.ts` — Channel + `watch` / `unwatch`
- `port-operations.test.ts`, `encoding-operations.test.ts`
- `exchange.test.ts`, `exchange-options.test.ts` — native exchange payloads
- `auto-reconnect.test.ts` — reconnect on `watch` disconnect
- `command-inventory.test.ts` — invoke names match Rust handler set

Mocks: `@tauri-apps/api/core` (`invoke`, `Channel`) in `tests/setup.ts`.

Coverage: Jest fails the suite if guest-js line coverage drops below 85%.

## Rust

```bash
cargo nextest run --workspace --features android-usb-serial/fake-transport,android-usb-serial/serialport-compat
# one crate, one area:
cargo nextest run -p tauri-plugin-serialplugin model_device
```

Kinds, layout and rules: [TESTING.md](../TESTING.md).

android-usb-serial's tests need its fake transport:
`cargo test -p android-usb-serial --features fake-transport,serialport-compat`.

## Android

```bash
# JDK 17 required for Robolectric / Gradle 8.9
export JAVA_HOME="$(/usr/libexec/java_home -v 17 2>/dev/null || echo "$JAVA_HOME")"
cd android && ./gradlew test

# JNI (local device/emulator):
./scripts/android-integration-ci.sh test
```

## Hardware (local only)

- Playground AT quick buttons: `pnpm playground`
- `examples/usb-driver-tester` — real chips
- `story_hardware` — Android device only: `cargo test -p android-usb-serial --features fake-transport story_hardware -- --ignored`
- `story_modem` — `SERIALPLUGIN_MODEM=… cargo nextest run -p tauri-plugin-serialplugin story_modem`
