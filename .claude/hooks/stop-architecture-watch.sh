#!/bin/bash
# Stop architecture watcher daemons started by this project's hooks.
# Works from SessionEnd (has env vars) and WorktreeRemove (has worktree_path in JSON).

input=$(cat)
session_id=$(echo "$input" | jq -r '.session_id // empty')
worktree_path=$(echo "$input" | jq -r '.worktree_path // empty')
project_hash="${MMDFLUX_ARCH_PROJECT_HASH:-}"

# Derive project hash from worktree_path if env var isn't set (WorktreeRemove context)
if [ -z "$project_hash" ] && [ -n "$worktree_path" ]; then
    project_hash=$(printf '%s' "$worktree_path" | shasum -a 256 | cut -c1-12)
fi

# Try session+project-specific cleanup first
if [ -n "$session_id" ] && [ -n "$project_hash" ]; then
    pidfile="/tmp/mmdflux-arch-watch-${session_id}-${project_hash}.pid"
    if [ -f "$pidfile" ]; then
        kill "$(cat "$pidfile")" 2>/dev/null || true
        rm -f "$pidfile" "/tmp/mmdflux-arch-watch-${session_id}-${project_hash}.log"
        exit 0
    fi
fi

# Try project-hash-only cleanup (matches any session for this worktree)
if [ -n "$project_hash" ]; then
    for pidfile in /tmp/mmdflux-arch-watch-*-"${project_hash}".pid; do
        [ -f "$pidfile" ] || continue
        pid=$(cat "$pidfile")
        kill "$pid" 2>/dev/null || true
        rm -f "$pidfile" "${pidfile%.pid}.log"
    done
    exit 0
fi

# Try legacy (session-only) cleanup for hosts started before the hash change
if [ -n "$session_id" ]; then
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
