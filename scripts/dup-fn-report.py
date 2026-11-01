#!/usr/bin/env python3
"""生产代码跨文件「字节级相同」函数体报告（**只读**，不作阻塞门禁）。

背景：#1238 / #1247 的收敛工作发现，「完全相同」的清单必须先把三类假阳性剔掉，
否则永远归不了 0。本脚本把它们固化下来：

1. **trait 方法声明**：`fn f(&self) -> T;` 没有函数体，旧扫描器会把签名往后找到的
   第一个 `{` 当成 body（`cm_benchmark/adapter.rs` 就因此被误报 3 组）。
2. **`#[cfg]` 互斥实现**：同一文件同名的多份若分处不同 `#[cfg]` 分支，是**有意**
   并存的两套实现（`workspace/fs.rs` 的 unix/windows 版、`cm_mcp` 的 stub/真实现）。
3. **test-only 模块**：`#[cfg(test)] mod NAME;` 指向的整个文件都是测试代码
   （`*_golden.rs` 一族），不参与生产重复统计。

判定口径与 #1238「复测口径」一致：去 `//` 与 `/* */` 注释、空白折叠、`sha1` 相同、
`nloc >= min-nloc`。**不折叠字符串字面量**——`system_only` / `user_only` 这类
「只差常量」的函数按 #1238 的非目标保留，折叠后会把它们算成重复。

用法：
    python3 scripts/dup-fn-report.py [--root src] [--min-nloc 4] [--json]

退出码恒为 0（报告工具）；需要门禁时由人来判断阈值。
"""

from __future__ import annotations

import argparse
import collections
import hashlib
import json
import pathlib
import re
import sys

FN_RE = re.compile(r"\bfn\s+([A-Za-z_0-9]+)\s*(?:<[^>]*>)?\s*\(", re.S)
MOD_RE = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([a-z_0-9]+)")
TEST_MOD_RE = re.compile(r"#\[cfg\(test\)\]\s*\n\s*(?:pub\s+)?mod\s+([a-z_0-9]+)\s*;")

COMMENT_LINE = re.compile(r"//[^\n]*")
COMMENT_BLOCK = re.compile(r"/\*.*?\*/", re.S)
WS = re.compile(r"\s+")


def normalize(src: str) -> str:
    s = COMMENT_LINE.sub("", src)
    s = COMMENT_BLOCK.sub("", s)
    return WS.sub(" ", s).strip()


def body_of(src: str, start: int) -> str | None:
    """取 `start` 之后的第一段 `{...}`；若先遇到 `;` 说明是 trait/函数声明，返回 None。"""
    brace = src.find("{", start)
    semi = src.find(";", start)
    if semi != -1 and (brace == -1 or semi < brace):
        return None
    if brace < 0:
        return None
    depth, i = 0, brace
    while i < len(src):
        if src[i] == "{":
            depth += 1
        elif src[i] == "}":
            depth -= 1
            if depth == 0:
                return src[brace : i + 1]
        i += 1
    return None


def collect_test_modules(root: pathlib.Path) -> set[str]:
    names: set[str] = set()
    for f in root.rglob("*.rs"):
        try:
            text = f.read_text(encoding="utf-8")
        except OSError:
            continue
        names.update(TEST_MOD_RE.findall(text))
    return names


def cfg_within(lines: list[str], line_no: int) -> str:
    """返回 `line_no` 所在函数**上方**最近的 `#[cfg(...)]`（用于识别互斥实现）。

    `line_no` 为 1-based 的 `fn` 所在行，故从 `line_no - 2`（0-based 的上一行）起往上找。
    """
    for i in range(line_no - 2, max(-1, line_no - 14), -1):
        if i < 0:
            break
        m = re.search(r"#\[cfg\((.*?)\)\]", lines[i])
        if m:
            return m.group(1).strip()
        stripped = lines[i].strip()
        if stripped and not stripped.startswith(("///", "//", "#[")):
            break
    return ""


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--root", default="src", help="扫描根目录（默认 src）")
    ap.add_argument("--min-nloc", type=int, default=4, help="最小行数（默认 4）")
    ap.add_argument("--json", action="store_true", help="输出机器可读 JSON")
    args = ap.parse_args()

    root = pathlib.Path(args.root)
    if not root.is_dir():
        print(f"目录不存在: {root}", file=sys.stderr)
        return 0

    test_mods = collect_test_modules(root)

    entries = []
    for f in sorted(root.rglob("*.rs")):
        path = str(f)
        stem = f.stem
        if path.endswith("_tests.rs") or "/tests/" in path or stem in test_mods:
            continue
        try:
            src = f.read_text(encoding="utf-8")
        except OSError:
            continue
        lines = src.splitlines()

        # 跟踪 `#[cfg(test)] mod tests { ... }` 的作用域
        stack: dict[int, bool] = {}
        depth, pending, marks, pos = 0, False, [], 0
        for line in src.splitlines(keepends=True):
            if "#[cfg(test)]" in line:
                pending = True
            m = MOD_RE.match(line)
            if m:
                stack[depth + 1] = pending or m.group(1) == "tests"
                pending = False
            depth += line.count("{") - line.count("}")
            for k in list(stack):
                if k > depth:
                    stack.pop(k)
            marks.append((pos, any(stack.values())))
            pos += len(line)

        def in_test(q: int) -> bool:
            flag = False
            for cp, is_t in marks:
                if cp <= q:
                    flag = is_t
                else:
                    break
            return flag

        for m in FN_RE.finditer(src):
            if in_test(m.start()):
                continue
            body = body_of(src, m.end())
            if not body:
                continue
            nloc = body.count("\n") + 1
            if nloc < args.min_nloc:
                continue
            line_no = src.count("\n", 0, m.start()) + 1
            entries.append(
                {
                    "name": m.group(1),
                    "path": path,
                    "line": line_no,
                    "nloc": nloc,
                    "cfg": cfg_within(lines, line_no),
                    "hash": hashlib.sha1(normalize(body).encode()).hexdigest(),
                }
            )

    buckets: dict[str, list[dict]] = collections.defaultdict(list)
    for e in entries:
        buckets[e["hash"]].append(e)
    groups = [v for v in buckets.values() if len(v) >= 2]

    intentional, pending_groups = [], []
    for g in groups:
        same_file_same_name = len({e["name"] for e in g}) == 1 and len({e["path"] for e in g}) == 1
        has_cfg = any(e["cfg"] for e in g)
        (intentional if (same_file_same_name and has_cfg) else pending_groups).append(g)

    if args.json:
        print(
            json.dumps(
                {
                    "scanned_functions": len(entries),
                    "duplicate_groups": len(groups),
                    "intentional_cfg_groups": [
                        [{"name": e["name"], "path": e["path"], "line": e["line"], "cfg": e["cfg"]} for e in g]
                        for g in intentional
                    ],
                    "groups": [
                        [{"name": e["name"], "path": e["path"], "line": e["line"], "nloc": e["nloc"]} for e in g]
                        for g in groups
                    ],
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        return 0

    print(f"扫描生产函数体：{len(entries)} 个（排除 test 模块；nloc >= {args.min_nloc}）")
    print(f"字节级相同分组：{len(groups)} 组")
    print(f"  其中 #[cfg] 互斥实现（有意并存）：{len(intentional)} 组")
    print(f"  其余（需人工判断能否合并）：{len(pending_groups)} 组\n")

    for g in groups:
        tag = "  [cfg 互斥]" if g in intentional else ""
        members = ", ".join(f"{e['name']}@{e['path']}:{e['line']}" for e in g)
        print(f"[{g[0]['nloc']:3}行 x{len(g)}] {members}{tag}")

    print(
        "\n提示：本报告**不作门禁**（#1238 非目标：先积累实测数据再谈阈值）。"
        "\n字节级相同 ≠ 可合并——返回值类型不同（如 canonical_workspace_root）或"
        "\n语义不同只是构造值相同（如 conflict/usage）都需人工判断。"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
