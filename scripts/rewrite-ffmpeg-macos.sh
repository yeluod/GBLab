#!/usr/bin/env bash
set -euo pipefail

binary="${1:?Usage: rewrite-ffmpeg-macos.sh <binary> [framework-dir] [source-framework-dir]}"
framework_dir="${2:-target/release/Frameworks}"
source_framework_dir="${3:-.ffmpeg-sdk/macos/frameworks}"
[[ -f "$binary" ]] || { printf 'Binary not found: %s\n' "$binary" >&2; exit 1; }
[[ -d "$source_framework_dir" ]] || { printf 'FFmpeg framework directory not found: %s\n' "$source_framework_dir" >&2; exit 1; }
mkdir -p "$framework_dir"
cp -Lf "$source_framework_dir"/*.dylib "$framework_dir/"
if ! otool -l "$binary" | grep -q '@executable_path/../Frameworks'; then install_name_tool -add_rpath '@executable_path/../Frameworks' "$binary"; fi
rewrite_ffmpeg_dependencies() {
  local file="$1"
  local dependency dependency_base
  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    dependency_base="$(basename "$dependency")"
    case "$dependency_base" in
      libav*.dylib|libsw*.dylib|libpostproc*.dylib)
        if [[ "$dependency" != "@rpath/$dependency_base" ]]; then
          install_name_tool -change "$dependency" "@rpath/$dependency_base" "$file"
        fi
        ;;
    esac
  done < <(otool -L "$file" | tail -n +2 | awk '{print $1}')
}

rewrite_ffmpeg_dependencies "$binary"
for library in "$framework_dir"/*.dylib; do
  [[ -e "$library" ]] || continue
  rewrite_ffmpeg_dependencies "$library"
  if otool -L "$library" | grep -Eq '/opt/homebrew|/usr/local|/Users/runner'; then printf 'Unrelocated dependency found in %s\n' "$library" >&2; exit 1; fi
done
if otool -L "$binary" | grep -Eq '/opt/homebrew|/usr/local|/Users/runner'; then printf 'Unrelocated FFmpeg dependency found in %s\n' "$binary" >&2; exit 1; fi
if otool -L "$binary" | grep -Eq 'ffmpeg\.exe|ffprobe\.exe|ffplay\.exe'; then printf 'FFmpeg CLI dependency found in %s\n' "$binary" >&2; exit 1; fi
printf 'macOS FFmpeg linkage verified: %s\n' "$binary"
