#!/usr/bin/env bash
# Playwright 已迁至官方 Client 仓。本脚本仅转发到同级 crabmate-client。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CLIENT="${CRABMATE_CLIENT_DIR:-$ROOT/../crabmate-client}"
SCRIPT="$CLIENT/scripts/e2e-playwright.sh"
if [[ ! -x "$SCRIPT" ]]; then
  echo "错误: 未找到 Client Playwright 入口：$SCRIPT" >&2
  echo "      请克隆 https://github.com/noisystreet/crabmate-client 到同级目录，" >&2
  echo "      或设置 CRABMATE_CLIENT_DIR，然后在 Client 仓执行 ./scripts/e2e-playwright.sh" >&2
  exit 1
fi
exec "$SCRIPT" "$@"
