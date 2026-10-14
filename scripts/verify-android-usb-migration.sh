#!/usr/bin/env bash
# Full Android USB migration verification gate (plan section 12).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt"
cargo fmt --all -- --check
cargo fmt --manifest-path tests/jni-bridge/Cargo.toml -- --check

echo "==> cargo clippy (host)"
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --manifest-path tests/jni-bridge/Cargo.toml --all-targets -- -D warnings

echo "==> rustup android target"
rustup target add aarch64-linux-android

echo "==> cargo check android-usb-serial (Android)"
cargo check -p android-usb-serial --target aarch64-linux-android

echo "==> cargo check plugin (Android)"
cargo check -p tauri-plugin-serialplugin --target aarch64-linux-android

echo "==> cargo clippy android-usb-serial (Android)"
cargo clippy -p android-usb-serial --target aarch64-linux-android -- -D warnings

echo "==> cargo clippy plugin (Android)"
cargo clippy -p tauri-plugin-serialplugin --target aarch64-linux-android --all-targets -- -D warnings

echo "==> android-usb-serial tests (fake-transport)"
cargo test -p android-usb-serial --features fake-transport

echo "==> JNI bridge regression (requires JDK)"
cargo test --manifest-path tests/jni-bridge/Cargo.toml

echo "==> workspace tests (nextest if available)"
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi

if [[ "$(uname -s)" == "Linux" ]] || [[ -d "/Applications/Android Studio.app" ]]; then
  echo "==> Android lint + Robolectric"
  # Gradle 8.9 rejects daemon JVMs newer than 21; pick 17/21/11 even if JAVA_HOME is 27.
  export JAVA_HOME="$("$ROOT/scripts/java-home-for-gradle.sh")"
  echo "    JAVA_HOME=$JAVA_HOME"
  (cd android && ./gradlew lintDebug test)
else
  echo "==> skip android gradle (no JDK/Android on this host)"
fi

echo "==> JS checks"
pnpm check && pnpm build && pnpm test

echo "==> fixture count"
count="$(find crates/android-usb-serial/tests/fixtures -name '*.json' | wc -l | tr -d ' ')"
test "$count" -ge 250

echo "==> legacy grep gate"
if rg -l 'SIOM|feedRx|FakeUsbSerialPort|MobileRxHub|UsbBridge|com\.hoho' \
  --glob '!docs/**' --glob '!*.md' --glob '!NOTICE' --glob '!scripts/**' .; then
  echo "legacy references found in production paths" >&2
  exit 1
fi

test ! -d android/usbserial

echo "ALL GATES PASSED"
