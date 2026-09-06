#!/usr/bin/env bash
# Verify npm publish surface: dist-js is present in the packed tarball.
# Optionally runs `cargo public-api` when installed.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> pnpm build"
pnpm build

TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "==> npm pack → $TMP"
npm pack --pack-destination "$TMP" >/dev/null
TGZ="$(ls "$TMP"/*.tgz | head -1)"
test -n "$TGZ"

echo "==> inspect $(basename "$TGZ")"
tar -tzf "$TGZ" | tee "$TMP/files.txt" >/dev/null

if ! grep -qE '^package/dist-js/' "$TMP/files.txt"; then
  echo "error: packed tarball missing package/dist-js/" >&2
  echo "--- contents ---" >&2
  cat "$TMP/files.txt" >&2
  exit 1
fi

echo "ok: dist-js present in pack"

if command -v cargo-public-api >/dev/null 2>&1 || cargo public-api --help >/dev/null 2>&1; then
  echo "==> cargo public-api"
  cargo public-api || true
else
  echo "skip: cargo-public-api not installed (cargo install cargo-public-api)"
fi
