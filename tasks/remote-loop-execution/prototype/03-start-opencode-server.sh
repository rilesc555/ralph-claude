#!/usr/bin/env bash
# 03-start-opencode-server.sh
#
# Starts an opencode serve instance on a given port.
# In the real deployment, this runs on the remote machine.
# For the prototype, we run it locally and verify Ziti can proxy to it.
#
# Prerequisites:
# - opencode CLI installed
# - A project directory to work in (or uses a temp dir)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
OPENCODE_PORT="${OPENCODE_PORT:-14100}"
OPENCODE_DIR="${OPENCODE_DIR:-$PROTO_DIR/test-project}"
PID_FILE="$PROTO_DIR/opencode-server.pid"
LOG_FILE="$PROTO_DIR/opencode-server.log"

# Check if already running
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "OpenCode server already running (PID: $(cat "$PID_FILE")) on port $OPENCODE_PORT"
    exit 0
fi

# Create a minimal test project if it doesn't exist
if [ ! -d "$OPENCODE_DIR" ]; then
    echo "Creating test project at: $OPENCODE_DIR"
    mkdir -p "$OPENCODE_DIR"
    cat > "$OPENCODE_DIR/README.md" << 'EOF'
# Test Project for Ralph Prototype

This is a minimal project for testing remote loop execution over OpenZiti.
EOF
    # Initialize git repo (opencode may expect one)
    (cd "$OPENCODE_DIR" && git init -q && git add . && git commit -q -m "init")
fi

echo "=== Starting opencode serve ==="
echo "Port: $OPENCODE_PORT"
echo "Working dir: $OPENCODE_DIR"
echo "Log: $LOG_FILE"

# Start opencode serve
cd "$OPENCODE_DIR"
opencode serve --port "$OPENCODE_PORT" > "$LOG_FILE" 2>&1 &
OPENCODE_PID=$!
echo "$OPENCODE_PID" > "$PID_FILE"

echo "OpenCode PID: $OPENCODE_PID"

# Wait for health check
echo "Waiting for opencode server to be healthy..."
MAX_WAIT=30
WAITED=0
while [ $WAITED -lt $MAX_WAIT ]; do
    if curl -sf "http://localhost:$OPENCODE_PORT/global/health" > /dev/null 2>&1; then
        echo "  Server is healthy!"
        break
    fi
    sleep 1
    WAITED=$((WAITED + 1))
    echo "  Waiting... (${WAITED}s)"
done

if [ $WAITED -ge $MAX_WAIT ]; then
    echo "ERROR: OpenCode server failed to start within ${MAX_WAIT}s"
    echo "Check log: $LOG_FILE"
    kill "$OPENCODE_PID" 2>/dev/null || true
    rm -f "$PID_FILE"
    exit 1
fi

echo ""
echo "=== OpenCode server is ready ==="
echo "Health:   http://localhost:$OPENCODE_PORT/global/health"
echo "API:      http://localhost:$OPENCODE_PORT/"
echo "PID:      $OPENCODE_PID"
echo ""
echo "Test it:  curl http://localhost:$OPENCODE_PORT/global/health"
echo ""
echo "Next step: ./04-start-ziti-proxy.sh"
