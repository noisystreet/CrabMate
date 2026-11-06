#!/usr/bin/env bash
# S4 验收口径：上层对 `cm_internal` 的引用面（C1 长路径 + C2 根上短名）。
#
# ## 为什么名单必须自动派生
#
# 手工维护名单会静默漂移，且漂移方向往往是**虚假恶化**：S4-P1b 把 `mcp` 合并成顶层模块后，
# 名单里仍留着 `mcp`，于是 `crate::mcp` 的 45 处引用被算成「对 cm_internal 的耦合」，
# C2 反而从 314 虚涨到 329 —— 比虚假改善更容易误导人（看上去像重构把事情做坏了）。
# 故此处两名单均从源码派生：
#   · C1 名单 ← `src/cm_internal/mod.rs` 的 `pub mod X;`
#   · C2 名单 ← `src/lib_server.rs` 的 `pub(crate) use crate::cm_internal::{…}`
# 桶搬迁时两者自动跟随，无需人工对表（#1300 §1.1 局限 3 的要求）。
#
# ## 口径的三个已知边界（终态验收不能只依赖此数）
#
#   1. 统计单位是**行**，不是符号引用次数；同一行出现两次只计一次。
#   2. 纯注释行（`//` / `//!` / `///` 开头）被剔除 —— 文档里写的路径不是引用。
#      行尾注释（`code // …`）无法可靠区分，仍计入；描述性文字请避免写出完整包路径。
#   3. grouped use（`use crate::{a, b}`）不被 `crate::a` 匹配，可能**低估**；
#      终态还需 P3 的白名单禁边脚本兜底。
#
# ## 用法
#
#   bash scripts/check-cm-internal-refs.sh                  # 打印 C1 / C2 / 合计
#   bash scripts/check-cm-internal-refs.sh --check          # 高于基线则 exit 1
#   bash scripts/check-cm-internal-refs.sh --print-baseline  # 生成基线文件内容
#
# 只读；不修改工作区。

set -uo pipefail

ROOT=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
INTERNAL_MOD="$ROOT/src/cm_internal/mod.rs"
LIB_SERVER="$ROOT/src/lib_server.rs"
BASELINE="$ROOT/scripts/cm_internal_refs_baseline.txt"

fail() { printf '错误：%s\n' "$*" >&2; exit 1; }

[ -f "$INTERNAL_MOD" ] || fail "找不到 $INTERNAL_MOD"
[ -f "$LIB_SERVER" ] || fail "找不到 $LIB_SERVER"

# C1 名单：cm_internal 的真子项（`pub mod X;`）。转发项（`pub use crate::cm_tools::…`）不在其中。
SUB=$(grep -oE '^pub mod [a-z_0-9]+;' "$INTERNAL_MOD" 2>/dev/null \
        | sed -E 's/^pub mod ([a-z_0-9]+);/\1/' | tr '\n' ' ' || true)

# C2 名单：根上从 cm_internal 再导出的短名。
SHORT=$(sed -n '/^pub(crate) use crate::cm_internal::{/,/^};/p' "$LIB_SERVER" 2>/dev/null \
          | sed '1d;$d' | tr ',' '\n' | tr -d ' ' | grep -E '^[a-z_0-9]+$' | tr '\n' ' ' || true)

# 防静默退化：解析为空说明源文件结构变了，必须失败而不是给出 0（0 会被误读成「已达标」）。
[ -n "${SUB// /}" ] || fail "未能从 cm_internal/mod.rs 解析出 pub mod 列表（文件结构变了？）"
[ -n "${SHORT// /}" ] || fail "未能从 lib_server.rs 解析出短名列表（文件结构变了？）"

# 一次扫描 → 剔除 cm_internal 自身与纯注释行 → 按名单归并计数。
measure() {
  local prefix=$1 names=$2
  grep -rnE "${prefix}[a-z_0-9]+" "$ROOT/src" --include='*.rs' 2>/dev/null \
    | grep -v '/src/cm_internal/' \
    | grep -vE ':[0-9]+:[[:space:]]*//' \
    | grep -oE "${prefix}[a-z_0-9]+" \
    | sed -E 's/.*:://' \
    | awk -v names="$names" '
        BEGIN { n = split(names, a, " "); for (i = 1; i <= n; i++) keep[a[i]] = 1 }
        ($0 in keep) { c++ }
        END { print c + 0 }
      '
}

C1=$(measure 'crate::cm_internal::' "$SUB")
C2=$(measure 'crate::' "$SHORT")
TOTAL=$((C1 + C2))
n_sub=$(printf '%s' "$SUB" | wc -w)
n_short=$(printf '%s' "$SHORT" | wc -w)

case "${1:-}" in
  --print-baseline)
    printf '# 由 bash scripts/check-cm-internal-refs.sh --print-baseline 生成。\n'
    printf '# 只许降不许升；名单自动派生，搬迁子模块后无需手工改本文件。\n'
    printf 'C1\t%s\nC2\t%s\n' "$C1" "$C2"
    ;;
  --check)
    [ -f "$BASELINE" ] || fail "基线文件不存在：$BASELINE（先跑 --print-baseline）"
    base_c1=$(awk -F'\t' '$1 == "C1" { print $2 }' "$BASELINE")
    base_c2=$(awk -F'\t' '$1 == "C2" { print $2 }' "$BASELINE")
    [ -n "$base_c1" ] && [ -n "$base_c2" ] || fail "基线格式不对（缺 C1/C2 行）：$BASELINE"
    printf 'C1=%s（基线 %s） C2=%s（基线 %s） 合计=%s（基线 %s）\n' \
      "$C1" "$base_c1" "$C2" "$base_c2" "$TOTAL" "$((base_c1 + base_c2))"
    rc=0
    if [ "$C1" -gt "$base_c1" ]; then
      printf '错误：C1 高于基线 %s 处 —— 新增了直接引用 cm_internal 的代码？\n' "$((C1 - base_c1))" >&2
      rc=1
    fi
    if [ "$C2" -gt "$base_c2" ]; then
      printf '错误：C2 高于基线 %s 处 —— 根上新增了经 cm_internal 的短名？\n' "$((C2 - base_c2))" >&2
      rc=1
    fi
    if [ "$C1" -lt "$base_c1" ] || [ "$C2" -lt "$base_c2" ]; then
      printf '提示：实际低于基线，用 --print-baseline 下调以锁住收益。\n'
    fi
    exit "$rc"
    ;;
  '')
    printf '== S4 验收口径：cm_internal 引用面 ==\n'
    printf 'C1 长路径（crate::cm_internal::<子项>）  : %s\n' "$C1"
    printf 'C2 根上短名（crate::<短名>）             : %s\n' "$C2"
    printf '合计                                     : %s\n' "$TOTAL"
    printf '名单：mod.rs 的 pub mod（%s 项）、lib_server.rs 的 re-export（%s 项）\n' "$n_sub" "$n_short"
    ;;
  *)
    fail "未知参数：$1（可用：--check / --print-baseline）"
    ;;
esac
