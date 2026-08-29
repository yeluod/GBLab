#!/usr/bin/env bash
set -euo pipefail

failed=0
while IFS=: read -r file line value; do
  uses="${value#*uses: }"
  uses="${uses%% *}"
  if [[ "$uses" == ./* ]]; then
    continue
  fi
  if [[ ! "$uses" =~ ^[^/@[:space:]]+/[^@[:space:]]+@[0-9a-fA-F]{40}$ ]]; then
    printf '%s:%s: external action must use a full 40-character commit SHA: %s\n' \
      "$file" "$line" "$uses" >&2
    failed=1
  fi
done < <(rg -n --no-heading '^[[:space:]-]*uses:[[:space:]]+[^[:space:]]+' .github --glob '*.yml' --glob '*.yaml')
exit "$failed"
