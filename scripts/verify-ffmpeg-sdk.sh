#!/usr/bin/env bash
set -euo pipefail

sdk_root="${1:?Usage: verify-ffmpeg-sdk.sh <sdk-root> [platform]}"
platform="${2:-}"
manifest="$sdk_root/manifest.json"
lockfile="${FFMPEG_SDK_LOCKFILE:-toolchains/ffmpeg-sdk.lock.json}"
[[ -d "$sdk_root" ]] || { printf 'FFmpeg SDK directory not found: %s\n' "$sdk_root" >&2; exit 1; }
[[ -f "$manifest" ]] || { printf 'FFmpeg SDK manifest not found: %s\n' "$manifest" >&2; exit 1; }
[[ -f "$lockfile" ]] || { printf 'FFmpeg SDK lockfile not found: %s\n' "$lockfile" >&2; exit 1; }
command -v jq >/dev/null 2>&1 || { printf 'jq is required to verify the FFmpeg SDK manifest.\n' >&2; exit 1; }
expected_version="$(jq -er '.ffmpegVersion' "$lockfile")"
actual_version="$(jq -er '.ffmpegVersion' "$manifest")"
[[ "$expected_version" == "$actual_version" ]] || { printf 'FFmpeg SDK version mismatch: expected %s, got %s\n' "$expected_version" "$actual_version" >&2; exit 1; }
expected_schema="$(jq -er '.schemaVersion' "$lockfile")"
actual_schema="$(jq -er '.schemaVersion' "$manifest")"
[[ "$expected_schema" == "$actual_schema" ]] || { printf 'FFmpeg SDK schema mismatch: expected %s, got %s\n' "$expected_schema" "$actual_schema" >&2; exit 1; }
expected_revision="$(jq -er '.sdkRevision' "$lockfile")"
actual_revision="$(jq -er '.sdkRevision' "$manifest")"
[[ "$expected_revision" == "$actual_revision" ]] || { printf 'FFmpeg SDK revision mismatch: expected %s, got %s\n' "$expected_revision" "$actual_revision" >&2; exit 1; }
expected_license="$(jq -er '.license' "$lockfile")"
actual_license="$(jq -er '.license' "$manifest")"
[[ "$expected_license" == "$actual_license" ]] || { printf 'FFmpeg SDK license mismatch: expected %s, got %s\n' "$expected_license" "$actual_license" >&2; exit 1; }
expected_link_mode="$(jq -er '.linkMode' "$lockfile")"
actual_link_mode="$(jq -er '.linkMode' "$manifest")"
[[ "$expected_link_mode" == "$actual_link_mode" ]] || { printf 'FFmpeg SDK link mode mismatch: expected %s, got %s\n' "$expected_link_mode" "$actual_link_mode" >&2; exit 1; }
if [[ -n "$platform" ]]; then
  actual_platform="$(jq -er '.platform' "$manifest")"
  [[ "$actual_platform" == "$platform" ]] || { printf 'FFmpeg SDK platform mismatch: expected %s, got %s\n' "$platform" "$actual_platform" >&2; exit 1; }
fi
actual_architecture="$(jq -er '.architecture' "$manifest")"
[[ -n "$actual_architecture" ]] || { printf 'FFmpeg SDK architecture is missing.\n' >&2; exit 1; }
platform_key="$(jq -er --arg platform "$platform" --arg architecture "$actual_architecture" '.platforms | to_entries[] | select(.value.platform == $platform and .value.architecture == $architecture) | .key' "$lockfile")"
expected_source="$(jq -er --arg key "$platform_key" '.platforms[$key].source.url' "$lockfile")"
actual_source="$(jq -er '.source' "$manifest")"
[[ "$actual_source" == "$expected_source" ]] || { printf 'FFmpeg SDK source mismatch: expected %s, got %s\n' "$expected_source" "$actual_source" >&2; exit 1; }
expected_archive_sha="$(jq -er --arg key "$platform_key" '.platforms[$key].source.sha256' "$lockfile")"
actual_archive_sha="$(jq -er '.archiveSha256' "$manifest")"
[[ "$actual_archive_sha" == "$expected_archive_sha" ]] || { printf 'FFmpeg SDK archive checksum mismatch: expected %s, got %s\n' "$expected_archive_sha" "$actual_archive_sha" >&2; exit 1; }
expected_libraries="$(jq -cS '.requiredLibraries | sort' "$lockfile")"
actual_libraries="$(jq -cS '.requiredLibraries | sort' "$manifest")"
[[ "$actual_libraries" == "$expected_libraries" ]] || { printf 'FFmpeg SDK required library set mismatch.\n' >&2; exit 1; }
jq -e '(.requiredLibraries as $required | (.requiredRuntimeImports | length > 0) and all(.requiredRuntimeImports[]; . as $name | $required | index($name) != null))' "$lockfile" >/dev/null || {
  printf 'FFmpeg requiredRuntimeImports must be a non-empty subset of requiredLibraries.\n' >&2
  exit 1
}
jq -er '.requiredLibraries[]' "$lockfile" | while IFS= read -r name; do
  found=0
  for directory in "$sdk_root/lib" "$sdk_root/frameworks" "$sdk_root/frameworks/lib"; do
    [[ -d "$directory" ]] || continue
    if find "$directory" -maxdepth 1 -type f \( -name "lib${name}.so" -o -name "lib${name}.so.*" -o -name "lib${name}.*.dylib" \) -print -quit | grep -q .; then
      found=1
      break
    fi
  done
  if (( found == 0 )); then printf 'Missing FFmpeg library: %s (sdk=%s)\n' "$name" "$sdk_root" >&2; exit 1; fi
done
[[ -f "$sdk_root/FFMPEG-LICENSE.txt" ]] || { printf 'FFmpeg license file not found: %s/FFMPEG-LICENSE.txt\n' "$sdk_root" >&2; exit 1; }
printf 'FFmpeg SDK verified: platform=%s version=%s root=%s\n' "$platform" "$actual_version" "$sdk_root"
