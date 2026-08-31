#!/usr/bin/env bash
set -euo pipefail

binary="${1:?Usage: verify-macos-linkage.sh <binary> <framework-dir> [architecture]}"
framework_dir="${2:?Usage: verify-macos-linkage.sh <binary> <framework-dir> [architecture]}"
architecture="${3:-arm64}"

fail() {
  printf 'macOS linkage verification failed: %s\n' "$1" >&2
  exit 1
}

for command_name in file lipo otool; do
  command -v "$command_name" >/dev/null 2>&1 || fail "required command is missing: $command_name"
done
[[ -f "$binary" ]] || fail "application binary not found: $binary"
[[ -d "$framework_dir" ]] || fail "framework directory not found: $framework_dir"

assert_architecture() {
  local file_path="$1"
  local file_output lipo_output
  file_output="$(file "$file_path")"
  [[ "$file_output" == *"$architecture"* ]] || fail "unexpected architecture for $file_path: $file_output"
  lipo_output="$(lipo -info "$file_path")"
  [[ "$lipo_output" == *"$architecture"* ]] || fail "$architecture slice is missing from $file_path: $lipo_output"
}

assert_dependencies() {
  local file_path="$1"
  local dependency
  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    case "$dependency" in
      @rpath/*|/usr/lib/*|/System/Library/*) ;;
      *) fail "non-relocatable dependency in $file_path: $dependency" ;;
    esac
  done < <(otool -L "$file_path" | awk 'NR > 1 { print $1 }')
}

assert_rpath() {
  otool -l "$binary" | grep -Fq '@executable_path/../Frameworks' ||
    fail "application binary has no @executable_path/../Frameworks rpath"
}

assert_framework_id() {
  local file_path="$1"
  local actual_id
  local expected_id
  expected_id="@rpath/$(basename "$file_path")"
  actual_id="$(otool -D "$file_path" | tail -n 1)"
  [[ "$actual_id" == "$expected_id" ]] || fail "unexpected install name for $file_path: $actual_id"
}

assert_rpath
assert_architecture "$binary"
assert_dependencies "$binary"

for library in "$framework_dir"/*.dylib; do
  [[ -e "$library" ]] || continue
  assert_framework_id "$library"
  assert_architecture "$library"
  assert_dependencies "$library"
  while IFS= read -r dependency; do
    [[ -n "$dependency" ]] || continue
    dependency_base="$(basename "$dependency")"
    case "$dependency_base" in
      libav*.dylib|libsw*.dylib|libpostproc*.dylib)
        [[ -f "$framework_dir/$dependency_base" ]] ||
          fail "FFmpeg dependency is missing from framework directory: $dependency_base"
        ;;
    esac
  done < <(otool -L "$library" | awk 'NR > 1 { print $1 }')
done

printf 'macOS linkage verified: binary=%s frameworks=%s architecture=%s\n' \
  "$binary" "$framework_dir" "$architecture"
