#!/usr/bin/env bash
# Playwright E2E 一键脚本
#
# 启动后端 → 运行 Playwright 测试 → 停止后端。
# 业务 UI 须已构建：设 CM_WEB_STATIC_DIR，或同级 ../crabmate-client/frontend/dist。
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
#   CM_WEB_STATIC_DIR      UI dist（默认尝试 ../crabmate-client/frontend/dist）
#   E2E_DIR                Playwright 项目目录（默认 e2e/）

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PORT="${CRABMATE_PORT:-8080}"
E2E_DIR="${E2E_DIR:-$ROOT/e2e}"
BACKEND_PID=""
EXIT_CODE=0

if [[ -z "${CM_WEB_STATIC_DIR:-}" ]]; then
  if [[ -f "${ROOT}/../crabmate-client/frontend/dist/index.html" ]]; then
    export CM_WEB_STATIC_DIR="${ROOT}/../crabmate-client/frontend/dist"
  elif [[ -f "${ROOT}/crabmate-client/frontend/dist/index.html" ]]; then
    export CM_WEB_STATIC_DIR="${ROOT}/crabmate-client/frontend/dist"
  fi
fi

if [[ -z "${CM_WEB_STATIC_DIR:-}" || ! -f "${CM_WEB_STATIC_DIR}/index.html" ]]; then
  echo "错误: 未找到 UI dist。请先: cd ../crabmate-client && make frontend" >&2
  echo "      然后: export CM_WEB_STATIC_DIR=\"\$(cd ../crabmate-client && pwd)/frontend/dist\"" >&2
  exit 1
fi
echo ">>> UI dist: ${CM_WEB_STATIC_DIR}"

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
# 端口检查
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
# 启动后端
# ---------------------------------------------------------------------------
# E2E 测试时只输出 WARN/ERROR，避免 INFO 日志刷屏
export RUST_LOG="${CM_E2E_RUST_LOG:-warn}"
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
# 运行 Playwright 测试
# ---------------------------------------------------------------------------
echo ">>> 运行 Playwright 测试..."
echo "    参数: $*"
echo ""

(
    cd "$E2E_DIR"
    CRABMATE_PORT="$PORT" no_proxy=127.0.0.1,localhost npx playwright test "$@"
) || EXIT_CODE=$?

if [[ $EXIT_CODE -eq 0 ]]; then
    echo ""
    echo ">>> 全部测试通过"
else
    echo ""
    echo "!!! 测试失败 (exit=$EXIT_CODE)"
fi

exit $EXIT_CODE
