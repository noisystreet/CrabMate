#!/usr/bin/env bash
# Victauri E2E 一键脚本
# 用法: ./scripts/victauri-e2e.sh [test_name|all]
#   无参数: 运行全部测试
#   victauri_e2e: 只运行 victauri_e2e 测试
#   real_llm: 只运行真实 LLM 测试 (需 REAL_LLM_E2E=1)

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_DIR="$ROOT/desktop-tauri/src-tauri"
BACKEND_BIN="$ROOT/target/debug/crabmate"

TEST="${1:-all}"
REAL_LLM="${REAL_LLM_E2E:-}"

echo "=== Victauri E2E ==="
echo "  test: $TEST"
echo "  real_llm: ${REAL_LLM:-no}"

# ── Phase 1: Build ──────────────────────────────────────────
echo ""
echo ">>> Building app + tests ..."
cd "$TAURI_DIR"
cargo build 2>&1 | tail -2
echo "   done."

# ── Phase 2: Kill old processes ─────────────────────────────
echo ""
echo ">>> Killing old processes ..."
pkill -9 -f crabmate-desktop 2>/dev/null || true
pkill -9 -f "crabmate serve" 2>/dev/null || true
sleep 2
rm -rf /tmp/victauri/*/
echo "   done."

# ── Phase 3: Start app in background ────────────────────────
echo ""
echo ">>> Starting app ..."
cd "$TAURI_DIR"
CM_E2E_FIXTURES=1 CM_DESKTOP_BACKEND_BIN="$BACKEND_BIN" ./target/debug/crabmate-desktop &
APP_PID=$!
echo "   PID: $APP_PID"

# ── Phase 4: Wait for port 7373 AND main window ────────────
echo ">>> Waiting for Victauri server (port 7373) ..."
for i in $(seq 1 30); do
    if ss -tlnp 2>/dev/null | grep -q 7373; then
        echo "   port ready after ${i}s"
        break
    fi
    if [ "$i" -eq 30 ]; then
        echo "   FAILED: app did not start within 30s"
        kill $APP_PID 2>/dev/null
        exit 1
    fi
    sleep 1
done

# Wait for main window to appear (backend startup + splash → main window)
echo ">>> Waiting for main window (backend startup + page load) ..."
sleep 15

# ── Phase 5: Clean stale discovery dirs ────────────────────
for d in /tmp/victauri/*/port; do
    port=$(cat "$d" 2>/dev/null)
    dir=$(dirname "$d")
    if [ "$port" != "7373" ]; then
        rm -rf "$dir"
    fi
done

# ── Phase 6: Run tests ──────────────────────────────────────
echo ""
echo ">>> Running tests ..."
cd "$TAURI_DIR"
export VICTAURI_E2E=1
export CM_E2E_FIXTURES=1
unset http_proxy https_proxy HTTP_PROXY HTTPS_PROXY

# Use pre-compiled binary to avoid recompilation killing the app
find_test_bin() {
    local name="$1"
    find target/debug/deps -name "${name}-*" -not -name '*.d' 2>/dev/null | head -1
}

if [ "$TEST" = "real_llm" ]; then
    export REAL_LLM_E2E=1
    BIN=$(find_test_bin victauri_real_llm)
    [ -n "$BIN" ] && exec "$BIN" || cargo test --test victauri_real_llm -- --nocapture
elif [ "$TEST" = "all" ]; then
    # cargo test --no-run first, then run binaries
    cargo test --no-fail-fast --no-run 2>/dev/null || true
    for name in victauri_e2e victauri_session_crud victauri_prefs_theme victauri_status_bar \
                victauri_settings victauri_settings2 victauri_keyboard victauri_conversation \
                victauri_user_data victauri_pagination victauri_visible_messages \
                victauri_sse_stub victauri_sse_more victauri_ide_layout \
                victauri_two_turn victauri_turn_layout victauri_real_llm; do
        BIN=$(find_test_bin "$name")
        if [ -n "$BIN" ]; then
            echo ">>> $name"
            "$BIN" || EXIT=$?
        fi
    done
else
    BIN=$(find_test_bin "$TEST")
    if [ -n "$BIN" ]; then
        "$BIN"
    else
        cargo test --test "$TEST" -- --nocapture
    fi
fi
EXIT=$?

# ── Phase 7: Cleanup ────────────────────────────────────────
echo ""
echo ">>> Stopping app ..."
kill $APP_PID 2>/dev/null
pkill -f crabmate-desktop 2>/dev/null || true
wait 2>/dev/null || true

echo ""
if [ $EXIT -eq 0 ]; then
    echo "=== ALL PASSED ==="
else
    echo "=== FAILED (exit code $EXIT) ==="
fi
exit $EXIT
