#!/usr/bin/env bash
# `crate::tools` 别名守卫（#1522）。
#
# 背景：`src/lib.rs` 曾用 `pub(crate) use crate::cm_tools::{… tools …}` 把
# `crate::tools` 做成 `cm_tools::tools` 的第二名字，导致同一模块有两个可达路径
# （双名比 112 : 6，#1510 P1 同类问题）。现在别名已删除，正名是
# `crate::cm_tools::tools`。
#
# 与 `unwrap` / `tool-name-literal` 那些 ratchet 不同：这里**没有存量可留**，
# 收敛后就是 0，所以本脚本是硬 0 断言，不走基线机制（基线是给「还有一堆存量、
# 只能先钉住不涨」的场景用的）。
#
# 顺带一提：`crate::tools` 是 `pub(crate)`，删它不改变 crate 外可见性；
# 承诺面 `crabmate::{build_tools, ToolsBuildOptions, …}` 由 `tests/lib_smoke.rs`
# 从外部视角编译验证，与本守卫互不替代。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# 三种形态都要挡——只挡第一种等于没挡：换个写法就能把双名长回来。
#   ① 直接引用旧路径    `use crate::tools::…` / `crate::tools::f()`
#   ② `as tools` 重建   `pub(crate) use crate::cm_tools::tools as tools;`
#      （这条不含 `crate::tools` 字面量，① 的正则抓不到，必须单独匹配）
#   ③ 根别名清单塞回    `src/lib.rs` 的 `pub(crate) use crate::cm_tools::{… tools …}`
# ③ 是跨行块，grep 逐行匹配抓不到，故用 `-A5` 取块内后续行再筛。
# 注意子模块里 `use crate::cm_tools::tools;` 是**正常**写法（只引入本地短名，
# 不制造 `crate::tools` 第二路径），所以 ③ 只针对 `src/lib.rs` 的 `pub(crate) use` 块。
fail=0

# `\b` 让 `crate::tools::x` 与 `use crate::tools;` 都命中，而 `crate::tools_x` 不命中。
refs="$(grep -rn --include='*.rs' -E 'crate::tools\b' src || true)"
if [[ -n "$refs" ]]; then
  echo "FORBIDDEN[refs]: 已删除的 \`crate::tools\` 别名不得再被引用（#1522）；请直连 \`crate::cm_tools::tools\`：" >&2
  echo "$refs" >&2
  fail=1
fi

rebuild="$(grep -rn --include='*.rs' -E 'cm_tools::tools\b[^;]*\bas\b[^;]*\btools\b' src || true)"
if [[ -n "$rebuild" ]]; then
  echo "FORBIDDEN[rebuild]: 不得用 \`as tools\` 再造第二路径（#1522）；正名就是 \`crate::cm_tools::tools\`：" >&2
  echo "$rebuild" >&2
  fail=1
fi

root_alias="$(grep -n -A5 -E 'pub\(crate\) use crate::cm_tools::\{' src/lib.rs 2>/dev/null | grep -E '\btools\b' || true)"
if [[ -n "$root_alias" ]]; then
  echo "FORBIDDEN[root_alias]: \`src/lib.rs\` 的 \`pub(crate) use crate::cm_tools::{…}\` 清单不得再含 \`tools\`（#1522）：" >&2
  echo "$root_alias" >&2
  fail=1
fi

if [[ "$fail" -ne 0 ]]; then
  exit 1
fi

echo "tools alias guard OK: 无 crate::tools 引用 / 无 as tools 重建 / lib.rs 别名清单无 tools"
