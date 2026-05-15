#!/usr/bin/env python3
"""Rust 代码规模棘轮（与 **`scripts/fn-nloc-ratchet.sh`** 同入口）：函数 `nloc` + 源文件物理行数。

**A. 函数体行数（lizard `nloc`）**

- 全体函数中 **最大 nloc** 不得高于 `scripts/fn_nloc_max_baseline.txt`。
- **nloc 最高的 10 个函数之和**不得高于 `scripts/fn_nloc_top10_sum_baseline.txt`。

**B. 源文件物理行数（按 `.rs` 文件）**

- 全体已扫描 Rust 源文件中 **最大行数**不得高于 `scripts/rust_file_max_lines_baseline.txt`。
- **行数最高的 10 个文件之行数和**不得高于 `scripts/rust_file_top10_lines_sum_baseline.txt`。
- **棘轮禁止增大**：自动写回上述两文件时，新值 **不得大于** 本次运行开始时磁盘上已存在的值。

**C. 行为常量（本脚本不读环境变量、不接 CLI 参数）**

- 基线路径固定为仓库内 `scripts/` 下上述四个文件名。
- 校验通过且度量低于当前棘轮时 **写回收紧**（含本地与 CI）。
- Git 侧仅校验：**工作区基线文件数值不得大于 `git show HEAD` 中同路径**（防止未降代码却抬基线过钩）。
"""
from __future__ import annotations

import subprocess
import sys
from pathlib import Path

try:
    import lizard
except ImportError:
    print("lizard 未安装。请执行: pip install lizard", file=sys.stderr)
    sys.exit(1)

ROOT = Path(__file__).resolve().parent.parent
RUST_ROOTS = [ROOT / "src", ROOT / "crates", ROOT / "frontend" / "src"]

MAX_NLOC_BASELINE = ROOT / "scripts" / "fn_nloc_max_baseline.txt"
TOP10_NLOC_SUM_BASELINE = ROOT / "scripts" / "fn_nloc_top10_sum_baseline.txt"
FILE_MAX_LINES_BASELINE = ROOT / "scripts" / "rust_file_max_lines_baseline.txt"
FILE_TOP10_LINES_SUM_BASELINE = ROOT / "scripts" / "rust_file_top10_lines_sum_baseline.txt"


def _rust_files() -> list[str]:
    out: list[str] = []
    for base in RUST_ROOTS:
        if base.is_dir():
            out.extend(str(p) for p in base.rglob("*.rs"))
    return out


def _read_int(path: Path, create_val: int, label: str) -> int:
    if path.is_file():
        try:
            return int(path.read_text().strip())
        except ValueError:
            print(
                f"fn-nloc: 无法解析基线文件（应为整数一行）: {path}",
                file=sys.stderr,
            )
            sys.exit(1)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(f"{create_val}\n", encoding="utf-8")
    print(
        f"fn-nloc: 已创建 {label} 基线 {create_val} -> {path}",
        file=sys.stderr,
    )
    return create_val


def _file_physical_line_count(path: Path) -> int:
    with path.open("r", encoding="utf-8", errors="replace") as f:
        return sum(1 for _ in f)


def _read_optional_int_baseline(path: Path) -> int | None:
    """磁盘上已提交的棘轮值（若文件不存在则为 None）。用于禁止棘轮增大。"""
    if not path.is_file():
        return None
    try:
        return int(path.read_text().strip())
    except ValueError:
        print(
            f"fn-nloc: 无法解析棘轮基线文件（应为整数一行）: {path}",
            file=sys.stderr,
        )
        sys.exit(1)


def _git_show_baseline_int(root: Path, rev: str, path: Path) -> int | None:
    """读取 `rev:path` 中棘轮整数一行；不可用时返回 None（无 Git、浅克隆、文件在该版本不存在）。"""
    try:
        rel = path.relative_to(root).as_posix()
    except ValueError:
        return None
    spec = f"{rev}:{rel}"
    try:
        proc = subprocess.run(
            ["git", "-C", str(root), "show", spec],
            capture_output=True,
            text=True,
            timeout=120,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if proc.returncode != 0:
        return None
    raw = proc.stdout.strip().splitlines()
    if not raw:
        return None
    try:
        return int(raw[0].strip())
    except ValueError:
        return None


def _reject_manual_baseline_increases(
    root: Path,
    baseline_specs: list[tuple[Path, str]],
) -> int:
    """禁止通过改基线文件「放水」：工作区数值不得大于 HEAD 中同文件。"""
    rc = 0
    for path, label in baseline_specs:
        disk = _read_optional_int_baseline(path)
        head_v = _git_show_baseline_int(root, "HEAD", path)

        if disk is not None and head_v is not None and disk > head_v:
            print(
                f"fn-nloc: 禁止棘轮基线增大（{label}）: 工作区={disk} > HEAD={head_v} ({path})",
                file=sys.stderr,
            )
            rc = 1

    if rc != 0:
        print(
            "fn-nloc: 请用拆分模块、压缩函数等方式降低度量；确需抬高基线须先降代码再提交。",
            file=sys.stderr,
        )
    return rc


def main() -> int:
    baseline_specs = [
        (MAX_NLOC_BASELINE, "最大 nloc"),
        (TOP10_NLOC_SUM_BASELINE, "top10 nloc 之和"),
        (FILE_MAX_LINES_BASELINE, "单文件最大行数"),
        (FILE_TOP10_LINES_SUM_BASELINE, "top10 文件行数和"),
    ]
    bump_rc = _reject_manual_baseline_increases(ROOT, baseline_specs)
    if bump_rc != 0:
        return bump_rc

    files = _rust_files()
    if not files:
        print("fn-nloc: 未找到 Rust 源文件", file=sys.stderr)
        return 1

    # --- function nloc (lizard) ---
    result = lizard.analyze_files(files)
    nlocs: list[int] = []
    for f in result:
        for fn in f.function_list:
            nlocs.append(fn.nloc)

    if not nlocs:
        print("fn-nloc: 未分析到任何函数", file=sys.stderr)
        return 1

    overall_max = max(nlocs)
    nlocs_sorted = sorted(nlocs, reverse=True)
    k_fn = min(10, len(nlocs_sorted))
    top10_sum = sum(nlocs_sorted[:k_fn])

    max_baseline = _read_int(MAX_NLOC_BASELINE, overall_max, "最大 nloc")
    top10_baseline = _read_int(TOP10_NLOC_SUM_BASELINE, top10_sum, "top10 nloc 之和")

    print(
        f"fn-nloc Rust: 函数数={len(nlocs)}, max nloc={overall_max} "
        f"(棘轮≤{max_baseline}), "
        f"top{k_fn} nloc 之和={top10_sum} (棘轮≤{top10_baseline})"
    )

    rc = 0

    if overall_max > max_baseline:
        print(
            f"fn-nloc: 最大 nloc {overall_max} 高于棘轮基线 {max_baseline}（禁止拉高峰值）",
            file=sys.stderr,
        )
        rc = 1

    if top10_sum > top10_baseline:
        print(
            f"fn-nloc: top{k_fn} nloc 之和 {top10_sum} 高于棘轮基线 {top10_baseline}",
            file=sys.stderr,
        )
        rc = 1

    # --- per-file physical line counts ---
    file_counts: list[tuple[int, str]] = []
    for s in files:
        p = Path(s)
        try:
            nlines = _file_physical_line_count(p)
        except OSError as e:
            print(f"fn-nloc: 无法读取文件行数 {p}: {e}", file=sys.stderr)
            rc = 1
            continue
        file_counts.append((nlines, s))

    if not file_counts:
        print("fn-nloc: 未统计到任何源文件行数", file=sys.stderr)
        return 1

    file_max = max(c[0] for c in file_counts)
    file_counts_sorted = sorted(file_counts, key=lambda x: -x[0])
    k_file = min(10, len(file_counts_sorted))
    file_top10_sum = sum(c[0] for c in file_counts_sorted[:k_file])

    # 写回前对照：这两个棘轮基线「不允许增大」（仅允许收紧或新建）。
    orig_file_max_lines_baseline = _read_optional_int_baseline(FILE_MAX_LINES_BASELINE)
    orig_file_top10_lines_sum_baseline = _read_optional_int_baseline(
        FILE_TOP10_LINES_SUM_BASELINE
    )

    file_max_baseline = _read_int(FILE_MAX_LINES_BASELINE, file_max, "单文件最大行数")
    file_top10_baseline = _read_int(
        FILE_TOP10_LINES_SUM_BASELINE,
        file_top10_sum,
        "top10 文件行数之和",
    )

    print(
        f"fn-nloc Rust 文件: 文件数={len(file_counts)}, max 行数={file_max} "
        f"(棘轮≤{file_max_baseline}), "
        f"top{k_file} 文件行数和={file_top10_sum} (棘轮≤{file_top10_baseline})"
    )

    if file_max > file_max_baseline:
        print(
            f"fn-nloc: 单文件最大行数 {file_max} 高于棘轮基线 {file_max_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if file_top10_sum > file_top10_baseline:
        print(
            f"fn-nloc: top{k_file} 文件行数和 {file_top10_sum} 高于棘轮基线 {file_top10_baseline}",
            file=sys.stderr,
        )
        rc = 1

    if rc == 0:
        if overall_max < max_baseline:
            MAX_NLOC_BASELINE.write_text(f"{overall_max}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已收紧最大 nloc 棘轮 {max_baseline} -> {overall_max} ({MAX_NLOC_BASELINE})"
            )
        if top10_sum < top10_baseline:
            TOP10_NLOC_SUM_BASELINE.write_text(f"{top10_sum}\n", encoding="utf-8")
            print(
                f"fn-nloc: 已收紧 top10 nloc 之和棘轮 {top10_baseline} -> {top10_sum} ({TOP10_NLOC_SUM_BASELINE})"
            )
        if file_max < file_max_baseline:
            deny_file_max = (
                orig_file_max_lines_baseline is not None
                and file_max > orig_file_max_lines_baseline
            )
            if deny_file_max:
                print(
                    "fn-nloc: 拒绝写回单文件最大行数棘轮："
                    f"度量值 {file_max} 大于本次运行开始时磁盘基线 "
                    f"{orig_file_max_lines_baseline}（棘轮禁止增大）",
                    file=sys.stderr,
                )
                rc = 1
            else:
                FILE_MAX_LINES_BASELINE.write_text(f"{file_max}\n", encoding="utf-8")
                print(
                    f"fn-nloc: 已收紧单文件最大行数棘轮 {file_max_baseline} -> {file_max} ({FILE_MAX_LINES_BASELINE})"
                )
        if file_top10_sum < file_top10_baseline:
            deny_file_top10 = (
                orig_file_top10_lines_sum_baseline is not None
                and file_top10_sum > orig_file_top10_lines_sum_baseline
            )
            if deny_file_top10:
                print(
                    "fn-nloc: 拒绝写回 top10 文件行数和棘轮："
                    f"度量值 {file_top10_sum} 大于本次运行开始时磁盘基线 "
                    f"{orig_file_top10_lines_sum_baseline}（棘轮禁止增大）",
                    file=sys.stderr,
                )
                rc = 1
            else:
                FILE_TOP10_LINES_SUM_BASELINE.write_text(
                    f"{file_top10_sum}\n", encoding="utf-8"
                )
                print(
                    f"fn-nloc: 已收紧 top10 文件行数和棘轮 {file_top10_baseline} -> {file_top10_sum} ({FILE_TOP10_LINES_SUM_BASELINE})"
                )

    return rc


if __name__ == "__main__":
    raise SystemExit(main())
