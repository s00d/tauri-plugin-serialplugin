#!/usr/bin/env bash
# Android JNI integration tests (FakeTransport harness).
#
# Usage:
#   ./scripts/android-integration-ci.sh prebuild   # tauri + ndk + sync (no emulator)
#   ./scripts/android-integration-ci.sh test       # gradlew connectedDebugAndroidTest only
#   ./scripts/android-integration-ci.sh all        # prebuild + test (local)
#
# CI runs prebuild only (no emulator). Local device/emulator tests:
#   ANDROID_INTEGRATION_NDK_ARCH=x86_64 ./scripts/android-integration-ci.sh prebuild
#   ./scripts/android-integration-ci.sh test

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
EXAMPLE="$ROOT/examples/serialport-test"
NDK_ARCH="${ANDROID_INTEGRATION_NDK_ARCH:-arm64-v8a}"
PHASE="${1:-all}"

case "$NDK_ARCH" in
  arm64-v8a)
    RUST_TARGET=aarch64-linux-android
    GRADLE_TASK=:app:connectedArm64DebugAndroidTest
    ;;
  x86_64)
    RUST_TARGET=x86_64-linux-android
    GRADLE_TASK=:app:connectedX86_64DebugAndroidTest
    ;;
  *)
    echo "error: unsupported ANDROID_INTEGRATION_NDK_ARCH=$NDK_ARCH (use arm64-v8a or x86_64)" >&2
    exit 1
    ;;
esac

GRADLE_SKIP_RUST=(
  -x rustBuildArm64Debug
  -x rustBuildUniversalDebug
  -x rustBuildArmDebug
  -x rustBuildX86Debug
  -x rustBuildX86_64Debug
)

prebuild() {
  cd "$EXAMPLE"
  pnpm tauri android build --debug --features android-test-harness
  cd src-tauri
  cargo ndk -t "$NDK_ARCH" -o gen/android/app/src/main/jniLibs build \
    --package serialport-test --features android-test-harness --lib
  cd ..
  "$ROOT/scripts/sync-android-integration-tests.sh"
}

run_tests() {
  cd "$EXAMPLE/src-tauri/gen/android"
  if [[ -z "${JAVA_HOME:-}" ]]; then
    if [[ "$(uname)" == "Darwin" ]]; then
      export JAVA_HOME="$(/usr/libexec/java_home -v 17)"
    fi
  fi
  chmod +x ./gradlew
  ./gradlew "$GRADLE_TASK" "${GRADLE_SKIP_RUST[@]}"
}

case "$PHASE" in
  prebuild) prebuild ;;
  test) run_tests ;;
  all)
    prebuild
    run_tests
    ;;
  *)
    echo "error: unknown phase '$PHASE' (prebuild|test|all)" >&2
    exit 1
    ;;
esac
