#!/bin/bash
# Stop architecture watcher daemons started by this project's hooks.
# Finds PID files by glob rather than relying on session env vars.

input=$(cat)
session_id=$(echo "$input" | jq -r '.session_id // empty')

if [ -n "$session_id" ]; then
    # Try session-specific cleanup first
    pidfile="/tmp/mmdflux-arch-watch-${session_id}.pid"
    if [ -f "$pidfile" ]; then
        kill "$(cat "$pidfile")" 2>/dev/null || true
        rm -f "$pidfile" "/tmp/mmdflux-arch-watch-${session_id}.log"
        exit 0
    fi
fi

# Fallback: clean up any PID files whose process is still ours
for pidfile in /tmp/mmdflux-arch-watch-*.pid; do
    [ -f "$pidfile" ] || continue
    pid=$(cat "$pidfile")
    if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
    fi
    rm -f "$pidfile"
    logfile="${pidfile%.pid}.log"
    rm -f "$logfile"
done
