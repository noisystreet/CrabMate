#!/usr/bin/env python3
"""生产路径 unwrap/expect ratchet（#1152 的防回退闸门）。

**为什么口径必须走 clippy 而不是 grep / 静态扫描**

本仓库重度使用 `#[cfg(feature = ...)]` 门控（`cm_types` 受 `protocol`、`cm_tools`
受 `server`），静态扫描看不见 cfg：它既要把 `#[cfg(test)]` 的 2500+ 处算进来，
又会把被 feature 关掉的模块算成生产代码。此外字符串里的 `//`（如正则
`https?://`）会让「按行截断注释」的朴素做法漏掉整段代码。

实测对比（本轮，rustc 1.98.0）：静态扫描得 **68**，clippy 得 **70**，差的几处
正是上述三类（`clone_validate.rs` 的 `https?://` 正则、`markdown_links/core.rs`
的 URL、`subprocess_session.rs` 的字符串；反向多算的是 `test_fixtures.rs`，
它受 `protocol` 门控、`--lib` 非 test 时不编译）。口径差会随代码漂移，所以
以 clippy 为准。

执行：

    cargo clippy --lib --all-features --message-format=json \\
        -- -W clippy::unwrap_used -W clippy::expect_used

`--lib` 不启用 test cfg，`#[cfg(test)] mod tests` 自然不参与编译；解析
`--message-format=json` 而不是抓文本，避免依赖 warning 文案。

**这不是「清零」计划**：`AGENTS.md` → Rust error handling 明确允许
`LazyLock` / `OnceLock` 一次性初始化与 `src/main.rs` 启动路径的 `expect`，
它们占当前 70 处里的 46 处（`code_nav_outline.rs` 11、`clone_validate.rs` 6、
`http_fetch/decode.rs` 5、`redact.rs` 5 等全是 `LazyLock<Regex>`）。本 ratchet
只冻结总量、只许降不许升。

基线 `scripts/unwrap_baseline.txt` 只许降不许升：某文件计数上升即失败。

用法：
    python3 scripts/unwrap_ratchet.py                   # 校验（pre-commit 用）
    python3 scripts/unwrap_ratchet.py --print-baseline  # 打印当前基线文本
"""

from __future__ import annotations

import argparse
import collections
import json
import pathlib
import subprocess
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
BASELINE_FILE = ROOT / "scripts" / "unwrap_baseline.txt"

CLIPPY_ARGS = [
    "cargo",
    "clippy",
    "--lib",
    "--all-features",
    "--message-format=json",
    "--",
    "-W",
    "clippy::unwrap_used",
    "-W",
    "clippy::expect_used",
]

LINT_CODES = frozenset({"clippy::unwrap_used", "clippy::expect_used"})


def rustc_version() -> str:
    try:
        out = subprocess.run(
            ["rustc", "--version"], cwd=ROOT, capture_output=True, text=True, check=True
        )
    except (OSError, subprocess.CalledProcessError):
        return "unknown"
    return out.stdout.strip()


def run_clippy() -> collections.Counter[str]:
    """返回 {相对路径: 处数}。"""
    proc = subprocess.run(CLIPPY_ARGS, cwd=ROOT, capture_output=True, text=True)
    if proc.stderr:
        sys.stderr.write(proc.stderr)

    hits: collections.Counter[str] = collections.Counter()
    for raw in proc.stdout.splitlines():
        raw = raw.strip()
        if not raw.startswith("{"):
            continue
        try:
            obj = json.loads(raw)
        except json.JSONDecodeError:
            continue
        if obj.get("reason") != "compiler-message":
            continue
        message = obj.get("message") or {}
        if (message.get("code") or {}).get("code") not in LINT_CODES:
            continue
        spans = message.get("spans") or []
        if not spans:
            continue
        hits[spans[0]["file_name"]] += 1

    # 编译失败时 clippy 不会产出这些诊断；静默通过比失败更危险。
    if proc.returncode != 0 and not hits:
        sys.stderr.write(
            "cargo clippy 失败且未解析到任何 unwrap/expect 诊断；"
            "请先修好编译错误（见上方输出）。\n"
        )
        raise SystemExit(1)

    return +hits


def load_baseline() -> tuple[dict[str, int], str]:
    """返回 ({路径: 处数}, 基线里记录的 rustc 版本)。"""
    if not BASELINE_FILE.exists():
        return {}, ""
    entries: dict[str, int] = {}
    version = ""
    for raw in BASELINE_FILE.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line:
            continue
        if line.startswith("#"):
            if line.startswith("# rustc:"):
                version = line.split(":", 1)[1].strip()
            continue
        path, count = line.split("\t")
        entries[path] = int(count)
    return entries, version


def render_baseline(hits: collections.Counter[str]) -> str:
    out = [
        "# 生产路径 unwrap/expect 基线（#1152）。",
        "#",
        "# 由 `python3 scripts/unwrap_ratchet.py --print-baseline` 生成。",
        f"# rustc: {rustc_version()}",
        "#",
        "# 口径：cargo clippy --lib --all-features -- -W clippy::unwrap_used",
        "#       -W clippy::expect_used 的诊断条数（--lib 不含 test cfg）。",
        "#",
        "# 只许降不许升：把 unwrap/expect 改成 `?` + 上下文后，请同步下调对应计数，",
        "# 归零的条目直接删除。允许保留的只有 AGENTS.md 点名的 LazyLock / OnceLock",
        "# 一次性初始化与 src/main.rs 启动路径。",
        "#",
        "# 注意：本计数与 clippy/rustc 版本绑定（见上方 rustc 行）。升级工具链后若数字",
        "# 整体漂移，应重新生成基线并在 PR 里说明是工具链变化而非回退。",
        "#",
        "# 格式：相对路径<TAB>处数",
    ]
    for path, count in sorted(hits.items()):
        out.append(f"{path}\t{count}")
    return "\n".join(out) + "\n"


def main() -> int:
    parser = argparse.ArgumentParser(description="生产路径 unwrap/expect ratchet")
    parser.add_argument(
        "--print-baseline",
        action="store_true",
        help="打印当前扫描结果的基线文本（人工确认后写入 baseline 文件）",
    )
    args = parser.parse_args()

    hits = run_clippy()
    if args.print_baseline:
        sys.stdout.write(render_baseline(hits))
        return 0

    baseline, baseline_version = load_baseline()
    total = sum(hits.values())
    limit = sum(baseline.values())

    regressions: list[str] = []
    for path, count in sorted(hits.items()):
        allowed = baseline.get(path, 0)
        if count > allowed:
            where = "新增" if path not in baseline else f"超出基线（{allowed} → {count}）"
            regressions.append(f"{path}\t{where}")

    if regressions:
        sys.stderr.write(
            "生产路径出现未登记的 unwrap/expect（#1152）。\n"
            "请改成 `?` + 上下文，或确认属于 AGENTS.md 允许的 LazyLock / 启动路径后\n"
            "用 `--print-baseline` 更新基线并在 PR 里说明理由。\n\n"
        )
        for line in regressions:
            sys.stderr.write(f"  {line}\n")
        sys.stderr.write(f"\n当前合计 {total} 处，基线上限 {limit} 处。\n")
        return 1

    current_version = rustc_version()
    print(f"生产路径 unwrap/expect：{total} 处 / {len(hits)} 个文件；基线上限 {limit}。")
    if baseline_version and baseline_version != current_version:
        print(
            f"提示：基线由 `{baseline_version}` 生成，当前为 `{current_version}`；"
            "若数字整体漂移属工具链变化，可重新生成基线。"
        )

    stale = [path for path in baseline if hits.get(path, 0) < baseline[path]]
    if stale:
        print(f"提示：{len(stale)} 条基线已高于实际值，请用 --print-baseline 下调以锁住收益。")
    return 0


if __name__ == "__main__":
    sys.exit(main())
