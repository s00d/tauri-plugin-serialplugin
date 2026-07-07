# Updating vendored `usbserial`

Single source of version truth: [`version.properties`](version.properties).

## Quick commands

```bash
# Is there a new upstream release on GitHub?
./scripts/vendor-usbserial.sh check

# Pull the latest upstream tag and run ./gradlew test
./scripts/vendor-usbserial.sh latest

# Pin a specific version
./scripts/vendor-usbserial.sh v3.11.0

# Re-import the current version from version.properties (no tag change)
./scripts/vendor-usbserial.sh vendor

# Via pnpm
pnpm vendor:usbserial:check
pnpm vendor:usbserial:latest
```

## Maintainer checklist (after `latest` or `vendor <tag>`)

1. **Gradle** — the script syncs `androidx.annotation` from upstream `build.gradle`. Verify `compileSdk` / `minSdk` if upstream raised requirements.
2. **`BuildConfig`** — if `grep -r BuildConfig android/usbserial/src` finds imports, the stub in [`stubs/com/hoho/android/usbserial/BuildConfig.java`](stubs/com/hoho/android/usbserial/BuildConfig.java) must remain (Tauri `sourceSets`).
3. **`consumer-rules.pro`** — when upstream changes, the script warns; update [`../consumer-rules.pro`](../consumer-rules.pro) if needed.
4. **Kotlin wrapper** — review [release notes](https://github.com/mik3y/usb-serial-for-android/releases): `SerialInputOutputManager`, `setReadQueue`, `UsbSerialPort` API → [`UsbBridge.kt`](../src/main/kotlin/app/tauri/serialplugin/manager/UsbBridge.kt) / [`UsbPortSession.kt`](../src/main/kotlin/app/tauri/serialplugin/manager/UsbPortSession.kt).
5. **Builds**
   ```bash
   cd android && ./gradlew test
   cd examples/serialport-test && pnpm tauri android build --debug
   ```
6. **Docs** — `VENDOR.md` and `version.properties` are updated by the script; manually update `CHANGELOG.md`, `android/README.md`, `README.md` (version mentions).
7. **Commit** — separate commit such as `chore(android): vendor usb-serial-for-android vX.Y.Z`.

## What the script does not touch

| Path | Why |
|------|-----|
| `usbserial/build.gradle` | Our `compileSdk`/`minSdk`, no maven-publish |
| `usbserial/stubs/` | Patch for Tauri sourceSets |
| `android/build.gradle` | Conditional `:usbserial` vs sourceSets wiring |

## Rollback

```bash
git checkout HEAD -- android/usbserial/src android/usbserial/LICENSE android/usbserial/consumer-rules.pro android/usbserial/VENDOR.md android/usbserial/version.properties
```

Or `git revert` the vendor commit.
