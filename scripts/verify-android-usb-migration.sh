#!/usr/bin/env bash
# Full Android USB migration verification gate (plan section 12).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt"
cargo fmt --all -- --check

echo "==> cargo clippy (host)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> rustup android target"
rustup target add aarch64-linux-android

echo "==> cargo check android-usb-serial (Android)"
cargo check -p android-usb-serial --target aarch64-linux-android

echo "==> cargo check plugin (Android)"
cargo check -p tauri-plugin-serialplugin --target aarch64-linux-android

echo "==> cargo clippy android-usb-serial (Android)"
cargo clippy -p android-usb-serial --target aarch64-linux-android -- -D warnings

echo "==> android-usb-serial tests (fake-transport)"
cargo test -p android-usb-serial --features fake-transport

echo "==> workspace tests (nextest if available)"
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi

if [[ "$(uname -s)" == "Linux" ]] || [[ -d "/Applications/Android Studio.app" ]]; then
  echo "==> Robolectric (android module)"
  if command -v /usr/libexec/java_home >/dev/null 2>&1; then
    export JAVA_HOME="${JAVA_HOME:-$(/usr/libexec/java_home -v 17 2>/dev/null || true)}"
  fi
  (cd android && ./gradlew test)
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
