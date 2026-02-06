#!/usr/bin/env bash
# 05-start-ziti-client.sh
#
# Starts the Ziti client-side tunneler that:
# - Uses the ralph-client identity
# - Intercepts traffic to ralph-opencode.ziti:14100
# - Routes it through the Ziti overlay to the daemon's hosted service
#
# After this, any local process can connect to ralph-opencode.ziti:14100
# and it transparently reaches the opencode server on the "remote" side.
#
# Prerequisites:
# - 01-04 scripts have been run
# - Root/sudo access (tun device for intercept mode)
#   OR use the proxy subcommand for non-root

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
IDENTITY_DIR="$PROTO_DIR/identities"
CLIENT_IDENTITY="$IDENTITY_DIR/ralph-client.json"
OPENCODE_PORT="${OPENCODE_PORT:-14100}"
INTERCEPT_HOST="ralph-opencode.ziti"
LOCAL_PROXY_PORT="${LOCAL_PROXY_PORT:-14200}"
PID_FILE="$PROTO_DIR/ziti-client.pid"
LOG_FILE="$PROTO_DIR/ziti-client.log"

if [ ! -f "$CLIENT_IDENTITY" ]; then
    echo "ERROR: Client identity not found: $CLIENT_IDENTITY"
    echo "Run 02-create-identities.sh first"
    exit 1
fi

# Check if already running
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Ziti client tunneler already running (PID: $(cat "$PID_FILE"))"
    exit 0
fi

echo "=== Starting Ziti client-side proxy ==="
echo "Identity: $CLIENT_IDENTITY"
echo "Service: ralph-opencode-prototype"
echo "Local proxy: localhost:$LOCAL_PROXY_PORT → Ziti → localhost:$OPENCODE_PORT (remote)"

# Use `ziti tunnel proxy` for non-root usage
# This creates a local TCP listener that forwards through Ziti
# (No tun device needed, no root needed)
# Format: ziti tunnel proxy <service-name:local-port>
echo ""
echo "Starting ziti tunnel proxy..."
echo "Log: $LOG_FILE"

ziti tunnel proxy \
    -i "$CLIENT_IDENTITY" \
    "ralph-opencode-prototype:$LOCAL_PROXY_PORT" \
    > "$LOG_FILE" 2>&1 &

CLIENT_PID=$!
echo "$CLIENT_PID" > "$PID_FILE"

# Wait for proxy to be ready
echo "Waiting for proxy to start..."
sleep 5

if kill -0 "$CLIENT_PID" 2>/dev/null; then
    echo ""
    echo "=== Ziti client proxy is running ==="
    echo "PID: $CLIENT_PID"
    echo "Local endpoint: http://localhost:$LOCAL_PROXY_PORT"
    echo ""
    echo "This proxies: localhost:$LOCAL_PROXY_PORT → Ziti → opencode:$OPENCODE_PORT"
    echo ""
    echo "Test it:  curl http://localhost:$LOCAL_PROXY_PORT/global/health"
    echo "Attach:   opencode attach http://localhost:$LOCAL_PROXY_PORT"
    echo ""
    echo "Next step: ./06-run-tests.sh"
else
    echo "ERROR: Ziti client proxy died"
    echo "Check log: $LOG_FILE"
    rm -f "$PID_FILE"
    exit 1
fi
