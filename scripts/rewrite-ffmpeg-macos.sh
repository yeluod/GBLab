#!/usr/bin/env bash
set -euo pipefail

binary="${1:?Usage: rewrite-ffmpeg-macos.sh <binary> [framework-dir]}"
framework_dir="${2:-target/release/Frameworks}"

[[ -f "$binary" ]] || { printf 'Binary not found: %s\n' "$binary" >&2; exit 1; }
if ! otool -l "$binary" | grep -q '@executable_path/../Frameworks'; then
  install_name_tool -add_rpath '@executable_path/../Frameworks' "$binary"
fi

while IFS= read -r dependency; do
  dependency_base="$(basename "$dependency")"
  case "$dependency_base" in
    libav*.dylib|libsw*.dylib)
      install_name_tool -change "$dependency" "@rpath/$dependency_base" "$binary"
      ;;
  esac
done < <(otool -L "$binary" | sed -n 's/^[[:space:]]*\([^ ]*\.dylib\).*/\1/p')

for library in "$framework_dir"/*.dylib; do
  [[ -e "$library" ]] || continue
  if otool -L "$library" | rg -q '/opt/homebrew|/usr/local|/Users/runner'; then
    printf 'Unrelocated dependency found in %s\n' "$library" >&2
    exit 1
  fi
done
if otool -L "$binary" | rg -q '/opt/homebrew|/usr/local|/Users/runner'; then
  printf 'Unrelocated FFmpeg dependency found in %s\n' "$binary" >&2
  exit 1
fi
printf 'macOS FFmpeg linkage verified: %s\n' "$binary"
