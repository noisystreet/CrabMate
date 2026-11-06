#!/usr/bin/env python3
"""Markdown 行长棘轮：md 文件里的超长行只许降不许升。

**为什么用基线而不是硬上限**：本仓库的中文文档基本「一段一行」（不硬折行），
现有 135 个 md 里 prose 行的 p90 列宽已经到 177。一上来就硬卡阈值只有两种结局：
要么全员红、要么有人把阈值调到毫无约束力。所以口径是「冻结现状 + 禁止新增」：
每个文件的超长行数记进 `scripts/md_line_length_baseline.txt`，只许降不许升。

计数口径
- 默认按**显示列宽**：东亚宽字符（Unicode `W`/`F`，中日韩文字与全角标点）计 2 列，
  与编辑器/终端里的观感一致；制表符计 4 列。`--count-mode chars` 改按码位数计数。
- 阈值默认 160 列（中文约 80 字），`--limit` 可临时覆盖。

不计入的行（折行也修不掉，统计它们只能逼人删内容）
- 围栏代码块内部（``` / ~~~ 包起来的行）：代码换行会改变语义。
- markdown 表格行（以 `|` 开头）：表格语法不允许折行。

用法：
    python3 scripts/md_line_length_ratchet.py                    # 校验（pre-commit 用）
    python3 scripts/md_line_length_ratchet.py --print-baseline    # 打印当前基线
    python3 scripts/md_line_length_ratchet.py --limit 200         # 临时看阈值放宽后的影响面
"""

from __future__ import annotations

import argparse
import pathlib
import re
import sys
import unicodedata

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE_FILE = ROOT / "scripts" / "md_line_length_baseline.txt"

DEFAULT_LIMIT = 160
SNIPPET_CHARS = 72
TAB_COLUMNS = 4

# 构建产物、忽略区、agent 草稿区：不参与统计（`.github/` 等项目文档目录仍会被扫）。
SKIP_DIR_PARTS = frozenset(
    {".git", "target", "node_modules", "agent_space", ".crabmate", "vendor", "dist", "__pycache__"}
)

WIDE_EAST_ASIAN = frozenset({"W", "F"})

# --count-mode 取值；"width" 是默认口径（显示列），"chars" 是码位数。
COUNT_MODES = ("width", "chars")


def line_size(line: str, count_mode: str) -> int:
    """返回该行在指定口径下的长度。"""
    if count_mode == "chars":
        return len(line)
    return sum(
        TAB_COLUMNS
        if ch == "\t"
        else (2 if unicodedata.east_asian_width(ch) in WIDE_EAST_ASIAN else 1)
        for ch in line
    )


def fence_marker(line: str) -> str | None:
    """若该行开启/关闭围栏代码块，返回其标记（``` 或 ~~~）。"""
    stripped = line.lstrip()
    for marker in ("```", "~~~"):
        if stripped.startswith(marker):
            return marker
    return None


def is_table_row(line: str) -> bool:
    return line.lstrip().startswith("|")


def snippet(line: str) -> str:
    text = " ".join(line.split())
    return text if len(text) <= SNIPPET_CHARS else text[:SNIPPET_CHARS] + "…"


def scan_file(path: pathlib.Path, limit: int, count_mode: str) -> list[tuple[int, int, str]]:
    """返回该文件所有超长行的 (行号, 长度, 内容摘要)；行号 1-based。"""
    offenders: list[tuple[int, int, str]] = []
    fence: str | None = None
    text = path.read_text(encoding="utf-8", errors="replace")
    for lineno, line in enumerate(text.splitlines(), start=1):
        marker = fence_marker(line)
        if fence is not None:
            # 围栏内部：只有同标记的那一行才是闭合，其余（含另一种标记）都算代码。
            if marker == fence:
                fence = None
            continue
        if marker is not None:
            fence = marker
            continue
        if is_table_row(line):
            continue
        size = line_size(line, count_mode)
        if size > limit:
            offenders.append((lineno, size, snippet(line)))
    return offenders


def collect(limit: int, count_mode: str) -> dict[str, list[tuple[int, int, str]]]:
    """{相对路径: 超长行列表}；只保留有超长行的文件。"""
    result: dict[str, list[tuple[int, int, str]]] = {}
    for path in sorted(ROOT.rglob("*.md")):
        rel = path.relative_to(ROOT)
        if any(part in SKIP_DIR_PARTS for part in rel.parts):
            continue
        offenders = scan_file(path, limit, count_mode)
        if offenders:
            result[rel.as_posix()] = offenders
    return result


def load_baseline_meta() -> tuple[int | None, str | None]:
    """读基线头部记录的生成参数；用于提示「换了口径，差异不代表回归」。"""
    if not BASELINE_FILE.exists():
        return None, None
    for raw in BASELINE_FILE.read_text(encoding="utf-8").splitlines():
        if not raw.startswith("#"):
            break
        limit = re.search(r"limit=(\d+)", raw)
        if limit:
            mode = re.search(r"count-mode=(\w+)", raw)
            return int(limit.group(1)), (mode.group(1) if mode else None)
    return None, None


def load_baseline() -> dict[str, int]:
    if not BASELINE_FILE.exists():
        return {}
    entries: dict[str, int] = {}
    for raw in BASELINE_FILE.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        rel, count = line.split("\t")
        entries[rel] = int(count)
    return entries


def render_baseline(
    result: dict[str, list[tuple[int, int, str]]], limit: int, count_mode: str
) -> str:
    out = [
        "# md 超长行基线（行长棘轮）。",
        "#",
        "# 由 `python3 scripts/md_line_length_ratchet.py --print-baseline` 生成。",
        "#",
        f"# 生成参数：limit={limit} count-mode={count_mode}（脚本读这行判断你是否换了口径）。",
        "#",
        "# 只许降不许升：把长行折开之后，请同步下调对应计数；归零的条目直接删除。",
        "# 新增的超长行会被门禁拦下——请折行，而不是重跑本命令抬高基线。",
        "#",
        "# 格式：相对路径<TAB>超长行数",
    ]
    for rel, offenders in sorted(result.items()):
        out.append(f"{rel}\t{len(offenders)}")
    return "\n".join(out) + "\n"


def warn_if_params_mismatch(limit: int, count_mode: str) -> None:
    base_limit, base_mode = load_baseline_meta()
    if base_limit is None or (base_limit == limit and base_mode == count_mode):
        return
    sys.stderr.write(
        f"警告：基线是用 limit={base_limit} count-mode={base_mode} 生成的，本次是 "
        f"limit={limit} count-mode={count_mode} —— 下面的差异来自口径变化，不代表回归。\n"
    )


def report_regressions(
    regressions: list[tuple[str, list[tuple[int, int, str]], int | None]],
    limit: int,
    count_mode: str,
    max_details: int,
    max_files: int,
) -> None:
    unit = "码位" if count_mode == "chars" else "列"
    sys.stderr.write(
        f"md 出现新的超长行（阈值 {limit} {unit}）。\n"
        "请把这一行折成多行（中文可在逗号、顿号处断行）；超长 URL 改用引用式链接，\n"
        "把链接定义单独放一行，正文里只留短引用：\n"
        "  详见 [ref]。\n"
        "  [ref]: https://example.com/…\n"
        "表格行与围栏代码块不计入（二者无法折行）。若某行确实无法折行，重跑\n"
        "`--print-baseline` 会量化成放宽门禁，需要在 PR 里说明并由 reviewer 同意。\n\n"
    )
    for rel, offenders, allowed in regressions[:max_files]:
        if allowed is None:
            where = "新增文件"
        else:
            where = f"基线 {allowed} → 实际 {len(offenders)}"
        sys.stderr.write(f"  {rel}: {len(offenders)} 行超长（{where}）\n")
        for lineno, size, text in offenders[:max_details]:
            sys.stderr.write(f"      L{lineno} {size} {unit}: {text}\n")
        hidden = len(offenders) - max_details
        if hidden > 0:
            sys.stderr.write(f"      …另有 {hidden} 行，未列出\n")
    if len(regressions) > max_files:
        sys.stderr.write(f"  …另有 {len(regressions) - max_files} 个文件超基线，未列出\n")


def main() -> int:
    parser = argparse.ArgumentParser(description="Markdown 行长棘轮（超长行只许降不许升）")
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="打印当前扫描结果的基线文本（人工确认后写入 baseline 文件）",
    )
    parser.add_argument("--limit", type=int, default=DEFAULT_LIMIT, help="行长阈值，默认 160")
    parser.add_argument(
        "--count-mode",
        choices=COUNT_MODES,
        default="width",
        help="width=显示列宽（默认），chars=码位数",
    )
    parser.add_argument("--max-details", type=int, default=3, help="每个文件最多列出几行，默认 3")
    parser.add_argument("--max-files", type=int, default=20, help="最多列出几个超基线文件，默认 20")
    args = parser.parse_args()

    if args.limit < 1:
        parser.error("--limit 必须是正整数")

    result = collect(args.limit, args.count_mode)
    if args.print_baseline:
        sys.stdout.write(render_baseline(result, args.limit, args.count_mode))
        return 0

    warn_if_params_mismatch(args.limit, args.count_mode)

    baseline = load_baseline()
    regressions: list[tuple[str, list[tuple[int, int, str]], int | None]] = []
    for rel, offenders in sorted(result.items()):
        allowed = baseline.get(rel)
        if allowed is None or len(offenders) > allowed:
            regressions.append((rel, offenders, allowed))

    if regressions:
        report_regressions(
            regressions, args.limit, args.count_mode, max(1, args.max_details), max(1, args.max_files)
        )
        total = sum(len(v) for v in result.values())
        limit_sum = sum(baseline.values())
        sys.stderr.write(f"\n当前合计 {total} 行超长 / {len(result)} 个文件，基线上限 {limit_sum} 行。\n")
        return 1

    unit = "码位" if args.count_mode == "chars" else "列"
    total = sum(len(v) for v in result.values())
    print(
        f"md 行长（≤ {args.limit} {unit}）：{total} 行超长 / {len(result)} 个文件；"
        f"基线上限 {sum(baseline.values())} 行。"
    )
    stale = [rel for rel, allowed in baseline.items() if allowed > len(result.get(rel, []))]
    if stale:
        print(f"提示：{len(stale)} 条基线已高于实际值，请用 --print-baseline 下调以锁住收益。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
