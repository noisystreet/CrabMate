#!/usr/bin/env bash
# Rust 函数 nloc + 源文件物理行数上限（lizard + 按行计数），入口 **`scripts/fn_nloc_rust_metrics.py`**。
#
# 规则：最大 nloc ≤ 200；单文件最大行数 ≤ 920（脚本内 `MAX_FILE_LINES_CAP`）。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/fn_nloc_rust_metrics.py"
