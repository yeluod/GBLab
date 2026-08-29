#!/usr/bin/env bash
set -euo pipefail

output_root="${1:-.ffmpeg-sdk/macos}"
lockfile="${FFMPEG_SDK_LOCKFILE:-toolchains/ffmpeg-sdk.lock.json}"
architecture="$(uname -m)"
platform_key="macos-$architecture"
command -v jq >/dev/null 2>&1 || { printf 'jq is required to prepare FFmpeg.\n' >&2; exit 1; }
[[ -f "$lockfile" ]] || { printf 'FFmpeg lockfile not found: %s\n' "$lockfile" >&2; exit 1; }
source_url="$(jq -er ".platforms[\"$platform_key\"].source.url" "$lockfile")"
filename="$(jq -er ".platforms[\"$platform_key\"].source.filename" "$lockfile")"
expected_sha256="$(jq -er ".platforms[\"$platform_key\"].source.sha256" "$lockfile")"
ffmpeg_version="$(jq -er '.ffmpegVersion' "$lockfile")"
mkdir -p "$output_root"
output_root="$(cd "$output_root" && pwd)"
archive="$output_root/$filename"
source_root="$output_root/source"
install_dir="$output_root/install"
download_archive() {
  printf 'Downloading FFmpeg source: platform=macos architecture=%s version=%s url=%s\n' "$architecture" "$ffmpeg_version" "$source_url"
  curl --fail --location --retry 3 --retry-all-errors --show-error --output "$archive" "$source_url" || {
    printf 'FFmpeg source download failed: platform=macos architecture=%s filename=%s url=%s\n' "$architecture" "$filename" "$source_url" >&2
    exit 1
  }
}
if [[ -f "$archive" ]]; then
  actual_sha256="$(shasum -a 256 "$archive" | awk '{print $1}')"
  if [[ "$actual_sha256" != "$expected_sha256" ]]; then
    printf 'FFmpeg source checksum mismatch: expected=%s actual=%s file=%s\n' "$expected_sha256" "$actual_sha256" "$archive" >&2
    rm -f "$archive"
    download_archive
  fi
else
  download_archive
fi
actual_sha256="$(shasum -a 256 "$archive" | awk '{print $1}')"
[[ "$actual_sha256" == "$expected_sha256" ]] || { printf 'FFmpeg source checksum mismatch after download: expected=%s actual=%s\n' "$expected_sha256" "$actual_sha256" >&2; exit 1; }
if [[ ! -f "$install_dir/lib/libavformat.dylib" ]]; then
  rm -rf "$source_root" "$install_dir"
  mkdir -p "$source_root"
  tar -xJf "$archive" -C "$source_root"
  source_dir="$(find "$source_root" -mindepth 1 -maxdepth 2 -type d -name 'ffmpeg-*' -print -quit)"
  [[ -n "$source_dir" ]] || { printf 'Unable to locate FFmpeg source directory in %s.\n' "$archive" >&2; exit 1; }
  pushd "$source_dir" >/dev/null
  ./configure --prefix="$install_dir" --disable-programs --disable-doc --disable-debug --disable-gpl --disable-version3 --disable-nonfree --disable-static --enable-shared --enable-pic --disable-network
  make -j"$(sysctl -n hw.ncpu)"
  make install
  popd >/dev/null
else
  source_dir="$(find "$source_root" -mindepth 1 -maxdepth 2 -type d -name 'ffmpeg-*' -print -quit)"
fi
framework_dir="$output_root/frameworks"
rm -rf "$framework_dir"
mkdir -p "$framework_dir"
for name in $(jq -er '.requiredLibraries[]' "$lockfile"); do
  source_lib="$(find "$install_dir/lib" -maxdepth 1 -name "lib${name}.*.dylib" -print -quit)"
  [[ -n "$source_lib" ]] || { printf 'Missing FFmpeg library: %s\n' "$name" >&2; exit 1; }
  cp -L "$source_lib" "$framework_dir/$(basename "$source_lib")"
done
for library in "$framework_dir"/*.dylib; do
  base="$(basename "$library")"
  install_name_tool -id "@rpath/$base" "$library"
  for dependency in $(otool -L "$library" | sed -n 's/^[[:space:]]*\([^ ]*\.dylib\).*/\1/p'); do
    dependency_base="$(basename "$dependency")"
    case "$dependency_base" in
      libav*.dylib|libsw*.dylib) install_name_tool -change "$dependency" "@rpath/$dependency_base" "$library" ;;
    esac
  done
done
cp "$lockfile" "$output_root/lockfile.json"
cat > "$output_root/manifest.json" <<EOF
{
  "schemaVersion": $(jq -er '.schemaVersion' "$lockfile"),
  "sdkRevision": $(jq -er '.sdkRevision' "$lockfile"),
  "ffmpegVersion": "$ffmpeg_version",
  "platform": "macos",
  "architecture": "$architecture",
  "linkMode": $(jq -er '.linkMode' "$lockfile"),
  "license": $(jq -er '.license' "$lockfile"),
  "source": "$source_url",
  "archiveSha256": "$actual_sha256"
}
EOF
if [[ ! -f "$output_root/FFMPEG-LICENSE.txt" ]]; then
  for license_file in COPYING.LGPLv2.1 COPYING.LGPLv2 COPYING; do
    if [[ -f "$source_dir/$license_file" ]]; then cp "$source_dir/$license_file" "$output_root/FFMPEG-LICENSE.txt"; break; fi
  done
fi
[[ -f "$output_root/FFMPEG-LICENSE.txt" ]] || { printf 'FFmpeg license file was not found in source archive.\n' >&2; exit 1; }
if [[ -n "${GITHUB_ENV:-}" ]]; then
  { printf 'FFMPEG_INCLUDE_DIR=%s\n' "$install_dir/include"; printf 'FFMPEG_LIBS_DIR=%s\n' "$install_dir/lib"; printf 'FFMPEG_LINK_MODE=dynamic\n'; } >> "$GITHUB_ENV"
fi
printf 'FFmpeg macOS SDK ready: %s\n' "$output_root"
