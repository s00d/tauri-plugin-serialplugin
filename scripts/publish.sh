#!/usr/bin/env bash
# Publish crates.io then npm (abort-safe order).
# Usage: ./scripts/publish.sh [--dry-run]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

DRY=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY=1
fi

echo "==> publish surface check"
bash "$ROOT/scripts/check-publish-surface.sh"

CARGO_VER="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
NPM_VER="$(node -p "require('./package.json').version")"
if [[ "$CARGO_VER" != "$NPM_VER" ]]; then
  echo "error: version mismatch Cargo.toml=$CARGO_VER package.json=$NPM_VER" >&2
  exit 1
fi

if [[ "$DRY" -eq 1 ]]; then
  echo "==> cargo publish --dry-run"
  cargo publish --dry-run --allow-dirty
  echo "==> npm publish --dry-run"
  npm publish --dry-run
  echo "DRY-RUN OK ($CARGO_VER)"
  exit 0
fi

echo "==> cargo publish ($CARGO_VER)"
cargo publish

echo "==> npm publish ($NPM_VER)"
# Ensure dist-js matches the just-built check step
pnpm build
npm publish

echo "PUBLISHED $CARGO_VER (crates.io + npm)"
