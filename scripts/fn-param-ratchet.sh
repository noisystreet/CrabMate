#!/usr/bin/env bash
# Rust 函数形参个数上限（lizard），入口为 **`scripts/fn_param_rust_metrics.py`**。
#
# 规则：单函数形参个数 ≤ 9（`MAX_PARAM_CAP`）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/fn_param_rust_metrics.py"
