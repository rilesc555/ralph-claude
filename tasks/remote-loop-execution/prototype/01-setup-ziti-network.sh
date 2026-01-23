#!/usr/bin/env bash
# 01-setup-ziti-network.sh
#
# Sets up a local OpenZiti network for the ralph remote loop prototype.
# Uses `ziti edge quickstart` to run controller + edge router in a single process.
#
# This creates:
# - A Ziti controller (manages network)
# - An embedded edge router (handles data plane)
# - A default admin session for creating identities and services
#
# Prerequisites:
# - ziti CLI installed (v1.4+)
# - Port 1280 (controller) and 3022 (edge router) available
#
# Output:
# - Network running in background (PID in ./ziti-network.pid)
# - Admin credentials in ./ziti-admin.env
# - PKI directory at ./ziti-pki/

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
ZITI_HOME="$PROTO_DIR/ziti-home"
PID_FILE="$PROTO_DIR/ziti-network.pid"
ADMIN_ENV="$PROTO_DIR/ziti-admin.env"
LOG_FILE="$PROTO_DIR/ziti-quickstart.log"

# Check if already running
if [ -f "$PID_FILE" ] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    echo "Ziti network already running (PID: $(cat "$PID_FILE"))"
    echo "To stop: kill $(cat "$PID_FILE")"
    exit 0
fi

echo "=== Setting up local OpenZiti network ==="
echo "ZITI_HOME: $ZITI_HOME"

# Clean previous state
rm -rf "$ZITI_HOME"
mkdir -p "$ZITI_HOME"

# Start quickstart in background
echo "Starting ziti edge quickstart..."
echo "Log: $LOG_FILE"

ZITI_HOME="$ZITI_HOME" ziti edge quickstart \
    --ctrl-port 1280 \
    --router-port 3022 \
    --home "$ZITI_HOME" \
    > "$LOG_FILE" 2>&1 &

ZITI_PID=$!
echo "$ZITI_PID" > "$PID_FILE"
echo "Quickstart PID: $ZITI_PID"

# Wait for controller to be ready
echo "Waiting for controller to start..."
MAX_WAIT=60
WAITED=0
while [ $WAITED -lt $MAX_WAIT ]; do
    if ziti edge login "localhost:1280" \
        --username admin --password admin \
        --yes 2>/dev/null; then
        break
    fi
    sleep 2
    WAITED=$((WAITED + 2))
    echo "  Waiting... (${WAITED}s)"
done

if [ $WAITED -ge $MAX_WAIT ]; then
    echo "ERROR: Controller failed to start within ${MAX_WAIT}s"
    echo "Check log: $LOG_FILE"
    kill "$ZITI_PID" 2>/dev/null || true
    rm -f "$PID_FILE"
    exit 1
fi

echo ""
echo "=== Ziti network is ready ==="
echo "Controller: https://localhost:1280"
echo "Admin: admin / admin"
echo "PID: $ZITI_PID"

# Save admin env for other scripts
cat > "$ADMIN_ENV" << EOF
export ZITI_CTRL=https://localhost:1280
export ZITI_USER=admin
export ZITI_PASS=admin
export ZITI_HOME=$ZITI_HOME
export ZITI_PID=$ZITI_PID
EOF

echo ""
echo "Admin env saved to: $ADMIN_ENV"
echo "Source it: source $ADMIN_ENV"
echo ""
echo "Next step: ./02-create-identities.sh"
