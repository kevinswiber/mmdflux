#!/bin/bash
# Start the architecture watcher daemon if not already running.
# Codex equivalent of .claude/hooks/start-architecture-watch.sh
# Note: Codex sets cwd to the project directory and passes session_id in JSON stdin.

# Read session_id from stdin JSON if available, otherwise use PID
input=$(cat)
session_id=$(echo "$input" | jq -r '.session_id // empty')
if [ -z "$session_id" ]; then
    session_id=$$
fi

pidfile="/tmp/mmdflux-arch-watch-${session_id}.pid"
logfile="/tmp/mmdflux-arch-watch-${session_id}.log"

# Don't double-start
if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    exit 0
fi

# Try the pre-built binary first, fall back to cargo
xtask_bin="./target/debug/xtask"
if [ ! -x "$xtask_bin" ]; then
    cargo build --package xtask --quiet 2>/dev/null
fi

if [ -x "$xtask_bin" ]; then
    "$xtask_bin" architecture host > "$logfile" 2>&1 &
else
    cargo xtask architecture host > "$logfile" 2>&1 &
fi
echo $! > "$pidfile"
disown
