#!/usr/bin/env bash
# Pin the path triggers on .github/workflows/packages-ci.yml so any
# change that could regenerate the wasm-pack output also re-runs the
# packages cross-package integration job.
#
# Verifies each required path appears under BOTH `push.paths` and
# `pull_request.paths`. A path that lives in only one trigger block
# silently bypasses the drift check on the other event type.
#
# Usage:
#   scripts/check-packages-ci-paths.sh

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
WORKFLOW="${REPO_ROOT}/.github/workflows/packages-ci.yml"

if [[ ! -f "${WORKFLOW}" ]]; then
  echo "missing workflow: ${WORKFLOW}" >&2
  exit 1
fi

REQUIRED_PATHS=(
  "packages/**"
  ".github/workflows/packages-ci.yml"
  "crates/mmdflux-wasm/**"
  "src/**"
  "Cargo.toml"
  "Cargo.lock"
  "Justfile"
  ".github/workflows/wasm-release.yml"
)

# Extract the block of lines under `on.<trigger>.paths:` until the next
# sibling key at the same indentation. The expected layout is:
#
#   on:
#     push:
#       paths:
#         - "..."
#     pull_request:
#       paths:
#         - "..."
#
extract_trigger_paths() {
  local trigger="$1"
  awk -v t="${trigger}" '
    # Match `  <trigger>:` at exactly two spaces of indent.
    $0 ~ "^  " t ":[[:space:]]*$" { in_trigger = 1; next }
    # Leaving the trigger block when another sibling key starts.
    in_trigger && /^  [a-z_-]+:[[:space:]]*$/ { in_trigger = 0 }
    # Inside the trigger: capture lines under its `paths:` subkey.
    in_trigger && /^    paths:[[:space:]]*$/ { in_paths = 1; next }
    in_trigger && in_paths && /^    [a-z_-]+:[[:space:]]*$/ { in_paths = 0 }
    in_trigger && in_paths { print }
  ' "${WORKFLOW}"
}

# Parse a single YAML list entry such as `      - "src/**"` into its
# unquoted value. Returns the empty string for lines that aren't list
# entries. Supports single- and double-quoted scalars; unquoted scalars
# are returned as-is. This is a minimal subset — sufficient because the
# workflow uses one entry per line with consistent quoting.
parse_yaml_list_value() {
  local line="$1"
  # Strip leading whitespace and the `- ` marker, if present.
  case "${line}" in
    *-\ *) ;;
    *) printf '' ; return ;;
  esac
  local value="${line#*- }"
  # Trim trailing whitespace.
  value="${value%"${value##*[![:space:]]}"}"
  # Strip a single matched pair of surrounding quotes.
  if [[ "${value}" == \"*\" || "${value}" == \'*\' ]]; then
    value="${value:1:${#value}-2}"
  fi
  printf '%s' "${value}"
}

check_trigger() {
  local trigger="$1"
  local block; block="$(extract_trigger_paths "${trigger}")"
  if [[ -z "${block}" ]]; then
    echo "FAIL ${trigger}: no paths: block found under on.${trigger}" >&2
    return 1
  fi

  # Collect every list-entry value from the block into a set.
  local -a present_paths=()
  while IFS= read -r line; do
    local value
    value="$(parse_yaml_list_value "${line}")"
    if [[ -n "${value}" ]]; then
      present_paths+=("${value}")
    fi
  done <<< "${block}"

  local missing=0
  for path in "${REQUIRED_PATHS[@]}"; do
    local found=0
    for present in "${present_paths[@]}"; do
      if [[ "${present}" == "${path}" ]]; then
        found=1
        break
      fi
    done
    if (( found == 0 )); then
      echo "missing ${trigger}.paths entry: ${path}" >&2
      missing=$((missing + 1))
    fi
  done
  return "${missing}"
}

total_missing=0
for trigger in push pull_request; do
  rc=0
  check_trigger "${trigger}" || rc=$?
  total_missing=$((total_missing + rc))
done

if (( total_missing > 0 )); then
  echo "FAIL: ${total_missing} required path entry/entries absent" >&2
  exit 1
fi

echo "ok: all required path triggers present in push and pull_request blocks"
