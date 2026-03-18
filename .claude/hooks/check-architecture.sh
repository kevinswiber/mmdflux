#!/bin/bash
# Check architecture boundaries after edits to .rs files.
# Uses the warm daemon if available, falls back to standalone run.
# --notify-dirty tells the daemon to mark itself dirty before checking,
# avoiding stale cached results from before the edit.
# If no watcher is running, restart it in the background for next time.
# Exit 2 = block the edit (PostToolUse convention).

input=$(cat)

file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

# Only check on .rs file edits
if [[ "$file_path" != *.rs ]]; then
    exit 0
fi

# If no watcher is running, restart it for next time
watcher_alive=false
for pidfile in /tmp/mmdflux-arch-watch-*.pid; do
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
output=$(cargo xtask architecture boundaries --notify-dirty 2>&1)
status=$?
if [ $status -ne 0 ]; then
    echo "$output" >&2
    exit 2
fi
