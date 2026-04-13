#!/bin/bash
# Check architecture boundaries after edits to .rs files.
# Uses the warm host if available, falls back to standalone run.
# The host's filesystem watcher detects changes automatically.
# If no watcher is running, restart it in the background for next time.
# Exit 2 = block the edit (PostToolUse convention).

input=$(cat)

file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

# Only check on .rs file edits
if [[ "$file_path" != *.rs ]]; then
    exit 0
fi

# If no watcher is running for THIS project dir, restart it for next time.
# PID files encode a hash of the project dir so worktrees don't collide.
project_hash=$(printf '%s' "$CLAUDE_PROJECT_DIR" | shasum -a 256 | cut -c1-12)
watcher_alive=false
for pidfile in /tmp/mmdflux-arch-watch-*-"${project_hash}".pid; do
    [ -f "$pidfile" ] || continue
    if kill -0 "$(cat "$pidfile")" 2>/dev/null; then
        watcher_alive=true
        break
    fi
done

if ! $watcher_alive; then
    "$CLAUDE_PROJECT_DIR"/.claude/hooks/start-architecture-watch.sh </dev/null &
    disown
fi

cd "$CLAUDE_PROJECT_DIR" || exit 0
output=$(cargo +stable xtask architecture check 2>&1)
status=$?
if [ $status -ne 0 ]; then
    echo "$output" >&2
    exit 2
fi
