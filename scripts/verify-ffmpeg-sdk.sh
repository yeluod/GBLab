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
expected_revision="$(jq -er '.sdkRevision' "$lockfile")"
actual_revision="$(jq -er '.sdkRevision' "$manifest")"
[[ "$expected_revision" == "$actual_revision" ]] || { printf 'FFmpeg SDK revision mismatch: expected %s, got %s\n' "$expected_revision" "$actual_revision" >&2; exit 1; }
expected_license="$(jq -er '.license' "$lockfile")"
actual_license="$(jq -er '.license' "$manifest")"
[[ "$expected_license" == "$actual_license" ]] || { printf 'FFmpeg SDK license mismatch: expected %s, got %s\n' "$expected_license" "$actual_license" >&2; exit 1; }
if [[ -n "$platform" ]]; then
  actual_platform="$(jq -er '.platform' "$manifest")"
  [[ "$actual_platform" == "$platform" ]] || { printf 'FFmpeg SDK platform mismatch: expected %s, got %s\n' "$platform" "$actual_platform" >&2; exit 1; }
fi
jq -er '.requiredLibraries[]' "$lockfile" | while IFS= read -r name; do
  found=0
  for pattern in "$sdk_root/lib/lib"$name'*.so*' "$sdk_root/lib/lib"$name'*.dylib' "$sdk_root/frameworks/lib"$name'*.dylib'; do
    if [[ -e "$pattern" ]]; then found=1; break; fi
  done
  if (( found == 0 )); then printf 'Missing FFmpeg library: %s (sdk=%s)\n' "$name" "$sdk_root" >&2; exit 1; fi
done
[[ -f "$sdk_root/FFMPEG-LICENSE.txt" ]] || { printf 'FFmpeg license file not found: %s/FFMPEG-LICENSE.txt\n' "$sdk_root" >&2; exit 1; }
printf 'FFmpeg SDK verified: platform=%s version=%s root=%s\n' "$platform" "$actual_version" "$sdk_root"
