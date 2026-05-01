#!/usr/bin/env python3
"""Rust 函数形参个数：硬上限 + 三项棘轮基线（均不可恶化，降低时可自动写回）。

- 全体函数中 **最大形参个数**（lizard `parameter_count`，含 `self`）不得高于
  `scripts/fn_param_max_baseline.txt`。
- **形参个数最高的 10 个函数之和**不得高于 `scripts/fn_param_top10_sum_baseline.txt`。
- **`#[allow(clippy::too_many_arguments)]` 出现次数**（按行匹配）不得高于
  `scripts/fn_param_allow_count_baseline.txt`。

环境变量：
  PARAM_COUNT_CAP                 单函数形参个数硬上限（默认 32），超过即失败
  PARAM_MAX_BASELINE_FILE         最大形参棘轮基线文件路径
  PARAM_TOP10_BASELINE_FILE       top10 形参之和棘轮基线文件路径
  PARAM_ALLOW_BASELINE_FILE       allow 次数棘轮基线文件路径
  PARAM_NO_UPDATE_BASELINE        设为 1/true 时不写回基线；CI（CI=true）默认不写回
"""
from __future__ import annotations

import os
import re
import sys
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src", ROOT / "crates", ROOT / "frontend-leptos" / "src"]

DEFAULT_MAX_BASELINE = ROOT / "scripts" / "fn_param_max_baseline.txt"
DEFAULT_TOP10_BASELINE = ROOT / "scripts" / "fn_param_top10_sum_baseline.txt"
DEFAULT_ALLOW_BASELINE = ROOT / "scripts" / "fn_param_allow_count_baseline.txt"

ALLOW_LINE_RE = re.compile(r"#\[[^\]]*clippy::too_many_arguments[^\]]*\]")


def _rust_files() -> list[str]:
    out: list[str] = []
    for base in RUST_ROOTS:
        if base.is_dir():
            out.extend(str(p) for p in base.rglob("*.rs"))
    return out


def _count_allow_lines() -> int:
    n = 0
    for base in RUST_ROOTS:
        if not base.is_dir():
            continue
        for path in base.rglob("*.rs"):
            try:
                text = path.read_text(encoding="utf-8")
            except OSError:
                continue
            for line in text.splitlines():
                if "clippy::too_many_arguments" in line and ALLOW_LINE_RE.search(line):
                    n += 1
    return n


def _truthy(s: str | None) -> bool:
    if s is None:
        return False
    return s.lower() in ("1", "true", "yes", "on")


def main() -> int:
    cap = int(os.environ.get("PARAM_COUNT_CAP", "32"))
    max_baseline_path = Path(
        os.environ.get("PARAM_MAX_BASELINE_FILE", str(DEFAULT_MAX_BASELINE))
    )
    top10_baseline_path = Path(
        os.environ.get("PARAM_TOP10_BASELINE_FILE", str(DEFAULT_TOP10_BASELINE))
    )
    allow_baseline_path = Path(
        os.environ.get("PARAM_ALLOW_BASELINE_FILE", str(DEFAULT_ALLOW_BASELINE))
    )
    no_update = _truthy(os.environ.get("PARAM_NO_UPDATE_BASELINE"))
    if _truthy(os.environ.get("CI")):
        no_update = True

    files = _rust_files()
    if not files:
        print("fn-param: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    result = lizard.analyze_files(files)
    params: list[int] = []
    over_cap: list[tuple[int, str, int, str]] = []
    for f in result:
        for fn in f.function_list:
            p = fn.parameter_count
            params.append(p)
            if p > cap:
                over_cap.append((p, f.filename, fn.start_line, fn.name))

    if not params:
        print("fn-param: 未分析到任何函数", file=sys.stderr)
        return 1

    overall_max = max(params)
    params_sorted = sorted(params, reverse=True)
    k = min(10, len(params_sorted))
    top10_sum = sum(params_sorted[:k])
    allow_count = _count_allow_lines()

    def read_int(path: Path, create_val: int, label: str) -> int:
        if path.is_file():
            try:
                return int(path.read_text().strip())
            except ValueError:
                print(
                    f"fn-param: 无法解析基线文件（应为整数一行）: {path}",
                    file=sys.stderr,
                )
                sys.exit(1)
        if not no_update:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(f"{create_val}\n", encoding="utf-8")
            print(
                f"fn-param: 已创建 {label} 基线 {create_val} -> {path}",
                file=sys.stderr,
            )
        return create_val

    max_baseline = read_int(max_baseline_path, overall_max, "最大形参个数")
    top10_baseline = read_int(top10_baseline_path, top10_sum, "top10 形参之和")
    allow_baseline = read_int(allow_baseline_path, allow_count, "too_many_arguments allow 行数")

    print(
        f"fn-param Rust: 函数数={len(params)}, max 形参={overall_max} "
        f"(硬上限≤{cap}, 棘轮≤{max_baseline}), "
        f"top{k} 形参之和={top10_sum} (棘轮≤{top10_baseline}), "
        f"allow 行数={allow_count} (棘轮≤{allow_baseline})"
    )

    rc = 0
    if over_cap:
        rc = 1
        print(f"超过单函数形参硬上限 ({cap})：", file=sys.stderr)
        over_cap.sort(key=lambda x: (-x[0], x[1], x[2], x[3]))
        for p, path, line, name in over_cap[:30]:
            print(f"  {p} 个形参\t{path}:{line}\t{name}", file=sys.stderr)
        if len(over_cap) > 30:
            print(f"  ... 另有 {len(over_cap) - 30} 个", file=sys.stderr)

    if overall_max > max_baseline:
        print(
            f"fn-param: 最大形参 {overall_max} 高于棘轮基线 {max_baseline}（禁止拉高峰值）",
            file=sys.stderr,
        )
        rc = 1

    if top10_sum > top10_baseline:
        print(
            f"fn-param: top{k} 形参之和 {top10_sum} 高于棘轮基线 {top10_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if allow_count > allow_baseline:
        print(
            f"fn-param: allow(clippy::too_many_arguments) 行数 {allow_count} "
            f"高于棘轮基线 {allow_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if rc == 0 and not no_update:
        if overall_max < max_baseline:
            max_baseline_path.write_text(f"{overall_max}\n", encoding="utf-8")
            print(
                f"fn-param: 已收紧最大形参棘轮 {max_baseline} -> {overall_max} ({max_baseline_path})"
            )
        if top10_sum < top10_baseline:
            top10_baseline_path.write_text(f"{top10_sum}\n", encoding="utf-8")
            print(
                f"fn-param: 已收紧 top10 形参之和棘轮 {top10_baseline} -> {top10_sum} ({top10_baseline_path})"
            )
        if allow_count < allow_baseline:
            allow_baseline_path.write_text(f"{allow_count}\n", encoding="utf-8")
            print(
                f"fn-param: 已收紧 allow 行数棘轮 {allow_baseline} -> {allow_count} ({allow_baseline_path})"
            )

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
