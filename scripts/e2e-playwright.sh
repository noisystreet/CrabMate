#!/usr/bin/env bash
# Playwright E2E 一键脚本
#
# 自动构建前端 → 启动后端 → 运行 Playwright 测试 → 停止后端
# 透传所有参数给 `npx playwright test`。
#
# 用法:
#   ./scripts/e2e-playwright.sh                          # 全部测试
#   ./scripts/e2e-playwright.sh specs/mock-tool-call-scenarios.spec.ts  # 指定文件
#   ./scripts/e2e-playwright.sh --headed                  # 有头模式调试
#   ./scripts/e2e-playwright.sh --ui                      # Playwright UI 模式
#
# 环境变量:
#   CRABMATE_PORT          后端绑定端口（默认 8080）
#   CRABMATE_BIN           crabmate 二进制路径（默认 cargo run）
#   SKIP_FRONTEND_BUILD    1 跳过前端构建
#   E2E_DIR                Playwright 项目目录（默认 e2e/）

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${CRABMATE_PORT:-8080}"
E2E_DIR="${E2E_DIR:-$ROOT/e2e}"
FRONTEND_DIR="$ROOT/frontend"
BACKEND_PID=""
EXIT_CODE=0

# ---------------------------------------------------------------------------
# 清理：停后端
# ---------------------------------------------------------------------------
cleanup() {
    if [[ -n "$BACKEND_PID" ]]; then
        echo ""
        echo ">>> 停止后端 (PID $BACKEND_PID)..."
        kill "$BACKEND_PID" 2>/dev/null || true
        wait "$BACKEND_PID" 2>/dev/null || true
        echo ">>> 后端已停止"
    fi
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# 1. 前端构建
# ---------------------------------------------------------------------------
if [[ -z "${SKIP_FRONTEND_BUILD:-}" ]]; then
    if [[ ! -f "$FRONTEND_DIR/dist/index.html" ]]; then
        echo ">>> 构建前端 (frontend/dist 不存在)..."
        (cd "$FRONTEND_DIR" && trunk build)
    else
        echo ">>> 前端已构建，跳过（设 SKIP_FRONTEND_BUILD=1 强制跳过）"
    fi
else
    echo ">>> SKIP_FRONTEND_BUILD=1，跳过前端构建"
fi

# ---------------------------------------------------------------------------
# 2. 端口检查
# ---------------------------------------------------------------------------
if command -v lsof &>/dev/null; then
    if lsof -ti ":$PORT" >/dev/null 2>&1; then
        echo "!!! 端口 $PORT 已被占用，请先释放或设置 CRABMATE_PORT"
        exit 1
    fi
else
    echo ">>> lsof 不可用，跳过端口检查"
fi

# ---------------------------------------------------------------------------
# 3. 启动后端
# ---------------------------------------------------------------------------
if [[ -n "${CRABMATE_BIN:-}" ]]; then
    echo ">>> 启动后端 (来自 CRABMATE_BIN=$CRABMATE_BIN)..."
    "$CRABMATE_BIN" serve --port "$PORT" &
else
    echo ">>> 启动后端 (cargo run -- serve --port $PORT)..."
    cargo run -- serve --port "$PORT" &
fi
BACKEND_PID=$!

echo ">>> 等待后端就绪 (:$PORT)..."
for i in $(seq 1 30); do
    if curl -s --connect-timeout 2 "http://127.0.0.1:$PORT/health" >/dev/null 2>&1; then
        echo ">>> 后端就绪"
        break
    fi
    if [[ $i -eq 30 ]]; then
        echo "!!! 后端启动超时"
        exit 1
    fi
    sleep 1
done

# ---------------------------------------------------------------------------
# 4. 运行 Playwright 测试
# ---------------------------------------------------------------------------
echo ">>> 运行 Playwright 测试..."
echo "    参数: $*"
echo ""

(
    cd "$E2E_DIR"
    no_proxy=127.0.0.1,localhost npx playwright test "$@"
) || EXIT_CODE=$?

if [[ $EXIT_CODE -eq 0 ]]; then
    echo ""
    echo ">>> 全部测试通过"
else
    echo ""
    echo "!!! 测试失败 (exit=$EXIT_CODE)"
fi

exit $EXIT_CODE
