#!/usr/bin/env bash
# `#[allow(unused*)]` 例外面棘轮（#1231）：模块级罩子不许新增，item 级豁免放行。
#
# `-D warnings` 抓不到被 allow 压住的告警（这正是 #1229 的根因），所以「allow 出现在
# 哪一层」单独门禁。实现与口径见 **`scripts/allow_exception_ratchet.py`**；
# 基线 `scripts/allow_exception_baseline.txt` 只许降不许升。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/scripts/allow_exception_ratchet.py"
