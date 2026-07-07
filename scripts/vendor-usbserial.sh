#!/usr/bin/env bash
# Vendor / update mik3y/usb-serial-for-android under android/usbserial/.
#
# Usage:
#   vendor-usbserial.sh check              # local version vs GitHub latest release
#   vendor-usbserial.sh latest [--no-verify]
#   vendor-usbserial.sh vendor [TAG] [--no-verify]   # import TAG or version.properties tag
#   vendor-usbserial.sh [TAG] [--no-verify]          # shorthand for vendor TAG
#
# Canonical version: android/usbserial/version.properties
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
DEST="$ROOT/android/usbserial"
VERSION_FILE="$DEST/version.properties"
UPSTREAM_REPO="mik3y/usb-serial-for-android"
VERIFY=1

usage() {
  sed -n '2,12p' "$0" | sed 's/^# \{0,1\}//'
}

read_local_version() {
  if [[ ! -f "$VERSION_FILE" ]]; then
    echo "Missing $VERSION_FILE" >&2
    exit 1
  fi
  # shellcheck disable=SC1090
  source "$VERSION_FILE"
  if [[ -z "${tag:-}" ]]; then
    tag="v${version}"
  fi
}

github_latest_tag() {
  curl -fsSL "https://api.github.com/repos/${UPSTREAM_REPO}/releases/latest" \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['tag_name'])"
}

github_tag_commit() {
  local t="$1"
  curl -fsSL "https://api.github.com/repos/${UPSTREAM_REPO}/git/refs/tags/${t}" \
    | python3 -c "import sys,json; print(json.load(sys.stdin)['object']['sha'])"
}

sync_annotation_dependency() {
  local upstream_build="$1"
  local our_build="$DEST/build.gradle"
  if [[ ! -f "$upstream_build" ]]; then
    echo "WARN: upstream build.gradle not found, skip annotation sync"
    return
  fi
  local dep
  dep="$(grep -Eo 'androidx\.annotation:annotation:[0-9.]+' "$upstream_build" | head -1 || true)"
  if [[ -z "$dep" ]]; then
    echo "WARN: androidx.annotation not found in upstream build.gradle"
    return
  fi
  local artifact="${dep##*:}"
  if grep -q 'androidx.annotation:annotation:' "$our_build"; then
    sed -i '' "s/implementation(\"androidx.annotation:annotation:[0-9.]*\")/implementation(\"androidx.annotation:annotation:${artifact}\")/" "$our_build"
    echo "Synced androidx.annotation -> ${artifact} in usbserial/build.gradle"
  fi
}

post_import_checks() {
  local old_rules="$1"
  if [[ -f "$old_rules" ]] && [[ -f "$DEST/consumer-rules.pro" ]]; then
    if ! diff -q "$old_rules" "$DEST/consumer-rules.pro" >/dev/null 2>&1; then
      echo ""
      echo "WARN: consumer-rules.pro changed upstream — review android/consumer-rules.pro"
      diff -u "$old_rules" "$DEST/consumer-rules.pro" || true
    fi
  fi

  if grep -rq 'com\.hoho\.android\.usbserial\.BuildConfig' "$DEST/src/main/java" 2>/dev/null; then
    if [[ ! -f "$DEST/stubs/com/hoho/android/usbserial/BuildConfig.java" ]]; then
      echo "ERROR: upstream uses BuildConfig but stub is missing at usbserial/stubs/" >&2
      exit 1
    fi
    echo "OK: BuildConfig stub present (required for Tauri sourceSets path)"
  else
    echo "NOTE: upstream no longer references BuildConfig — stub may be removable"
  fi

  echo ""
  echo "Manual follow-up: see android/usbserial/UPDATE.md (SerialPortManager, CHANGELOG, README)"
}

run_gradle_verify() {
  echo ""
  echo "Running android/./gradlew test ..."
  local java_home=""
  if command -v /usr/libexec/java_home >/dev/null 2>&1; then
    java_home="$(/usr/libexec/java_home -v 17 2>/dev/null || true)"
  fi
  (
    cd "$ROOT/android"
    if [[ -n "$java_home" ]]; then export JAVA_HOME="$java_home"; fi
    ./gradlew test --no-daemon -q
  )
  echo "Gradle test: OK"
}

import_tag() {
  local tag="$1"
  local version="${tag#v}"
  local tmp old_rules
  tmp="$(mktemp -d)"
  old_rules="$(mktemp)"

  if [[ -f "$DEST/consumer-rules.pro" ]]; then
    cp "$DEST/consumer-rules.pro" "$old_rules"
  else
    : >"$old_rules"
  fi

  echo "Fetching ${UPSTREAM_REPO} ${tag} ..."
  curl -fsSL "https://github.com/${UPSTREAM_REPO}/archive/refs/tags/${tag}.tar.gz" \
    | tar -xz -C "$tmp"

  local src="$tmp/usb-serial-for-android-${version}/usbSerialForAndroid"
  if [[ ! -d "$src/src/main/java" ]]; then
    rm -rf "$tmp" "$old_rules"
    echo "Expected module not found at $src" >&2
    exit 1
  fi

  rm -rf "$DEST/src"
  mkdir -p "$DEST"
  cp -R "$src/src" "$DEST/"
  rm -rf "$DEST/src/test" "$DEST/src/androidTest"
  cp "$src/consumer-rules.pro" "$DEST/" 2>/dev/null \
    || cp "$src/proguard-rules.pro" "$DEST/consumer-rules.pro"

  curl -fsSL "https://raw.githubusercontent.com/${UPSTREAM_REPO}/${tag}/LICENSE.txt" \
    -o "$DEST/LICENSE"

  sync_annotation_dependency "$src/build.gradle"

  local commit import_date
  commit="$(github_tag_commit "$tag")"
  import_date="$(date +%Y-%m-%d)"

  cat >"$VERSION_FILE" <<EOF
# Canonical vendored usb-serial-for-android version (updated by scripts/vendor-usbserial.sh).
version=${version}
tag=${tag}
EOF

  cat >"$DEST/VENDOR.md" <<EOF
# Vendored: usb-serial-for-android

| Field | Value |
|-------|-------|
| Upstream | https://github.com/${UPSTREAM_REPO} |
| Version | ${version} |
| Tag | \`${tag}\` |
| Commit | \`${commit}\` |
| Imported | ${import_date} |
| Module path (upstream) | \`usbSerialForAndroid/\` |
| License | Apache-2.0 (see [LICENSE](LICENSE)) |

Only \`src/main/java\`, \`src/main/AndroidManifest.xml\`, and \`consumer-rules.pro\` were copied.
Examples, tests, and JitPack publish config were omitted.

Update workflow: [UPDATE.md](UPDATE.md) · script: \`scripts/vendor-usbserial.sh\`
EOF

  rm -rf "$tmp"

  local count
  count="$(find "$DEST/src/main/java" -name '*.java' | wc -l | tr -d ' ')"
  echo "Vendored ${count} Java files -> android/usbserial/ (${tag}, ${commit:0:8})"

  post_import_checks "$old_rules"
  rm -f "$old_rules"

  if [[ "$VERIFY" -eq 1 ]]; then
    run_gradle_verify
  fi
}

normalize_tag() {
  local t="$1"
  [[ "$t" != v* ]] && t="v${t}"
  echo "$t"
}

cmd_check() {
  read_local_version
  local latest local_tag upstream_tag
  latest="$(github_latest_tag)"
  local_tag="$(normalize_tag "$tag")"
  upstream_tag="$(normalize_tag "$latest")"
  echo "Vendored:  ${local_tag} (version ${version})"
  echo "Upstream latest release: ${upstream_tag}"
  if [[ "$local_tag" == "$upstream_tag" ]]; then
    echo "Status: up to date"
  else
    echo "Status: update available — run: ./scripts/vendor-usbserial.sh latest"
  fi
}

parse_args() {
  local cmd="vendor"
  local tag_arg=""

  while [[ $# -gt 0 ]]; do
    case "$1" in
      -h|--help) usage; exit 0 ;;
      --no-verify) VERIFY=0; shift ;;
      check) cmd="check"; shift ;;
      latest) cmd="latest"; shift ;;
      vendor) cmd="vendor"; shift ;;
      v[0-9]*|[0-9]*.[0-9]*)
        if [[ "$cmd" == "vendor" && -z "$tag_arg" ]]; then
          tag_arg="$1"
          [[ "$tag_arg" != v* ]] && tag_arg="v$tag_arg"
        else
          echo "Unexpected argument: $1" >&2
          usage >&2
          exit 1
        fi
        shift
        ;;
      *)
        echo "Unknown argument: $1" >&2
        usage >&2
        exit 1
        ;;
    esac
  done

  case "$cmd" in
    check) cmd_check ;;
    latest)
      local t
      t="$(github_latest_tag)"
      t="$(normalize_tag "$t")"
      import_tag "$t"
      ;;
    vendor)
      if [[ -n "$tag_arg" ]]; then
        import_tag "$tag_arg"
      else
        read_local_version
        import_tag "$tag"
      fi
      ;;
  esac
}

parse_args "$@"
