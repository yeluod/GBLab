#!/usr/bin/env bash
set -euo pipefail

output_root="${1:-.ffmpeg-sdk/linux}"
asset_id="532622743"
asset_sha256="2f0424be5df8caf00e6732240f142df23fb4d2ba586c6dfa450b8b5cdf61263a"
asset_url="https://api.github.com/repos/BtbN/FFmpeg-Builds/releases/assets/${asset_id}"

mkdir -p "$output_root"
output_root="$(cd "$output_root" && pwd)"
archive="$output_root/ffmpeg-linux64-lgpl-shared-8.1.tar.xz"
extract_root="$output_root/extract"

if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 3 -H 'Accept: application/octet-stream' -o "$archive" "$asset_url"
fi
printf '%s  %s\n' "$asset_sha256" "$archive" | sha256sum -c -

if [[ ! -d "$output_root/include" || ! -d "$output_root/lib" ]]; then
  rm -rf "$extract_root" "${output_root:?}/include" "${output_root:?}/lib"
  mkdir -p "$extract_root"
  tar -xJf "$archive" -C "$extract_root"
  package_root="$(find "$extract_root" -mindepth 1 -maxdepth 3 -type d -name include | head -n 1 | xargs -r dirname)"
  [[ -n "$package_root" ]] || { printf 'Unable to locate FFmpeg Linux SDK.\n' >&2; exit 1; }
  cp -R "$package_root/include" "$output_root/include"
  cp -R "$package_root/lib" "$output_root/lib"
fi

for name in avcodec avdevice avfilter avformat avutil swresample swscale; do
  compgen -G "$output_root/lib/lib${name}*.so*" >/dev/null || {
    printf 'Missing FFmpeg library: %s\n' "$name" >&2
    exit 1
  }
done

cat > "$output_root/manifest.json" <<EOF
{
  "source": "$asset_url",
  "assetId": "$asset_id",
  "version": "8.1",
  "archiveSha256": "$asset_sha256",
  "platform": "linux",
  "architecture": "x86_64",
  "license": "LGPL-2.1-or-later"
}
EOF

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    printf 'FFMPEG_INCLUDE_DIR=%s\n' "$output_root/include"
    printf 'FFMPEG_LIBS_DIR=%s\n' "$output_root/lib"
    printf 'FFMPEG_LINK_MODE=dynamic\n'
  } >> "$GITHUB_ENV"
fi

printf 'FFmpeg Linux SDK ready: %s\n' "$output_root"
