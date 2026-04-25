#!/usr/bin/env bash
set -euo pipefail

BUDGET_MS=${BUDGET_MS:-420000}
SOCKET=/tmp/agentd-perf.sock
PROJECT_ROOT=/tmp/mock-project

cleanup() { kill "$SERVER_PID" 2>/dev/null || true; rm -f "$SOCKET"; }
trap cleanup EXIT

cargo build --release -p agentd
ulimit -n 65536

sudo ./target/release/agentd socket --path "$SOCKET" &
SERVER_PID=$!
sleep 2

OUT=$(./target/release/agentd simulate \
    --socket "$SOCKET" \
    --project-root "$PROJECT_ROOT" \
    --max-agents 1000 \
    --tasks 2000 \
    --verify 2>&1 | tee /dev/stderr)

TOTAL_MS=$(printf '%s\n' "$OUT" | awk -F= '/^SIMULATE_TOTAL_MS=/{print $2}' | tail -1)
FAILED=$(printf '%s\n' "$OUT" | awk -F= '/^SIMULATE_FAILED=/{print $2}' | tail -1)

[ -n "$TOTAL_MS" ] || { echo "FAIL: no SIMULATE_TOTAL_MS in output"; exit 1; }
[ "${FAILED:-0}" = "0" ] || { echo "FAIL: $FAILED tasks failed"; exit 1; }

if printf '%s' "$OUT" | grep -qiE 'EPIPE|EMFILE|Broken pipe|Too many open files'; then
    echo "FAIL: EPIPE/EMFILE detected in output"
    exit 1
fi

echo
if [ "$TOTAL_MS" -gt "$BUDGET_MS" ]; then
    echo "FAIL: ${TOTAL_MS}ms > budget ${BUDGET_MS}ms"
    exit 1
fi
echo "PASS: ${TOTAL_MS}ms ($(( BUDGET_MS - TOTAL_MS ))ms under ${BUDGET_MS}ms budget)"
