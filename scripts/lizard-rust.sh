#!/usr/bin/env bash
# 对 `src/` 下 Rust 代码做圈复杂度（CCN）扫描，使用 lizard（https://github.com/terryyin/lizard）。
# 业务 UI 复杂度门禁在 Client 仓 crabmate-client。
# 未安装时：pip install lizard
#
# 规则（与 scripts/lizard_rust_metrics.py 一致）：全仓每个函数 CCN ≤ 10。
# 额外参数原样传给 Python，例如：
#   bash scripts/lizard-rust.sh --list-above 8
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if ! python3 -c "import lizard" 2>/dev/null; then
  echo "lizard 未安装。请执行: pip install lizard" >&2
  echo "（或: uv pip install lizard；检查见 .pre-commit-config.yaml 中 lizard-rust）" >&2
  exit 1
fi
exec python3 "$ROOT/scripts/lizard_rust_metrics.py" "$@"
