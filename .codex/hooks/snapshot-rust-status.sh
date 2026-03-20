#!/bin/bash
set -euo pipefail

# Snapshot Rust-related git status at the start of a turn so the Stop hook can
# tell whether this turn created/edited/deleted any .rs paths.

input=$(cat)
session_id=$(printf '%s' "$input" | jq -r '.session_id // empty')
cwd=$(printf '%s' "$input" | jq -r '.cwd // empty')

if [ -z "$session_id" ] || [ -z "$cwd" ]; then
    exit 0
fi

cd "$cwd" || exit 0

snapshot_file="/tmp/mmdflux-rust-status-${session_id}.txt"

rust_git_status() {
    git -c core.quotepath=false status --porcelain=v1 --untracked-files=all 2>/dev/null \
        | grep -E '(^.. .*\.rs$)|(^.. .*\.rs -> .*$)|(^.. .* -> .*\.rs$)' \
        | LC_ALL=C sort \
        || true
}

current_status=$(rust_git_status)

printf '%s' "$current_status" > "$snapshot_file"
