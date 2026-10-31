#!/usr/bin/env python3
"""`#[allow(unused*)]` 例外面棘轮（#1231）。

**为什么需要它**：`-D warnings` 永远抓不到被 `allow` 压住的告警——这正是 #1229 的根因。
`src/lib_server.rs` 曾给 10 个 `cm_*` 模块整体加 `#[allow(unused)]`，而 `unused` 这个
lint group 含 `dead_code`，于是模块内的死代码与多余 import 全部静默，clippy 一直绿。

所以「allow 出现在哪一层」必须单独门禁。本脚本只盯**模块级罩子**，不碰 item 级豁免：

| 形态 | 处置 |
|---|---|
| `#![allow(unused)]` / `#![allow(unused_imports)]` / `#![allow(dead_code)]` | 模块级罩子 → 计数冻结在基线 |
| `#[allow(unused)]`（outer，`unused` group） | 罩住整个 `mod` 的同一手法 → 零容忍 |
| item 级 `#[allow(dead_code)]` / `#[allow(unused_imports)]` | **允许**（#1229 认可的降级形态：显式、就近、可带理由） |
| `#![cfg_attr(not(feature = "x"), allow(dead_code))]` | **允许**（feature 对齐的首选写法） |

基线 `scripts/allow_exception_baseline.txt` **只许降不许升**：新增一处模块级罩子即失败；
某文件真正清干净后，请用 `--print-baseline` 下调，把收益锁住。

用法：
    python3 scripts/allow_exception_ratchet.py              # 校验（pre-commit 用）
    python3 scripts/allow_exception_ratchet.py --print-baseline
"""

from __future__ import annotations

import argparse
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SRC = ROOT / "src"
BASELINE_FILE = ROOT / "scripts" / "allow_exception_baseline.txt"

# 模块级罩子关注的 lint。`unused` group 含 `dead_code`，是 #1229 的根因。
MODULE_LINTS = ("unused", "unused_imports", "dead_code")

INNER_ALLOW = re.compile(r"^\s*#!\[\s*allow\s*\(([^)]*)\)\s*\]")
OUTER_ALLOW = re.compile(r"^\s*#\[\s*allow\s*\(([^)]*)\)\s*\]")


def parse_lints(group: str) -> set[str]:
    """`allow(a, clippy::b)` → {"a", "b"}；工具前缀对判定无意义，去掉。"""
    names = set()
    for raw in group.split(","):
        name = raw.strip()
        if name:
            names.add(name.rsplit("::", 1)[-1])
    return names


def scan() -> collections.Counter[tuple[str, str]]:
    """返回 {(相对路径, lint): 处数}，只统计模块级罩子。"""
    hits: collections.Counter[tuple[str, str]] = collections.Counter()
    for path in sorted(SRC.rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        text = path.read_text(encoding="utf-8", errors="replace")
        for line in text.splitlines():
            # 注释里出现的形态不算（本仓注释经常引用这些属性作为说明）。
            if line.lstrip().startswith("//"):
                continue
            inner = INNER_ALLOW.match(line)
            if inner:
                for lint in parse_lints(inner.group(1)) & set(MODULE_LINTS):
                    hits[(rel, lint)] += 1
                continue
            outer = OUTER_ALLOW.match(line)
            # item 级 `dead_code` / `unused_imports` 是认可的降级形态；只有 `unused`
            # group 会被拿来罩整个 mod，零容忍。
            if outer and "unused" in parse_lints(outer.group(1)):
                hits[(rel, "unused")] += 1
    return hits


def load_baseline() -> dict[tuple[str, str], int]:
    if not BASELINE_FILE.exists():
        return {}
    entries: dict[tuple[str, str], int] = {}
    for raw in BASELINE_FILE.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        path, lint, count = line.split("\t")
        entries[(path, lint)] = int(count)
    return entries


def render_baseline(hits: collections.Counter[tuple[str, str]]) -> str:
    out = [
        "# 模块级 `allow(unused* / dead_code)` 罩子的基线（#1231）。",
        "#",
        "# 由 `python3 scripts/allow_exception_ratchet.py --print-baseline` 生成。",
        "#",
        "# 只许降不许升：清掉某处罩子后请下调对应计数，归零的条目直接删除。",
        "# 新增代码不要加模块级 allow —— 用 item 级 `#[allow(dead_code)] // 理由`，",
        "# 或 `#[cfg_attr(not(feature = \"x\"), allow(dead_code))]` 做 feature 对齐。",
        "#",
        "# 格式：相对路径<TAB>lint<TAB>处数",
    ]
    for (path, lint), count in sorted(hits.items()):
        out.append(f"{path}\t{lint}\t{count}")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="allow 例外面棘轮（模块级罩子）")
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="打印当前扫描结果的基线文本（人工确认后写入 baseline 文件）",
    )
    args = parser.parse_args()

    hits = scan()
    if args.print_baseline:
        sys.stdout.write(render_baseline(hits))
        return 0

    baseline = load_baseline()
    total = sum(hits.values())
    limit = sum(baseline.values())

    regressions: list[str] = []
    for key, count in sorted(hits.items()):
        allowed = baseline.get(key, 0)
        if count > allowed:
            where = "新增" if key not in baseline else f"超出基线（{allowed} → {count}）"
            regressions.append(f"{key[0]}\t{key[1]}\t{where}")

    if regressions:
        sys.stderr.write(
            "出现新增的模块级 `allow(unused* / dead_code)` 罩子（#1231）。\n"
            "模块级 allow 会让死代码与多余 import 重新隐身（#1229 的根因）。请改为：\n"
            "  · item 级 `#[allow(dead_code)] // 为什么`（就近、显式、可审）\n"
            "  · `#[cfg_attr(not(feature = \"x\"), allow(dead_code))]`（feature 对齐）\n"
            "  · 直接删死代码\n"
            "确有必要（如再导出面），用 `--print-baseline` 更新基线并在 PR 里说明原因。\n\n"
        )
        for line in regressions:
            sys.stderr.write(f"  {line}\n")
        sys.stderr.write(f"\n当前合计 {total} 处，基线上限 {limit} 处。\n")
        return 1

    stale = [key for key in baseline if hits.get(key, 0) < baseline[key]]
    print(
        f"模块级 allow 罩子：{total} 处 / {len(hits)} 个 (文件, lint) 组合；基线上限 {limit}。"
    )
    if stale:
        print(
            f"提示：{len(stale)} 条基线已高于实际值，请用 --print-baseline 下调以锁住收益。"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
