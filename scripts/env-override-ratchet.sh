#!/usr/bin/env bash
# 环境覆盖守卫（#1353）：`part_<数字>` 函数名只许降不许升；`cm_config/env_overrides/`
# 的 `CM_*` 键集合必须与 `fixtures/env_override_keys.txt` 严格一致且不重复。
# 口径与「为什么需要键 golden」见 **`scripts/env_override_guard.py`**。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
exec python3 "$ROOT/scripts/env_override_guard.py" "$@"
