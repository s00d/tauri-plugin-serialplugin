#!/usr/bin/env bash
# Publish JS package first (pnpm), then crates.io.
# Usage: ./scripts/publish.sh [--dry-run]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if [[ "$#" -gt 1 || ( "$#" -eq 1 && "${1:-}" != "--dry-run" ) ]]; then
  echo "Usage: $0 [--dry-run]" >&2
  exit 2
fi

DRY=0
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY=1
fi

CARGO_VER="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
NPM_VER="$(node -p "require('./package.json').version")"
if [[ "$CARGO_VER" != "$NPM_VER" ]]; then
  echo "error: version mismatch Cargo.toml=$CARGO_VER package.json=$NPM_VER" >&2
  exit 1
fi
echo "==> version $CARGO_VER"

echo "==> pnpm registry auth"
if ! pnpm whoami >/dev/null 2>&1; then
  echo "error: not logged in to the npm registry via pnpm." >&2
  echo "  pnpm login" >&2
  echo "Then re-run: pnpm release:publish" >&2
  exit 1
fi
echo "ok: pnpm whoami=$(pnpm whoami)"

echo "==> cargo credentials"
if [[ -n "${CARGO_REGISTRY_TOKEN:-}" ]] \
  || [[ -f "${CARGO_HOME:-$HOME/.cargo}/credentials.toml" ]] \
  || [[ -f "${CARGO_HOME:-$HOME/.cargo}/credentials" ]]; then
  echo "ok: cargo auth available (token env and/or credentials file)"
else
  echo "error: no cargo credentials. Run: cargo login" >&2
  echo "  or export CARGO_REGISTRY_TOKEN=..." >&2
  exit 1
fi

echo "==> js deps + publish surface"
pnpm install --frozen-lockfile
bash "$ROOT/scripts/check-publish-surface.sh"

if [[ "$DRY" -eq 1 ]]; then
  echo "==> pnpm publish --dry-run"
  pnpm publish --dry-run --no-git-checks
  echo "==> cargo publish --dry-run"
  cargo publish --dry-run --allow-dirty
  echo "DRY-RUN OK ($CARGO_VER)"
  exit 0
fi

# JS registry first: sessions expire often; fail before crates.io.
echo "==> pnpm publish ($NPM_VER)"
pnpm build
pnpm publish --no-git-checks

echo "==> cargo publish ($CARGO_VER)"
echo "note: path dep android-usb-serial must already be on crates.io at the required version"
echo "      if needed: cargo publish -p android-usb-serial"
cargo publish

echo "PUBLISHED $CARGO_VER (pnpm registry + crates.io)"
