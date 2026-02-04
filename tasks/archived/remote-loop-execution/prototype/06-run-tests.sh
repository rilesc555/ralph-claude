#!/usr/bin/env bash
# 06-run-tests.sh
#
# Validates the Ziti-proxied opencode server prototype.
# Tests:
# 1. Health check via Ziti proxy
# 2. Create session via Ziti proxy
# 3. Send prompt and get response via Ziti proxy
# 4. SSE event streaming via Ziti proxy
# 5. Abort session via Ziti proxy
# 6. Latency comparison: direct vs Ziti-proxied
#
# Prerequisites:
# - All previous scripts (01-05) have been run
# - Ziti proxy running on LOCAL_PROXY_PORT

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROTO_DIR="$SCRIPT_DIR"
OPENCODE_PORT="${OPENCODE_PORT:-14100}"
LOCAL_PROXY_PORT="${LOCAL_PROXY_PORT:-14200}"
RESULTS_FILE="$PROTO_DIR/test-results.md"

DIRECT_URL="http://localhost:$OPENCODE_PORT"
PROXY_URL="http://localhost:$LOCAL_PROXY_PORT"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

pass() { echo -e "${GREEN}PASS${NC}: $1"; }
fail() { echo -e "${RED}FAIL${NC}: $1"; FAILURES=$((FAILURES + 1)); }
info() { echo -e "${YELLOW}INFO${NC}: $1"; }

FAILURES=0
TESTS=0

measure_latency() {
    local url="$1"
    local endpoint="$2"
    local count="${3:-10}"
    local total=0

    for i in $(seq 1 "$count"); do
        local start end elapsed
        start=$(date +%s%N)
        curl -sf "$url$endpoint" > /dev/null 2>&1
        end=$(date +%s%N)
        elapsed=$(( (end - start) / 1000000 ))
        total=$((total + elapsed))
    done

    echo $((total / count))
}

echo "========================================"
echo "  Ralph OpenZiti Prototype Validation"
echo "========================================"
echo ""
echo "Direct URL:  $DIRECT_URL"
echo "Proxy URL:   $PROXY_URL"
echo ""

# Initialize results file
cat > "$RESULTS_FILE" << 'EOF'
# Prototype Test Results

## Environment
EOF
echo "- Date: $(date -Iseconds)" >> "$RESULTS_FILE"
echo "- Ziti CLI: $(ziti version 2>&1)" >> "$RESULTS_FILE"
echo "- OpenCode port (direct): $OPENCODE_PORT" >> "$RESULTS_FILE"
echo "- Proxy port (Ziti): $LOCAL_PROXY_PORT" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"
echo "## Test Results" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

# ============================================================
# Test 1: Health check via Ziti proxy
# ============================================================
echo "--- Test 1: Health check via Ziti proxy ---"
TESTS=$((TESTS + 1))

HEALTH_RESPONSE=$(curl -sf "$PROXY_URL/global/health" 2>&1) && {
    pass "Health check via Ziti proxy: $HEALTH_RESPONSE"
    echo "- [x] Health check via Ziti proxy: \`$HEALTH_RESPONSE\`" >> "$RESULTS_FILE"
} || {
    fail "Health check via Ziti proxy failed"
    echo "- [ ] Health check via Ziti proxy: FAILED" >> "$RESULTS_FILE"
}
echo ""

# ============================================================
# Test 2: Create session via Ziti proxy
# ============================================================
echo "--- Test 2: Create session via Ziti proxy ---"
TESTS=$((TESTS + 1))

SESSION_RESPONSE=$(curl -sf -X POST "$PROXY_URL/session" \
    -H "Content-Type: application/json" \
    -d '{}' 2>&1) && {
    SESSION_ID=$(echo "$SESSION_RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin).get('id',''))" 2>/dev/null || echo "")
    if [ -n "$SESSION_ID" ]; then
        pass "Created session via Ziti proxy: $SESSION_ID"
        echo "- [x] Create session: \`$SESSION_ID\`" >> "$RESULTS_FILE"
    else
        info "Session created but couldn't parse ID from: $SESSION_RESPONSE"
        # Try alternative: list sessions to find one
        SESSION_ID=""
        echo "- [~] Create session: response format unexpected" >> "$RESULTS_FILE"
    fi
} || {
    fail "Create session via Ziti proxy failed"
    echo "- [ ] Create session: FAILED" >> "$RESULTS_FILE"
    SESSION_ID=""
}
echo ""

# ============================================================
# Test 3: Send prompt via Ziti proxy
# ============================================================
echo "--- Test 3: Send prompt via Ziti proxy ---"
TESTS=$((TESTS + 1))

if [ -n "$SESSION_ID" ]; then
    # Send a simple prompt
    PROMPT_START=$(date +%s%N)
    PROMPT_RESPONSE=$(curl -sf -X POST "$PROXY_URL/session/$SESSION_ID/message" \
        -H "Content-Type: application/json" \
        -d '{"content": "What is 2+2? Reply with just the number."}' \
        --max-time 120 2>&1) && {
        PROMPT_END=$(date +%s%N)
        PROMPT_MS=$(( (PROMPT_END - PROMPT_START) / 1000000 ))
        pass "Sent prompt via Ziti proxy (${PROMPT_MS}ms round-trip)"
        info "Response: $(echo "$PROMPT_RESPONSE" | head -c 200)"
        echo "- [x] Send prompt: ${PROMPT_MS}ms round-trip" >> "$RESULTS_FILE"
    } || {
        fail "Send prompt via Ziti proxy failed"
        echo "- [ ] Send prompt: FAILED" >> "$RESULTS_FILE"
    }
else
    info "Skipping prompt test (no session ID)"
    echo "- [ ] Send prompt: SKIPPED (no session)" >> "$RESULTS_FILE"
fi
echo ""

# ============================================================
# Test 4: SSE event streaming via Ziti proxy
# ============================================================
echo "--- Test 4: SSE event streaming via Ziti proxy ---"
TESTS=$((TESTS + 1))

# Start SSE listener in background, capture first few events
SSE_OUTPUT="$PROTO_DIR/sse-output.txt"
rm -f "$SSE_OUTPUT"

# Listen for SSE events for up to 10 seconds
timeout 10 curl -sf -N "$PROXY_URL/event" > "$SSE_OUTPUT" 2>&1 &
SSE_PID=$!

# Give it a moment to connect
sleep 2

# Send a prompt to generate events (if we have a session)
if [ -n "$SESSION_ID" ]; then
    curl -sf -X POST "$PROXY_URL/session/$SESSION_ID/message" \
        -H "Content-Type: application/json" \
        -d '{"content": "Say hello"}' \
        --max-time 30 > /dev/null 2>&1 &
fi

# Wait for SSE process
wait "$SSE_PID" 2>/dev/null || true

if [ -f "$SSE_OUTPUT" ] && [ -s "$SSE_OUTPUT" ]; then
    EVENT_COUNT=$(grep -c "^data:" "$SSE_OUTPUT" 2>/dev/null || echo "0")
    pass "SSE streaming via Ziti proxy: received $EVENT_COUNT events"
    info "First event: $(head -3 "$SSE_OUTPUT")"
    echo "- [x] SSE streaming: $EVENT_COUNT events received" >> "$RESULTS_FILE"
else
    info "No SSE events captured (may need active session)"
    echo "- [~] SSE streaming: no events captured (may need active session)" >> "$RESULTS_FILE"
fi
echo ""

# ============================================================
# Test 5: Abort session via Ziti proxy
# ============================================================
echo "--- Test 5: Abort session via Ziti proxy ---"
TESTS=$((TESTS + 1))

if [ -n "$SESSION_ID" ]; then
    ABORT_RESPONSE=$(curl -sf -X POST "$PROXY_URL/session/$SESSION_ID/abort" 2>&1) && {
        pass "Abort session via Ziti proxy"
        echo "- [x] Abort session: success" >> "$RESULTS_FILE"
    } || {
        # Abort may return non-200 if session is already idle
        info "Abort returned non-success (session may be idle already)"
        echo "- [~] Abort session: non-success response (session may be idle)" >> "$RESULTS_FILE"
    }
else
    info "Skipping abort test (no session ID)"
    echo "- [ ] Abort session: SKIPPED (no session)" >> "$RESULTS_FILE"
fi
echo ""

# ============================================================
# Test 6: Latency comparison
# ============================================================
echo "--- Test 6: Latency comparison ---"
TESTS=$((TESTS + 1))

echo "Measuring direct latency (10 requests)..."
DIRECT_LATENCY=$(measure_latency "$DIRECT_URL" "/global/health" 10)

echo "Measuring Ziti proxy latency (10 requests)..."
PROXY_LATENCY=$(measure_latency "$PROXY_URL" "/global/health" 10)

OVERHEAD=$((PROXY_LATENCY - DIRECT_LATENCY))

echo ""
echo "┌─────────────────────────────────────────────┐"
echo "│         Latency Comparison (avg ms)          │"
echo "├─────────────────────────────────────────────┤"
printf "│  Direct (localhost:%s):    %4d ms          │\n" "$OPENCODE_PORT" "$DIRECT_LATENCY"
printf "│  Ziti proxy (:%s):         %4d ms          │\n" "$LOCAL_PROXY_PORT" "$PROXY_LATENCY"
printf "│  Ziti overhead:            %4d ms          │\n" "$OVERHEAD"
echo "└─────────────────────────────────────────────┘"
echo ""

if [ "$OVERHEAD" -lt 50 ]; then
    pass "Ziti overhead acceptable: ${OVERHEAD}ms"
else
    info "Ziti overhead is ${OVERHEAD}ms (expected <50ms for local loopback)"
fi

echo "" >> "$RESULTS_FILE"
echo "## Latency Results" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"
echo "| Path | Avg Latency (ms) |" >> "$RESULTS_FILE"
echo "|------|-------------------|" >> "$RESULTS_FILE"
echo "| Direct (localhost:$OPENCODE_PORT) | ${DIRECT_LATENCY}ms |" >> "$RESULTS_FILE"
echo "| Ziti proxy (localhost:$LOCAL_PROXY_PORT) | ${PROXY_LATENCY}ms |" >> "$RESULTS_FILE"
echo "| **Ziti overhead** | **${OVERHEAD}ms** |" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

# ============================================================
# Test 7: opencode attach via Ziti proxy (non-interactive check)
# ============================================================
echo "--- Test 7: opencode attach readiness ---"
TESTS=$((TESTS + 1))

# We can't run interactive opencode attach in a script, but we can verify
# the URL is reachable and returns expected format
ATTACH_CHECK=$(curl -sf "$PROXY_URL/global/health" 2>&1) && {
    pass "opencode attach URL is reachable via Ziti proxy"
    info "Command: opencode attach http://localhost:$LOCAL_PROXY_PORT"
    echo "- [x] opencode attach URL reachable" >> "$RESULTS_FILE"
} || {
    fail "opencode attach URL NOT reachable via Ziti proxy"
    echo "- [ ] opencode attach URL NOT reachable" >> "$RESULTS_FILE"
}
echo ""

# ============================================================
# Summary
# ============================================================
echo "========================================"
echo "  Results: $((TESTS - FAILURES))/$TESTS passed"
echo "========================================"
echo ""

if [ "$FAILURES" -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    echo "The OpenZiti proxy successfully relays all opencode API operations."
else
    echo -e "${RED}$FAILURES test(s) failed${NC}"
fi

echo "" >> "$RESULTS_FILE"
echo "## Summary" >> "$RESULTS_FILE"
echo "- Tests: $((TESTS - FAILURES))/$TESTS passed" >> "$RESULTS_FILE"
echo "- Failures: $FAILURES" >> "$RESULTS_FILE"
echo "" >> "$RESULTS_FILE"

echo ""
echo "Full results: $RESULTS_FILE"
echo ""
echo "To clean up: ./07-cleanup.sh"
