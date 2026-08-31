#!/usr/bin/env bash
set -euo pipefail

binary="${1:?Usage: rewrite-ffmpeg-macos.sh <binary> [framework-dir]}"
framework_dir="${2:-target/release/Frameworks}"
[[ -f "$binary" ]] || { printf 'Binary not found: %s\n' "$binary" >&2; exit 1; }
[[ -d "$framework_dir" ]] || { printf 'FFmpeg framework directory not found: %s\n' "$framework_dir" >&2; exit 1; }
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
  base="$(basename "$library")"
  # Cached SDKs may contain an absolute LC_ID_DYLIB from the build machine.
  # Normalize the install name again after copying into the final artifact.
  install_name_tool -id "@rpath/$base" "$library"
  rewrite_ffmpeg_dependencies "$library"
done
bash "$(dirname "$0")/verify-macos-linkage.sh" "$binary" "$framework_dir"
