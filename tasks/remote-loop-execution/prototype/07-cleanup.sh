#!/usr/bin/env bash
# 07-cleanup.sh
#
# Stops all prototype processes and optionally cleans up state.
#
# Stops:
# - Ziti client proxy
# - Ziti host tunneler
# - OpenCode server
# - Ziti network (controller + router)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"

stop_process() {
    local name="$1"
    local pid_file="$2"

    if [ -f "$pid_file" ]; then
        local pid
        pid=$(cat "$pid_file")
        if kill -0 "$pid" 2>/dev/null; then
            echo "Stopping $name (PID: $pid)..."
            kill "$pid" 2>/dev/null || true
            sleep 1
            kill -9 "$pid" 2>/dev/null || true
        else
            echo "$name not running (stale PID file)"
        fi
        rm -f "$pid_file"
    else
        echo "$name: no PID file found"
    fi
}

echo "=== Cleaning up prototype ==="

stop_process "Ziti client proxy" "$PROTO_DIR/ziti-client.pid"
stop_process "Ziti host tunneler" "$PROTO_DIR/ziti-host.pid"
stop_process "OpenCode server" "$PROTO_DIR/opencode-server.pid"
stop_process "Ziti network" "$PROTO_DIR/ziti-network.pid"

echo ""

# Ask about full cleanup
if [ "${FULL_CLEANUP:-0}" = "1" ] || [ "${1:-}" = "--full" ]; then
    echo "Full cleanup: removing all state..."
    rm -rf "$PROTO_DIR/ziti-home"
    rm -rf "$PROTO_DIR/identities"
    rm -rf "$PROTO_DIR/test-project"
    rm -f "$PROTO_DIR"/*.log
    rm -f "$PROTO_DIR"/*.pid
    rm -f "$PROTO_DIR/ziti-admin.env"
    rm -f "$PROTO_DIR/sse-output.txt"
    echo "Done. All prototype state removed."
else
    echo "Processes stopped. State preserved."
    echo "For full cleanup: $0 --full"
fi

echo ""
echo "=== Cleanup complete ==="
