#!/usr/bin/env bash
# Fast PR-oriented gate (no Android emulator / Robolectric / aarch64 cross).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> cargo fmt"
cargo fmt --all -- --check

echo "==> cargo clippy (host)"
cargo clippy --workspace --all-targets -- -D warnings

echo "==> android-usb-serial golden (fake-transport)"
cargo test -p android-usb-serial --features fake-transport

echo "==> workspace tests"
if command -v cargo-nextest >/dev/null 2>&1; then
  cargo nextest run --workspace
else
  cargo test --workspace
fi

echo "==> JS"
pnpm install --frozen-lockfile
pnpm check && pnpm test

echo "CI-FAST GATES PASSED"
