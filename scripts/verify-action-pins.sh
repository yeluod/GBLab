#!/usr/bin/env bash
set -euo pipefail

failed=0
resolved_actions=()

resolve_action_commit() {
  local action_path="$1"
  local sha="$2"
  local owner repository repository_path resolved

  owner="${action_path%%/*}"
  repository_path="${action_path#*/}"
  repository="${owner}/${repository_path%%/*}"
  resolved="${repository}@${sha}"

  for existing in "${resolved_actions[@]-}"; do
    if [[ "$existing" == "$resolved" ]]; then
      return 0
    fi
  done
  resolved_actions+=("$resolved")

  if ! gh api --silent "repos/${repository}/commits/${sha}"; then
    printf 'external action commit does not exist or is not accessible: %s\n' "$resolved" >&2
    return 1
  fi
}

if ! command -v gh >/dev/null 2>&1; then
  printf 'GitHub CLI is required to resolve pinned action commits.\n' >&2
  exit 1
fi

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
    continue
  fi
  action_path="${uses%@*}"
  sha="${uses##*@}"
  if ! resolve_action_commit "$action_path" "$sha"; then
    printf '%s:%s: invalid external action pin: %s\n' "$file" "$line" "$uses" >&2
    failed=1
  fi
done < <(rg -n --no-heading '^[[:space:]-]*uses:[[:space:]]+[^[:space:]]+' .github --glob '*.yml' --glob '*.yaml')
exit "$failed"
