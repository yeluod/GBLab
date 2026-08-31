#!/usr/bin/env bash
set -euo pipefail

app_path="${1:?Usage: verify-macos-bundle.sh <app> <dmg> [lockfile]}"
dmg_path="${2:?Usage: verify-macos-bundle.sh <app> <dmg> [lockfile]}"
lockfile="${3:-toolchains/ffmpeg-sdk.lock.json}"

fail() {
  printf 'macOS bundle verification failed: %s\n' "$1" >&2
  exit 1
}

for command_name in jq otool file lipo hdiutil; do
  command -v "$command_name" >/dev/null 2>&1 || fail "required command is missing: $command_name"
done

[[ -d "$app_path" ]] || fail "application bundle not found: $app_path"
[[ -f "$dmg_path" ]] || fail "DMG not found: $dmg_path"
[[ -f "$lockfile" ]] || fail "lockfile not found: $lockfile"
hdiutil imageinfo "$dmg_path" >/dev/null || fail "invalid DMG image: $dmg_path"

expected_libraries="$(jq -cS '.requiredLibraries | sort' "$lockfile")"

is_ffmpeg_name() {
  case "$(basename "$1")" in
    libav*.dylib|libsw*.dylib|libpostproc*.dylib) return 0 ;;
    *) return 1 ;;
  esac
}

assert_relocatable_dependencies() {
  local file="$1"
  local dependency
  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    case "$dependency" in
      @rpath/*|/usr/lib/*|/System/Library/*) ;;
      *) fail "non-relocatable Mach-O dependency in $(basename "$file"): $dependency" ;;
    esac
  done < <(otool -L "$file" | awk 'NR > 1 { print $1 }')
}

assert_macho_architecture() {
  local file="$1"
  local file_output lipo_output
  file_output="$(file "$file")"
  [[ "$file_output" == *arm64* ]] || fail "unexpected architecture for $(basename "$file"): $file_output"
  lipo_output="$(lipo -info "$file")"
  [[ "$lipo_output" == *arm64* ]] || fail "arm64 slice is missing from $(basename "$file"): $lipo_output"
}

assert_framework_id() {
  local file="$1"
  local expected_id
  expected_id="@rpath/$(basename "$file")"
  local actual_id
  actual_id="$(otool -D "$file" | tail -n 1)"
  [[ "$actual_id" == "$expected_id" ]] || fail "unexpected install name for $(basename "$file"): $actual_id"
}

verify_app() {
  local current_app="$1"
  local framework_dir="$current_app/Contents/Frameworks"
  local resources_dir="$current_app/Contents/Resources"
  local binary_path actual_libraries name library dependency dependency_base
  local -a matches
  [[ -d "$framework_dir" ]] || fail "Frameworks directory not found: $framework_dir"
  [[ -f "$resources_dir/manifest.json" ]] || fail "manifest.json is missing from app resources: $current_app"
  [[ -f "$resources_dir/FFMPEG-LICENSE.txt" ]] || fail "FFMPEG-LICENSE.txt is missing from app resources: $current_app"
  actual_libraries="$(jq -cS '.requiredLibraries | sort' "$resources_dir/manifest.json")"
  [[ "$actual_libraries" == "$expected_libraries" ]] || fail "manifest required library set does not match lockfile: $current_app"

  binary_path="$(find "$current_app/Contents/MacOS" -type f -perm -111 -print -quit)"
  [[ -n "$binary_path" ]] || fail "application executable is missing: $current_app"
  assert_relocatable_dependencies "$binary_path"
  assert_macho_architecture "$binary_path"

  for name in $(jq -er '.requiredLibraries[]' "$lockfile"); do
    matches=("$framework_dir/lib${name}"*.dylib)
    [[ -e "${matches[0]}" ]] || fail "required FFmpeg dylib is missing: lib${name} ($current_app)"
  done

  for library in "$framework_dir"/*.dylib; do
    [[ -e "$library" ]] || continue
    is_ffmpeg_name "$library" || continue
    assert_framework_id "$library"
    assert_relocatable_dependencies "$library"
    assert_macho_architecture "$library"
    while IFS= read -r dependency; do
      [[ -n "$dependency" ]] || continue
      dependency_base="$(basename "$dependency")"
      case "$dependency_base" in
        libav*.dylib|libsw*.dylib|libpostproc*.dylib)
          [[ -f "$framework_dir/$dependency_base" ]] || fail "FFmpeg dependency is missing from app: $dependency_base"
          ;;
      esac
    done < <(otool -L "$library" | awk 'NR > 1 { print $1 }')
  done
}

verify_app "$app_path"

mount_point="$(mktemp -d "${TMPDIR:-/tmp}/gblab-dmg.XXXXXXXX")"
cleanup_mount() {
  hdiutil detach "$mount_point" -force >/dev/null 2>&1 || true
  rmdir "$mount_point" >/dev/null 2>&1 || true
}
trap cleanup_mount EXIT
hdiutil attach -readonly -nobrowse -mountpoint "$mount_point" "$dmg_path" >/dev/null ||
  fail "unable to mount DMG: $dmg_path"
dmg_app="$(find "$mount_point" -maxdepth 2 -name '*.app' -print -quit)"
[[ -n "$dmg_app" ]] || fail 'DMG does not contain an application bundle'
verify_app "$dmg_app"

printf 'macOS bundle verified: app=%s dmg=%s\n' "$app_path" "$dmg_path"
