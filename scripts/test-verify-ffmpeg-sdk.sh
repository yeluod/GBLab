#!/usr/bin/env bash
set -euo pipefail

test_root="$(mktemp -d)"
trap 'rm -rf "$test_root"' EXIT
sdk_root="$test_root/sdk"
lockfile="$test_root/lock.json"
mkdir -p "$sdk_root/lib"
cat > "$lockfile" <<'JSON'
{
  "schemaVersion": 1,
  "sdkRevision": 1,
  "ffmpegVersion": "8.1",
  "license": "LGPL-2.1-or-later",
  "linkMode": "dynamic",
  "requiredLibraries": ["avcodec"]
}
JSON
cat > "$sdk_root/manifest.json" <<'JSON'
{
  "schemaVersion": 1,
  "sdkRevision": 1,
  "ffmpegVersion": "8.1",
  "platform": "linux",
  "architecture": "x86_64",
  "linkMode": "dynamic",
  "license": "LGPL-2.1-or-later",
  "source": "fixture",
  "archiveSha256": "fixture"
}
JSON
printf '%s\n' 'fixture license' > "$sdk_root/FFMPEG-LICENSE.txt"
: > "$sdk_root/lib/libavcodec.so.62"
FFMPEG_SDK_LOCKFILE="$lockfile" bash scripts/verify-ffmpeg-sdk.sh "$sdk_root" linux
rm "$sdk_root/lib/libavcodec.so.62"
if FFMPEG_SDK_LOCKFILE="$lockfile" bash scripts/verify-ffmpeg-sdk.sh "$sdk_root" linux; then
  printf '%s\n' 'verifier unexpectedly accepted a missing library' >&2
  exit 1
fi
printf '%s\n' 'verify-ffmpeg-sdk fixtures passed'
