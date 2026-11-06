#!/usr/bin/env bash
# md 行长棘轮：markdown 单行超长行只许降不许升（默认 160 显示列，中文约 80 字）。
# 口径（宽字符按 2 列、表格行/围栏代码块豁免）与基线生成方式见
# **`scripts/md_line_length_ratchet.py`**；基线 `scripts/md_line_length_baseline.txt` 只许降不许升。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/scripts/md_line_length_ratchet.py" "$@"
