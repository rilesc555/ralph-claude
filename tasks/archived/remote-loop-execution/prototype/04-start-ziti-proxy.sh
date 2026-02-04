#!/usr/bin/env bash
# 04-start-ziti-proxy.sh
#
# Starts the Ziti host-side tunneler that:
# - Binds the ralph-opencode-prototype service using the daemon identity
# - Forwards incoming Ziti connections to localhost:OPENCODE_PORT
#
# This simulates what the ralph daemon would do on the remote machine:
# host a Ziti service that proxies to the local opencode serve port.
#
# Uses ziti-edge-tunnel in host mode (no intercept, just hosting).
#
# Prerequisites:
# - 01-setup-ziti-network.sh (Ziti network running)
# - 02-create-identities.sh (identities enrolled)
# - 03-start-opencode-server.sh (opencode serve running)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
IDENTITY_DIR="$PROTO_DIR/identities"
DAEMON_IDENTITY="$IDENTITY_DIR/ralph-daemon.json"
OPENCODE_PORT="${OPENCODE_PORT:-14100}"
SERVICE_NAME="ralph-opencode-prototype"
PID_FILE="$PROTO_DIR/ziti-host.pid"
LOG_FILE="$PROTO_DIR/ziti-host.log"

if [ ! -f "$DAEMON_IDENTITY" ]; then
    echo "ERROR: Daemon identity not found: $DAEMON_IDENTITY"
    echo "Run 02-create-identities.sh first"
    exit 1
fi

# Check if already running
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Ziti host tunneler already running (PID: $(cat "$PID_FILE"))"
    exit 0
fi

echo "=== Starting Ziti host-side tunneler ==="
echo "Identity: $DAEMON_IDENTITY"
echo "Service: $SERVICE_NAME → localhost:$OPENCODE_PORT"

# First, configure the service's host.v1 config on the controller
# This tells the hosting tunneler where to forward traffic
source "$PROTO_DIR/ziti-admin.env"
ziti edge login "$ZITI_CTRL" \
    --username "$ZITI_USER" --password "$ZITI_PASS" \
    --yes 2>/dev/null

# Create host.v1 config (tells hosting identity where to forward)
# Use 127.0.0.1 explicitly (not "localhost" which may resolve to IPv6 ::1)
echo "Creating host.v1 config..."
ziti edge create config "${SERVICE_NAME}-host" host.v1 \
    "{\"protocol\": \"tcp\", \"address\": \"127.0.0.1\", \"port\": $OPENCODE_PORT}" 2>/dev/null || \
    echo "  (already exists, updating...)" && \
    ziti edge update config "${SERVICE_NAME}-host" \
        --data "{\"protocol\": \"tcp\", \"address\": \"127.0.0.1\", \"port\": $OPENCODE_PORT}" 2>/dev/null || true

# Create intercept.v1 config (tells dialing identity how to reach the service)
INTERCEPT_HOST="ralph-opencode.ziti"
INTERCEPT_PORT="$OPENCODE_PORT"
echo "Creating intercept.v1 config..."
ziti edge create config "${SERVICE_NAME}-intercept" intercept.v1 \
    "{\"protocols\": [\"tcp\"], \"addresses\": [\"$INTERCEPT_HOST\"], \"portRanges\": [{\"low\": $INTERCEPT_PORT, \"high\": $INTERCEPT_PORT}]}" 2>/dev/null || \
    echo "  (already exists)"

# Update service with configs
echo "Updating service with configs..."
ziti edge update service "$SERVICE_NAME" \
    --configs "${SERVICE_NAME}-host,${SERVICE_NAME}-intercept" 2>/dev/null || true

# Start ziti-edge-tunnel in host mode (runs as root for tun device, but host mode doesn't need it)
echo ""
echo "Starting ziti-edge-tunnel run-host..."
echo "Log: $LOG_FILE"

# run-host mode: only hosts services, no interception (no root needed)
ziti-edge-tunnel run-host \
    --identity "$DAEMON_IDENTITY" \
    > "$LOG_FILE" 2>&1 &

TUNNEL_PID=$!
echo "$TUNNEL_PID" > "$PID_FILE"

# Wait for tunnel to connect
echo "Waiting for tunnel to connect to Ziti network..."
sleep 5

if kill -0 "$TUNNEL_PID" 2>/dev/null; then
    echo ""
    echo "=== Ziti host tunneler is running ==="
    echo "PID: $TUNNEL_PID"
    echo "Hosting: $SERVICE_NAME → localhost:$OPENCODE_PORT"
    echo "Intercept: $INTERCEPT_HOST:$INTERCEPT_PORT"
    echo ""
    echo "Next step: ./05-start-ziti-client.sh"
else
    echo "ERROR: Ziti tunnel process died"
    echo "Check log: $LOG_FILE"
    rm -f "$PID_FILE"
    exit 1
fi
