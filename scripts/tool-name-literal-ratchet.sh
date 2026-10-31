#!/usr/bin/env bash
# 内建工具名字面量 ratchet（#1155）：判定形态（`==` / `!=` / `=>` / `|`）下不允许新增
# 工具名字面量，一律用 `crate::cm_names::*`。实现与口径说明见
# **`scripts/tool_name_literal_ratchet.py`**；基线 `scripts/tool_name_literal_baseline.txt` 只许降不许升。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/scripts/tool_name_literal_ratchet.py"
