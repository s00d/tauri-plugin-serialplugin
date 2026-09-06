#!/usr/bin/env bash
# Verify npm + crates.io publish surfaces before release.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "==> version sync"
CARGO_VER="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
NPM_VER="$(node -p "require('./package.json').version")"
if [[ "$CARGO_VER" != "$NPM_VER" ]]; then
  echo "error: version mismatch Cargo.toml=$CARGO_VER package.json=$NPM_VER" >&2
  exit 1
fi
echo "ok: version $CARGO_VER"

echo "==> package.json description"
DESC="$(node -p "require('./package.json').description || ''")"
if [[ -z "$DESC" || "$DESC" == *"!["* ]]; then
  echo "error: package.json description missing or looks like README badges" >&2
  exit 1
fi
echo "ok: description=$DESC"

echo "==> pnpm build"
pnpm build

TMP="$(mktemp -d)"
cleanup() { rm -rf "$TMP"; }
trap cleanup EXIT

echo "==> pnpm pack → $TMP"
pnpm pack --pack-destination "$TMP" >/dev/null
TGZ="$(ls "$TMP"/*.tgz | head -1)"
test -n "$TGZ"

echo "==> inspect $(basename "$TGZ")"
tar -tzf "$TGZ" >"$TMP/files.txt"

need_npm=(
  package/dist-js/index.js
  package/dist-js/index.cjs
  package/dist-js/index.d.ts
  package/guest-js/index.ts
  package/CHANGELOG.md
  package/README.md
  package/CONTRIBUTING.md
  package/SECURITY.md
  package/LICENSE-MIT
  package/LICENSE-APACHE
)
for f in "${need_npm[@]}"; do
  if ! grep -qxF "$f" "$TMP/files.txt"; then
    echo "error: packed tarball missing $f" >&2
    exit 1
  fi
done
echo "ok: pnpm pack surface"

echo "==> cargo package --list"
LIST="$TMP/cargo-list.txt"
cargo package --list --allow-dirty >"$LIST"
for bad in \
  '^examples/' \
  '^\.github/' \
  '^docs/' \
  '^banner\.png$' \
  '^tests/' \
  '^scripts/' \
  '^crates/' \
  '^coverage/' \
  '^test-results/' \
  '^android/src/test/' \
  '^android/BUILD_INSTRUCTIONS\.md$' \
  '^jest\.config\.js$' \
  '^\.jestignore$' \
  '^\.versionrc\.json$' \
  '^pnpm-lock\.yaml$' \
  '^pnpm-workspace\.yaml$' \
  '^tsconfig\.json$' \
  '^rollup\.config\.mjs$'
do
  if rg -q "$bad" "$LIST"; then
    echo "error: cargo package still includes: $bad" >&2
    rg "$bad" "$LIST" >&2 || true
    exit 1
  fi
done
for need in '^permissions/default.toml$' '^android/src/main/AndroidManifest.xml$' '^README.md$' '^src/lib.rs$' '^SECURITY.md$' '^CONTRIBUTING.md$'; do
  if ! rg -q "$need" "$LIST"; then
    echo "error: cargo package missing: $need" >&2
    exit 1
  fi
done
echo "ok: cargo package surface ($(wc -l <"$LIST" | tr -d ' ') files)"

if command -v cargo-public-api >/dev/null 2>&1 || cargo public-api --help >/dev/null 2>&1; then
  echo "==> cargo public-api"
  cargo public-api || true
else
  echo "skip: cargo-public-api not installed"
fi

echo "PUBLISH SURFACE OK"
