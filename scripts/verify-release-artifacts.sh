#!/usr/bin/env bash
set -euo pipefail

release_root="${1:?Usage: verify-release-artifacts.sh <release-assets> }"
[[ -d "$release_root" ]] || { printf 'Release artifact directory not found: %s\n' "$release_root" >&2; exit 1; }

require_artifact() {
  local pattern="$1"
  local description="$2"
  find "$release_root" -type f -name "$pattern" -print -quit | grep -q . || {
    printf 'Missing %s artifact.\n' "$description" >&2
    exit 1
  }
}

require_artifact '*.dmg' 'macOS DMG'
require_artifact '*-setup.exe' 'Windows NSIS installer'
require_artifact '*.msi' 'Windows MSI installer'

checksum_file="$release_root/SHA256SUMS.txt"
: > "$checksum_file"
while IFS= read -r -d '' file; do
  checksum="$(sha256sum "$file" | awk '{print $1}')"
  printf '%s  %s\n' "$checksum" "${file##*/}" >> "$checksum_file"
done < <(find "$release_root" -type f \( -name '*.dmg' -o -name '*.exe' -o -name '*.msi' \) -print0 | sort -z)

[[ -s "$checksum_file" ]] || { printf 'No release installers found for checksum generation.\n' >&2; exit 1; }
printf 'Release artifacts verified and checksummed: root=%s\n' "$release_root"
