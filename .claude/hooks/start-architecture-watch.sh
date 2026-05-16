#!/bin/bash
# Start the architecture host if not already running.
# Can be called from SessionStart hook or from check-architecture.sh as recovery.

# After EnterWorktree the agent's $PWD points at the worktree, but
# $CLAUDE_PROJECT_DIR stays frozen at session-launch (always main).
# Prefer $PWD when it looks like a sibling checkout of this project.
project_dir="$CLAUDE_PROJECT_DIR"
if [ -n "$PWD" ] && [ "$PWD" != "$CLAUDE_PROJECT_DIR" ] \
    && [ -f "$PWD/Cargo.toml" ] && [ -d "$PWD/.claude/hooks" ]; then
    project_dir="$PWD"
fi

cd "$project_dir" || exit 0

# Read session_id from stdin JSON if available, otherwise use PID
session_id=$(jq -r '.session_id // empty' 2>/dev/null || true)
if [ -z "$session_id" ]; then
    session_id=$$
fi

# Include a hash of the project dir so each worktree gets its own host.
project_hash=$(printf '%s' "$project_dir" | shasum -a 256 | cut -c1-12)
pidfile="/tmp/mmdflux-arch-watch-${session_id}-${project_hash}.pid"
logfile="/tmp/mmdflux-arch-watch-${session_id}-${project_hash}.log"

# Persist identifiers for stop hook (only works in SessionStart context)
if [ -n "$CLAUDE_ENV_FILE" ]; then
    echo "export MMDFLUX_ARCH_SESSION_ID=$session_id" >> "$CLAUDE_ENV_FILE"
    echo "export MMDFLUX_ARCH_PROJECT_HASH=$project_hash" >> "$CLAUDE_ENV_FILE"
fi

# Don't double-start
if [ -f "$pidfile" ] && kill -0 "$(cat "$pidfile")" 2>/dev/null; then
    exit 0
fi

# Try the pre-built binary first, fall back to cargo
xtask_bin="$project_dir/target/debug/xtask"
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
