#!/usr/bin/env bash
set -euo pipefail

output_root="${1:-.ffmpeg-sdk/linux}"
lockfile="${FFMPEG_SDK_LOCKFILE:-toolchains/ffmpeg-sdk.lock.json}"
architecture="$(uname -m)"
platform_key="linux-$architecture"
command -v jq >/dev/null 2>&1 || { printf 'jq is required to prepare FFmpeg.\n' >&2; exit 1; }
[[ -f "$lockfile" ]] || { printf 'FFmpeg lockfile not found: %s\n' "$lockfile" >&2; exit 1; }
source_url="$(jq -er ".platforms[\"$platform_key\"].source.url" "$lockfile")"
filename="$(jq -er ".platforms[\"$platform_key\"].source.filename" "$lockfile")"
expected_sha256="$(jq -er ".platforms[\"$platform_key\"].source.sha256" "$lockfile")"
ffmpeg_version="$(jq -er '.ffmpegVersion' "$lockfile")"
mkdir -p "$output_root"
output_root="$(cd "$output_root" && pwd)"
archive="$output_root/$filename"
extract_root="$output_root/extract"
download_archive() {
  printf 'Downloading FFmpeg SDK: platform=linux architecture=%s version=%s url=%s\n' "$architecture" "$ffmpeg_version" "$source_url"
  curl --fail --location --retry 3 --retry-all-errors --show-error --output "$archive" "$source_url" || {
    printf 'FFmpeg SDK download failed: platform=linux architecture=%s filename=%s url=%s\n' "$architecture" "$filename" "$source_url" >&2
    exit 1
  }
}
if [[ -f "$archive" ]]; then
  actual_sha256="$(sha256sum "$archive" | awk '{print $1}')"
  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    printf 'FFmpeg SDK checksum mismatch: expected=%s actual=%s file=%s\n' "$expected_sha256" "$actual_sha256" "$archive" >&2
    rm -f "$archive"
    download_archive
  fi
else
  download_archive
fi
actual_sha256="$(sha256sum "$archive" | awk '{print $1}')"
[[ "$actual_sha256" == "$expected_sha256" ]] || { printf 'FFmpeg SDK checksum mismatch after download: expected=%s actual=%s file=%s\n' "$expected_sha256" "$actual_sha256" "$archive" >&2; exit 1; }
if [[ ! -d "$output_root/include" || ! -d "$output_root/lib" ]]; then
  rm -rf "$extract_root" "${output_root:?}"/include "${output_root:?}"/lib "${output_root:?}"/bin
  mkdir -p "$extract_root"
  tar -xJf "$archive" -C "$extract_root"
  package_root="$(find "$extract_root" -mindepth 1 -maxdepth 4 -type d -name include -print -quit)"
  package_root="$(dirname "$package_root")"
  [[ -n "$package_root" ]] || { printf 'Unable to locate FFmpeg Linux SDK in %s.\n' "$archive" >&2; exit 1; }
  cp -R "$package_root/include" "$output_root/include"
  cp -R "$package_root/lib" "$output_root/lib"
  [[ ! -d "$package_root/bin" ]] || cp -R "$package_root/bin" "$output_root/bin"
fi
for name in $(jq -er '.requiredLibraries[]' "$lockfile"); do
  library_pattern="$output_root/lib/lib${name}"'*.so*'
  compgen -G "$library_pattern" >/dev/null || { printf 'Missing FFmpeg library: %s (sdk=%s)\n' "$name" "$output_root" >&2; exit 1; }
done
cp "$lockfile" "$output_root/lockfile.json"
cat > "$output_root/manifest.json" <<EOF
{
  "schemaVersion": $(jq -er '.schemaVersion' "$lockfile"),
  "sdkRevision": $(jq -er '.sdkRevision' "$lockfile"),
  "ffmpegVersion": "$ffmpeg_version",
  "platform": "linux",
  "architecture": "$architecture",
  "linkMode": $(jq -er '.linkMode' "$lockfile"),
  "license": $(jq -er '.license' "$lockfile"),
  "source": "$source_url",
  "archiveSha256": "$actual_sha256"
}
EOF
if [[ ! -f "$output_root/FFMPEG-LICENSE.txt" ]]; then
  license_file="$(find "$extract_root" -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' \) -print -quit)"
  if [[ -n "$license_file" ]]; then cp "$license_file" "$output_root/FFMPEG-LICENSE.txt"; else printf '%s\n' 'FFmpeg is distributed under the LGPL-2.1-or-later license.' > "$output_root/FFMPEG-LICENSE.txt"; fi
fi
if [[ -n "${GITHUB_ENV:-}" ]]; then
  { printf 'FFMPEG_INCLUDE_DIR=%s\n' "$output_root/include"; printf 'FFMPEG_LIBS_DIR=%s\n' "$output_root/lib"; printf 'FFMPEG_LINK_MODE=dynamic\n'; } >> "$GITHUB_ENV"
fi
printf 'FFmpeg Linux SDK ready: %s\n' "$output_root"
