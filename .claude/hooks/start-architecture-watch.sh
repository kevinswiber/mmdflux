#!/bin/bash
# Start the architecture watcher daemon if not already running.
# Can be called from SessionStart hook or from check-architecture.sh as recovery.

cd "$CLAUDE_PROJECT_DIR" || exit 0

# Read session_id from stdin JSON if available, otherwise use PID
session_id=$(jq -r '.session_id // empty' 2>/dev/null || true)
if [ -z "$session_id" ]; then
    session_id=$$
fi

pidfile="/tmp/mmdflux-arch-watch-${session_id}.pid"
logfile="/tmp/mmdflux-arch-watch-${session_id}.log"

# Persist session_id for stop hook (only works in SessionStart context)
if [ -n "$CLAUDE_ENV_FILE" ]; then
    echo "export MMDFLUX_ARCH_SESSION_ID=$session_id" >> "$CLAUDE_ENV_FILE"
fi

# Don't double-start
if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    exit 0
fi

# Try the pre-built binary first, fall back to cargo
xtask_bin="$CLAUDE_PROJECT_DIR/target/debug/xtask"
if [ ! -x "$xtask_bin" ]; then
    cargo build --package xtask --quiet 2>/dev/null
fi

if [ -x "$xtask_bin" ]; then
    "$xtask_bin" architecture boundaries --watch --background > "$logfile" 2>&1 &
else
    cargo xtask architecture boundaries --watch --background > "$logfile" 2>&1 &
fi
echo $! > "$pidfile"
disown
