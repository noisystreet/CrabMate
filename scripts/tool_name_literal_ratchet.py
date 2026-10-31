#!/usr/bin/env python3
"""内建工具名字面量 ratchet（#1155 的防回退闸门）。

**为什么不能只数个数**：`src/` 里有大量与工具名同名的字符串——JSON 字段名
（`v.get("event")`）、子进程命令名（`Command::new("git")`）、`source_role` 标签、
`self_config_info` 的小节 id、正文匹配针。朴素 grep 会把它们算成「工具名字面量」，
逼着后来者去"修" `dir.join("git")` 这种无关代码。

因此本脚本只统计**判定形态**：字符串字面量紧邻 `==` / `!=` / `=>` / `|`，
即「拿工具名做比较、做 match 分支、做 or-pattern」。`#1155` 要消除的「拼错静默失效」
恰好只发生在这类位置——写错的分支永远不命中，没有任何测试会报错。

基线 `scripts/tool_name_literal_baseline.txt` 只许降不许升：新增一处判定形态字面量
（或某个 (文件, 名字) 组合的处数上升）即失败。

用法：
    python3 scripts/tool_name_literal_ratchet.py              # 校验（pre-commit 用）
    python3 scripts/tool_name_literal_ratchet.py --print-baseline   # 打印当前基线
"""

from __future__ import annotations

import argparse
import collections
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
NAMES_FILE = ROOT / "src" / "cm_names.rs"
BASELINE_FILE = ROOT / "scripts" / "tool_name_literal_baseline.txt"

# 只扫生产代码：常量自身、工具注册表分片、测试目录/文件。
SKIP_REL_SUFFIXES = ("src/cm_names.rs",)
SKIP_PATH_PARTS = ("/tests/", "/benches/", "/examples/")
CFG_TEST_MARKER = "#[cfg(test)]"

# 判定形态：字面量紧邻比较 / match 分支 / or-pattern。
# 每个模式都要求「名字」出现在比较或分支位置上，而不是任意字符串。
PATTERNS = (
    re.compile(r'(?:==|!=)\s*"([A-Za-z0-9_]+)"'),
    re.compile(r'"([A-Za-z0-9_]+)"\s*(?:==|!=)'),
    re.compile(r'"([A-Za-z0-9_]+)"\s*=>'),
    re.compile(r'"([A-Za-z0-9_]+)"\s*\|'),
    re.compile(r'\|\s*"([A-Za-z0-9_]+)"'),
)


def load_tool_names() -> set[str]:
    text = NAMES_FILE.read_text(encoding="utf-8")
    return set(re.findall(r'pub const [A-Z0-9_]+: &str = "([a-z0-9_]+)";', text))


def code_part(line: str) -> str:
    """去掉行尾注释。

    粗糙（不认字符串里的 `//`），对本文统计的短字面量足够；宁可少算也不误报，
    因为基线允许存在条目，误报的代价是有人被迫改无关代码。
    """
    return line.split("//", 1)[0]


def production_lines(path: pathlib.Path) -> list[str]:
    lines = path.read_text(encoding="utf-8", errors="replace").splitlines()
    for i, line in enumerate(lines):
        if line.strip() == CFG_TEST_MARKER:
            return lines[:i]
    return lines


def scan() -> collections.Counter[tuple[str, str]]:
    """返回 {(相对路径, 工具名): 处数}。"""
    tool_names = load_tool_names()
    hits: collections.Counter[tuple[str, str]] = collections.Counter()
    for path in sorted((ROOT / "src").rglob("*.rs")):
        rel = path.relative_to(ROOT).as_posix()
        if rel.endswith(SKIP_REL_SUFFIXES):
            continue
        if any(part in f"/{rel}" for part in SKIP_PATH_PARTS):
            continue
        if "/specs/" in rel and rel.endswith(".inc.rs"):
            continue
        if path.name.startswith("tests") or path.name.endswith("_tests.rs"):
            continue
        for line in production_lines(path):
            code = code_part(line)
            if '"' not in code:
                continue
            for pattern in PATTERNS:
                for name in pattern.findall(code):
                    if name in tool_names:
                        hits[(rel, name)] += 1
    return hits


def load_baseline() -> dict[tuple[str, str], int]:
    if not BASELINE_FILE.exists():
        return {}
    entries: dict[tuple[str, str], int] = {}
    for raw in BASELINE_FILE.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        path, name, count = line.split("\t")
        entries[(path, name)] = int(count)
    return entries


def render_baseline(hits: collections.Counter[tuple[str, str]]) -> str:
    out = [
        "# 内建工具名字面量的判定形态基线（#1155）。",
        "#",
        "# 由 `python3 scripts/tool_name_literal_ratchet.py --print-baseline` 生成。",
        "#",
        "# 只许降不许升：把字面量换成 `crate::cm_names::*` 之后，请同步下调对应计数，",
        "# 归零的条目直接删除。新增工具名请先加常量，不要在判定点写字面量。",
        "#",
        "# 格式：相对路径<TAB>工具名<TAB>处数",
    ]
    for (path, name), count in sorted(hits.items()):
        out.append(f"{path}\t{name}\t{count}")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="内建工具名字面量 ratchet")
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
            "内建工具名判定形态出现未登记的字面量（#1155）。\n"
            "请改用 `crate::cm_names::*` 常量；确属同名巧合（命令名 / JSON 键 / 小节 id /\n"
            "source_role / 正文匹配针），用 `--print-baseline` 重新生成基线并在 PR 里说明原因。\n\n"
        )
        for line in regressions:
            sys.stderr.write(f"  {line}\n")
        sys.stderr.write(f"\n当前合计 {total} 处，基线上限 {limit} 处。\n")
        return 1

    stale = [key for key in baseline if hits.get(key, 0) < baseline[key]]
    print(f"工具名判定形态字面量：{total} 处 / {len(hits)} 个 (文件, 名字) 组合；基线上限 {limit}。")
    if stale:
        print(
            f"提示：{len(stale)} 条基线已高于实际值，请用 --print-baseline 下调以锁住收益。"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
