#!/usr/bin/env bash
# Resolve a JAVA_HOME that can *run* Gradle 8.9 + AGP 8.5.
#
# The daemon JVM must be 11–21. Newer JDKs (22+) fail with
# "Unsupported class file major version …" even if the project compiles to 11.
# Preference: 17 → 21 → 11. Prints the path to stdout; errors on stderr.
set -euo pipefail

MIN=11
MAX=21
PREFER=(17 21 11)

java_major() {
  local home="$1"
  local bin="$home/bin/java"
  [[ -x "$bin" ]] || return 1
  # `java -version` → e.g. openjdk version "17.0.20.1" or "1.8.0_504"
  local ver
  ver="$("$bin" -version 2>&1 | awk -F '"' '/version/ {print $2; exit}')"
  [[ -n "$ver" ]] || return 1
  if [[ "$ver" == 1.* ]]; then
    echo "${ver#1.}" | cut -d. -f1
  else
    echo "$ver" | cut -d. -f1
  fi
}

is_compatible() {
  local major="$1"
  [[ "$major" =~ ^[0-9]+$ ]] || return 1
  (( major >= MIN && major <= MAX ))
}

try_home() {
  local home="$1"
  [[ -n "$home" && -d "$home" ]] || return 1
  local major
  major="$(java_major "$home")" || return 1
  if is_compatible "$major"; then
    echo "$home"
    return 0
  fi
  return 1
}

# 1) Honour JAVA_HOME only when already compatible (do not silently keep JDK 27).
if [[ -n "${JAVA_HOME:-}" ]]; then
  if try_home "$JAVA_HOME"; then
    exit 0
  fi
fi

# 2) Preferred majors via macOS java_home or common Linux paths.
candidates=()
if [[ "$(uname -s)" == "Darwin" ]] && command -v /usr/libexec/java_home >/dev/null 2>&1; then
  for v in "${PREFER[@]}"; do
    candidates+=("$(/usr/libexec/java_home -v "$v" 2>/dev/null || true)")
  done
fi

# Linux / generic installs
for v in "${PREFER[@]}"; do
  for base in \
    "/usr/lib/jvm/java-${v}-openjdk-amd64" \
    "/usr/lib/jvm/java-${v}-openjdk" \
    "/usr/lib/jvm/temurin-${v}-jdk" \
    "/opt/homebrew/opt/openjdk@${v}/libexec/openjdk.jdk/Contents/Home" \
    "/usr/local/opt/openjdk@${v}/libexec/openjdk.jdk/Contents/Home"
  do
    candidates+=("$base")
  done
done

# Any remaining JVMs under common roots (last resort scan)
if [[ "$(uname -s)" == "Darwin" ]]; then
  while IFS= read -r line; do
    # /usr/libexec/java_home -V lines end with the home path
    home="$(awk '{print $NF}' <<<"$line")"
    candidates+=("$home")
  done < <(/usr/libexec/java_home -V 2>&1 | grep -E '^\s+[0-9]' || true)
fi
for root in /usr/lib/jvm "$HOME/.sdkman/candidates/java" "$HOME/Library/Java/JavaVirtualMachines" /Library/Java/JavaVirtualMachines; do
  [[ -d "$root" ]] || continue
  while IFS= read -r home; do
    candidates+=("$home")
  done < <(find "$root" -maxdepth 3 \( -path '*/Contents/Home' -o -name 'java' \) 2>/dev/null \
    | while read -r p; do
        if [[ "$(basename "$p")" == "java" && -x "$p" ]]; then
          dirname "$(dirname "$p")"
        elif [[ "$(basename "$p")" == "Home" ]]; then
          echo "$p"
        fi
      done || true)
done

# Prefer exact PREFER majors first among discovered homes.
for want in "${PREFER[@]}"; do
  for home in "${candidates[@]}"; do
    [[ -n "$home" && -d "$home" ]] || continue
    major="$(java_major "$home" 2>/dev/null || true)"
    [[ "$major" == "$want" ]] || continue
    echo "$home"
    exit 0
  done
done

# Any compatible major in range.
for home in "${candidates[@]}"; do
  if try_home "$home"; then
    exit 0
  fi
done

echo "error: no JDK ${MIN}-${MAX} found for Gradle (current JAVA_HOME=${JAVA_HOME:-unset})." >&2
echo "Install Temurin/OpenJDK 17, e.g.:" >&2
echo "  macOS:  brew install openjdk@17" >&2
echo "  Ubuntu: sudo apt-get install -y openjdk-17-jdk" >&2
echo "Then: export JAVA_HOME=\$(./scripts/java-home-for-gradle.sh)" >&2
exit 1
