#!/usr/bin/env bash
set -euo pipefail

output_root="${1:-.ffmpeg-sdk/macos}"
ffmpeg_version="${FFMPEG_VERSION:-8.1.2}"
source_url="${FFMPEG_SOURCE_URL:-https://ffmpeg.org/releases/ffmpeg-${ffmpeg_version}.tar.xz}"
source_sha256="${FFMPEG_SOURCE_SHA256:-464beb5e7bf0c311e68b45ae2f04e9cc2af88851abb4082231742a74d97b524c}"

mkdir -p "$output_root"
output_root="$(cd "$output_root" && pwd)"
archive="$output_root/ffmpeg-${ffmpeg_version}.tar.xz"
source_dir="$output_root/source/ffmpeg-${ffmpeg_version}"
install_dir="$output_root/install"

if [[ ! -f "$archive" ]]; then
  curl --fail --location --retry 3 --output "$archive" "$source_url"
fi
printf '%s  %s\n' "$source_sha256" "$archive" | shasum -a 256 -c -

if [[ ! -f "$install_dir/lib/libavformat.dylib" ]]; then
  rm -rf "$output_root/source" "$install_dir"
  mkdir -p "$output_root/source"
  tar -xJf "$archive" -C "$output_root/source"
  pushd "$source_dir" >/dev/null
  ./configure \
    --prefix="$install_dir" \
    --disable-programs \
    --disable-doc \
    --disable-debug \
    --disable-gpl \
    --disable-version3 \
    --disable-nonfree \
    --disable-static \
    --enable-shared \
    --enable-pic \
    --disable-network
  make -j"$(sysctl -n hw.ncpu)"
  make install
  popd >/dev/null
fi

framework_dir="$output_root/frameworks"
rm -rf "$framework_dir"
mkdir -p "$framework_dir"
for name in avcodec avdevice avfilter avformat avutil swresample swscale; do
  source_lib=""
  for candidate in "$install_dir/lib/lib${name}."*.dylib; do
    candidate_base="$(basename "$candidate")"
    if [[ "$candidate_base" =~ ^lib${name}\.[0-9]+\.dylib$ ]]; then
      source_lib="$candidate"
      break
    fi
  done
  if [[ -z "$source_lib" ]]; then
    source_lib="$(find "$install_dir/lib" -maxdepth 1 -name "lib${name}.*.dylib" | sort | head -n 1)"
  fi
  if [[ -z "$source_lib" ]]; then
    printf 'Missing FFmpeg library: %s\n' "$name" >&2
    exit 1
  fi
  cp -L "$source_lib" "$framework_dir/$(basename "$source_lib")"
done

for library in "$framework_dir"/*.dylib; do
  base="$(basename "$library")"
  install_name_tool -id "@rpath/$base" "$library"
  while IFS= read -r dependency; do
    dependency_base="$(basename "$dependency")"
    case "$dependency_base" in
      libav*.dylib|libsw*.dylib)
        install_name_tool -change "$dependency" "@rpath/$dependency_base" "$library"
        ;;
    esac
  done < <(otool -L "$library" | sed -n 's/^[[:space:]]*\([^ ]*\.dylib\).*/\1/p')
done

cat > "$output_root/manifest.json" <<EOF
{
  "source": "$source_url",
  "version": "$ffmpeg_version",
  "sourceSha256": "$source_sha256",
  "platform": "macos",
  "architecture": "$(uname -m)",
  "license": "LGPL-2.1-or-later"
}
EOF

if [[ ! -f "$output_root/FFMPEG-LICENSE.txt" ]]; then
  for license_file in COPYING.LGPLv2.1 COPYING.LGPLv2 COPYING; do
    if [[ -f "$source_dir/$license_file" ]]; then
      cp "$source_dir/$license_file" "$output_root/FFMPEG-LICENSE.txt"
      break
    fi
  done
fi
[[ -f "$output_root/FFMPEG-LICENSE.txt" ]] || {
  printf 'FFmpeg license file was not found in source archive.\n' >&2
  exit 1
}

if [[ -n "${GITHUB_ENV:-}" ]]; then
  {
    printf 'FFMPEG_INCLUDE_DIR=%s\n' "$install_dir/include"
    printf 'FFMPEG_LIBS_DIR=%s\n' "$install_dir/lib"
    printf 'FFMPEG_LINK_MODE=dynamic\n'
  } >> "$GITHUB_ENV"
fi

printf 'FFmpeg macOS SDK ready: %s\n' "$output_root"
