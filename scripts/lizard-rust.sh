#!/usr/bin/env bash
# 对 `src/`、`crates/` 与 `frontend/src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 未安装时：pip install lizard
#
# 规则（与 scripts/lizard_rust_metrics.py 一致）：单函数 CCN 不得超过 15。
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py"
