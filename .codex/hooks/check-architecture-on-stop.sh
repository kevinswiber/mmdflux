#!/bin/bash
set -euo pipefail

# Run architecture boundaries at the end of a turn when Rust-related git status
# changed during that turn. If a prior Stop hook already blocked the turn,
# re-run the check until it passes.

input=$(cat)
session_id=$(printf '%s' "$input" | jq -r '.session_id // empty')
cwd=$(printf '%s' "$input" | jq -r '.cwd // empty')
stop_hook_active=$(printf '%s' "$input" | jq -r '.stop_hook_active // false')

if [ -z "$session_id" ] || [ -z "$cwd" ]; then
    exit 0
fi

cd "$cwd" || exit 0

snapshot_file="/tmp/mmdflux-rust-status-${session_id}.txt"
log_file="/tmp/mmdflux-stop-hook-${session_id}.log"
previous_status=""
if [ -f "$snapshot_file" ]; then
    previous_status=$(cat "$snapshot_file")
fi

rust_git_status() {
    git -c core.quotepath=false status --porcelain=v1 --untracked-files=all 2>/dev/null \
        | grep -E '(^.. .*\.rs$)|(^.. .*\.rs -> .*$)|(^.. .* -> .*\.rs$)' \
        | LC_ALL=C sort \
        || true
}

status_lines_for_comm() {
    if [ -n "${1:-}" ]; then
        printf '%s\n' "$1"
    fi
}

current_status=$(rust_git_status)

rust_status_changed=false
if [ "$current_status" != "$previous_status" ]; then
    rust_status_changed=true
fi

if [ "$stop_hook_active" != "true" ] && [ "$rust_status_changed" != "true" ]; then
    exit 0
fi

delta_lines=$(
    comm -3 \
        <(status_lines_for_comm "$previous_status") \
        <(status_lines_for_comm "$current_status") \
        | awk '{ sub(/^\t+/, ""); print }' \
        | sed '/^$/d' \
        || true
)

watched_rust_change=false
if printf '%s\n' "$delta_lines" | grep -Eq '(^.. src/.*\.rs$)|(^.. src/.*\.rs -> .*$)|(^.. .* -> src/.*\.rs$)'; then
    watched_rust_change=true
fi

if [ "$watched_rust_change" = "true" ]; then
    architecture_cmd=(cargo xtask architecture check)
else
    architecture_cmd=(cargo xtask architecture check --fresh)
fi

{
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] stop hook start"
    echo "stop_hook_active=$stop_hook_active"
    echo "rust_status_changed=$rust_status_changed"
    echo "watched_rust_change=$watched_rust_change"
    echo "command=${architecture_cmd[*]}"
    if [ -n "$delta_lines" ]; then
        echo "delta_lines:"
        printf '%s\n' "$delta_lines"
    fi
} >> "$log_file"

set +e
output=$("${architecture_cmd[@]}" 2>&1)
status=$?
set -e

{
    echo "status=$status"
    if [ -n "$output" ]; then
        echo "output:"
        printf '%s\n' "$output"
    fi
    echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] stop hook end"
    echo
} >> "$log_file"

if [ $status -ne 0 ]; then
    {
        echo "Architecture boundaries failed after Rust file changes in this turn."
        echo "Hook log: $log_file"
        echo
        echo "$output"
    } >&2
    exit 2
fi
