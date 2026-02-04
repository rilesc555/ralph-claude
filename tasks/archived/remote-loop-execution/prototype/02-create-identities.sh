#!/usr/bin/env bash
# 02-create-identities.sh
#
# Creates Ziti identities for the ralph prototype:
# - ralph-daemon: Server identity (hosts the opencode proxy service)
# - ralph-client: Client identity (dials the service)
#
# Also creates the service and policies to allow traffic flow.
#
# Prerequisites:
# - 01-setup-ziti-network.sh has been run
# - Ziti controller is running

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
ADMIN_ENV="$PROTO_DIR/ziti-admin.env"
IDENTITY_DIR="$PROTO_DIR/identities"

if [ ! -f "$ADMIN_ENV" ]; then
    echo "ERROR: Run 01-setup-ziti-network.sh first"
    exit 1
fi

source "$ADMIN_ENV"

# Login
echo "=== Logging into Ziti controller ==="
ziti edge login "$ZITI_CTRL" \
    --username "$ZITI_USER" --password "$ZITI_PASS" \
    --yes 2>/dev/null

mkdir -p "$IDENTITY_DIR"

echo ""
echo "=== Creating identities ==="

# Create daemon identity (will host the service)
echo "Creating ralph-daemon identity..."
ziti edge create identity ralph-daemon \
    --role-attributes "ralph-daemon" \
    -o "$IDENTITY_DIR/ralph-daemon.jwt" 2>/dev/null || \
    echo "  (already exists)"

# Create client identity (will dial the service)
echo "Creating ralph-client identity..."
ziti edge create identity ralph-client \
    --role-attributes "ralph-client" \
    -o "$IDENTITY_DIR/ralph-client.jwt" 2>/dev/null || \
    echo "  (already exists)"

echo ""
echo "=== Enrolling identities ==="

# Enroll daemon identity
if [ ! -f "$IDENTITY_DIR/ralph-daemon.json" ]; then
    echo "Enrolling ralph-daemon..."
    ziti edge enroll "$IDENTITY_DIR/ralph-daemon.jwt" \
        -o "$IDENTITY_DIR/ralph-daemon.json"
else
    echo "ralph-daemon already enrolled"
fi

# Enroll client identity
if [ ! -f "$IDENTITY_DIR/ralph-client.json" ]; then
    echo "Enrolling ralph-client..."
    ziti edge enroll "$IDENTITY_DIR/ralph-client.jwt" \
        -o "$IDENTITY_DIR/ralph-client.json"
else
    echo "ralph-client already enrolled"
fi

echo ""
echo "=== Creating service and policies ==="

# Create the opencode proxy service
SERVICE_NAME="ralph-opencode-prototype"
echo "Creating service: $SERVICE_NAME"
ziti edge create service "$SERVICE_NAME" \
    --role-attributes "ralph-opencode-service" 2>/dev/null || \
    echo "  (already exists)"

# Service policies: who can bind (host) and dial (connect)
echo "Creating bind policy (daemon can host)..."
ziti edge create service-policy "${SERVICE_NAME}-bind" Bind \
    --service-roles "@${SERVICE_NAME}" \
    --identity-roles "#ralph-daemon" 2>/dev/null || \
    echo "  (already exists)"

echo "Creating dial policy (client can connect)..."
ziti edge create service-policy "${SERVICE_NAME}-dial" Dial \
    --service-roles "@${SERVICE_NAME}" \
    --identity-roles "#ralph-client" 2>/dev/null || \
    echo "  (already exists)"

# Edge router policies: which identities can use which routers
echo "Creating edge-router-policy..."
ziti edge create edge-router-policy "ralph-all-routers" \
    --edge-router-roles "#all" \
    --identity-roles "#all" 2>/dev/null || \
    echo "  (already exists)"

echo "Creating service-edge-router-policy..."
ziti edge create service-edge-router-policy "ralph-all-services" \
    --edge-router-roles "#all" \
    --service-roles "#all" 2>/dev/null || \
    echo "  (already exists)"

echo ""
echo "=== Identities and policies created ==="
echo ""
echo "Identities:"
echo "  Daemon: $IDENTITY_DIR/ralph-daemon.json"
echo "  Client: $IDENTITY_DIR/ralph-client.json"
echo ""
echo "Service: $SERVICE_NAME"
echo "  Bind: ralph-daemon identity"
echo "  Dial: ralph-client identity"
echo ""
echo "Next step: ./03-start-opencode-server.sh"
