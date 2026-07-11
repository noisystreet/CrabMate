#!/usr/bin/env bash
# Victauri E2E 一键脚本（Linux headless：默认 exec xvfb-run 重入，窗口不落到本机桌面）
#
# 用法: ./scripts/victauri-e2e.sh [test_name|all|real_llm]
#
# 环境变量:
#   VICTAURI_USE_XVFB     1（默认）| 0 | auto
#                         1: exec xvfb-run 重跑本脚本，窗口仅在虚拟显示；0: 本机桌面调试
#   VICTAURI_PORT         Victauri MCP 端口（默认 7373）
#   VICTAURI_START_TIMEOUT  等待 /health 秒数（默认 90）
#   VICTAURI_MAIN_WINDOW_WAIT  health 后主窗口额外等待秒数（默认 15）
#   VICTAURI_E2E_LOG      桌面应用日志路径（默认 /tmp/crabmate-desktop-e2e.log）
#   CM_E2E_FIXTURES       默认 1
#   CM_DESKTOP_BACKEND_BIN  后端 crabmate 二进制路径
#   REAL_LLM_E2E          仅 real_llm 套件需要
#   VICTAURI_INSIDE_XVFB  内部：已由 xvfb-run 重入，勿手动设置

set -euo pipefail

# 保留原始参数（exec xvfb-run 重入时传递）
E2E_SCRIPT_ARGS=("$@")

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TAURI_DIR="$ROOT/desktop-tauri/src-tauri"
DESKTOP_ROOT="$ROOT/desktop-tauri"
BACKEND_BIN="${CM_DESKTOP_BACKEND_BIN:-$ROOT/target/debug/crabmate}"
DESKTOP_BIN="$TAURI_DIR/target/debug/crabmate-desktop"

TEST="${1:-all}"
REAL_LLM="${REAL_LLM_E2E:-}"
VICTAURI_PORT="${VICTAURI_PORT:-7373}"
VICTAURI_START_TIMEOUT="${VICTAURI_START_TIMEOUT:-120}"
VICTAURI_MAIN_WINDOW_WAIT="${VICTAURI_MAIN_WINDOW_WAIT:-20}"
VICTAURI_E2E_LOG="${VICTAURI_E2E_LOG:-/tmp/crabmate-desktop-e2e.log}"

should_use_xvfb() {
    case "${VICTAURI_USE_XVFB:-1}" in
        1 | true | yes) return 0 ;;
        0 | false | no) return 1 ;;
        auto)
            if [ -z "${DISPLAY:-}" ] || [ "${CI:-}" = "true" ] || [ "${GITHUB_ACTIONS:-}" = "true" ]; then
                return 0
            fi
            return 1
            ;;
        *)
            echo "unknown VICTAURI_USE_XVFB=${VICTAURI_USE_XVFB} (use 1|0|auto)" >&2
            exit 1
            ;;
    esac
}

# 在 Wayland 桌面上：必须 exec xvfb-run 重跑整个脚本，否则 Tauri 仍会弹到本机
maybe_reexec_under_xvfb() {
    if ! should_use_xvfb; then
        return 0
    fi
    if [ -n "${VICTAURI_INSIDE_XVFB:-}" ]; then
        return 0
    fi
    if ! command -v xvfb-run >/dev/null 2>&1; then
        echo "xvfb-run not found; install package xvfb (e.g. apt install xvfb)" >&2
        exit 1
    fi
    echo ">>> Relaunching under xvfb-run (E2E windows are also hidden via CM_E2E_FIXTURES) ..." >&2
    exec env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u GDK_BACKEND \
        WINIT_UNIX_BACKEND=x11 \
        LIBGL_ALWAYS_SOFTWARE=1 \
        xvfb-run --auto-servernum --server-args='-screen 0 1280x720x24' \
        env VICTAURI_INSIDE_XVFB=1 \
        WINIT_UNIX_BACKEND=x11 \
        LIBGL_ALWAYS_SOFTWARE=1 \
        CM_E2E_FIXTURES="${CM_E2E_FIXTURES:-1}" \
        CM_DESKTOP_BACKEND_BIN="${CM_DESKTOP_BACKEND_BIN:-$BACKEND_BIN}" \
        REAL_LLM_E2E="${REAL_LLM_E2E:-}" \
        VICTAURI_PORT="${VICTAURI_PORT}" \
        VICTAURI_START_TIMEOUT="${VICTAURI_START_TIMEOUT}" \
        VICTAURI_MAIN_WINDOW_WAIT="${VICTAURI_MAIN_WINDOW_WAIT}" \
        VICTAURI_E2E_LOG="${VICTAURI_E2E_LOG}" \
        bash "$0" "${E2E_SCRIPT_ARGS[@]}"
}

maybe_reexec_under_xvfb

# 启动桌面端：剥离 Wayland；CM_E2E_FIXTURES 令 splash/main 为 visible(false)
start_desktop_background() {
    if [ -n "${VICTAURI_INSIDE_XVFB:-}" ]; then
        echo "   display: ${DISPLAY:-<xvfb>} + CM_E2E_FIXTURES hidden windows" >&2
    else
        echo "   display: ${DISPLAY:-<unset>} + CM_E2E_FIXTURES hidden windows" >&2
    fi
    env -u WAYLAND_DISPLAY -u WAYLAND_SOCKET -u GDK_BACKEND \
        WINIT_UNIX_BACKEND=x11 \
        LIBGL_ALWAYS_SOFTWARE=1 \
        CM_E2E_FIXTURES=1 \
        CM_DESKTOP_BACKEND_BIN="$BACKEND_BIN" \
        "$DESKTOP_BIN" >>"$VICTAURI_E2E_LOG" 2>&1 &
    echo $!
}

wait_for_victauri_health() {
    local pid="$1"
    for i in $(seq 1 "$VICTAURI_START_TIMEOUT"); do
        if curl -sf "http://127.0.0.1:${VICTAURI_PORT}/health" >/dev/null 2>&1; then
            echo "   Victauri /health OK after ${i}s"
            return 0
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            if pgrep -f "$DESKTOP_BIN" >/dev/null 2>&1; then
                sleep 1
                continue
            fi
            echo "   FAILED: desktop process exited before Victauri ready"
            echo "   --- last 40 lines of ${VICTAURI_E2E_LOG} ---"
            tail -40 "$VICTAURI_E2E_LOG" 2>/dev/null || true
            return 1
        fi
        if [ "$i" -eq "$VICTAURI_START_TIMEOUT" ]; then
            echo "   FAILED: Victauri not healthy within ${VICTAURI_START_TIMEOUT}s"
            echo "   --- last 40 lines of ${VICTAURI_E2E_LOG} ---"
            tail -40 "$VICTAURI_E2E_LOG" 2>/dev/null || true
            return 1
        fi
        sleep 1
    done
}

echo "=== Victauri E2E ==="
echo "  test: $TEST"
echo "  real_llm: ${REAL_LLM:-no}"
echo "  xvfb: ${VICTAURI_USE_XVFB:-1}$([ -n "${VICTAURI_INSIDE_XVFB:-}" ] && echo ' (inside xvfb-run)')"
echo "  port: $VICTAURI_PORT"

# ── Phase 1: Build ──────────────────────────────────────────
echo ""
echo ">>> Building backend + frontend + desktop ..."
cd "$ROOT"
if [ ! -x "$BACKEND_BIN" ]; then
    cargo build -p crabmate
fi

if [ ! -f "$ROOT/frontend/dist/index.html" ]; then
    if ! command -v trunk >/dev/null 2>&1; then
        echo "frontend/dist missing and trunk not installed" >&2
        echo "  cd frontend && trunk build" >&2
        exit 1
    fi
    (cd "$ROOT/frontend" && trunk build)
fi

CM_DESKTOP_BACKEND_BIN="$BACKEND_BIN" bash "$DESKTOP_ROOT/scripts/prepare-sidecar.sh"

cd "$TAURI_DIR"
cargo build --tests 2>&1 | tail -3
echo "   done."

# ── Phase 2: Kill old processes ─────────────────────────────
echo ""
echo ">>> Killing old processes ..."
pkill -9 -f 'src-tauri/target/debug/crabmate-desktop' 2>/dev/null || true
pkill -9 -f 'target/debug/crabmate-desktop' 2>/dev/null || true
pkill -9 -f "crabmate serve" 2>/dev/null || true
sleep 2
rm -rf /tmp/victauri/*/
echo "   done."

# ── Phase 3: Start app in background ────────────────────────
echo ""
echo ">>> Starting app (Tauri + WebView required for Victauri; xvfb keeps it off-screen) ..."
cd "$TAURI_DIR"
: >"$VICTAURI_E2E_LOG"
APP_PID=$(start_desktop_background) || exit 1
echo "   PID: $APP_PID"
echo "   log: $VICTAURI_E2E_LOG"

# ── Phase 4: Wait for Victauri health + main window ─────────
echo ">>> Waiting for Victauri server (http://127.0.0.1:${VICTAURI_PORT}/health) ..."
wait_for_victauri_health "$APP_PID"

echo ">>> Waiting for main window (backend startup + page load, ${VICTAURI_MAIN_WINDOW_WAIT}s) ..."
sleep "$VICTAURI_MAIN_WINDOW_WAIT"

# ── Phase 5: Clean stale discovery dirs ────────────────────
for d in /tmp/victauri/*/port; do
    [ -e "$d" ] || continue
    port=$(cat "$d" 2>/dev/null || true)
    dir=$(dirname "$d")
    if [ "$port" != "$VICTAURI_PORT" ]; then
        rm -rf "$dir"
    fi
done

# ── Phase 6: Run tests ──────────────────────────────────────
echo ""
echo ">>> Running tests ..."
cd "$TAURI_DIR"
export VICTAURI_E2E=1
export CM_E2E_FIXTURES=1
export VICTAURI_PORT
unset http_proxy https_proxy HTTP_PROXY HTTPS_PROXY

EXIT=0

find_test_bin() {
    local name="$1"
    find target/debug/deps -name "${name}-*" -not -name '*.d' 2>/dev/null | head -1
}

if [ "$TEST" = "real_llm" ]; then
    export REAL_LLM_E2E=1
    BIN=$(find_test_bin victauri_real_llm)
    if [ -n "$BIN" ]; then
        "$BIN" || EXIT=$?
    else
        cargo test --test victauri_real_llm -- --nocapture || EXIT=$?
    fi
elif [ "$TEST" = "all" ]; then
    cargo test --no-fail-fast --no-run 2>/dev/null || true
    for name in victauri_e2e victauri_session_crud victauri_prefs_theme victauri_status_bar \
        victauri_settings victauri_settings2 victauri_keyboard victauri_conversation \
        victauri_user_data victauri_pagination victauri_visible_messages \
        victauri_sse_stub victauri_sse_more victauri_scroll_send victauri_ide_layout \
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
        "$BIN" || EXIT=$?
    else
        cargo test --test "$TEST" -- --nocapture || EXIT=$?
    fi
fi

# ── Phase 7: Cleanup ────────────────────────────────────────
echo ""
echo ">>> Stopping app ..."
kill "$APP_PID" 2>/dev/null || true
pkill -f 'target/debug/crabmate-desktop' 2>/dev/null || true
wait 2>/dev/null || true

echo ""
if [ "$EXIT" -eq 0 ]; then
    echo "=== ALL PASSED ==="
else
    echo "=== FAILED (exit code $EXIT) ==="
fi
exit "$EXIT"
