#!/usr/bin/env python3
"""环境覆盖（`CM_*`）守卫，两条互不重叠的检查（#1353）。

1. **`part_<数字>` 函数名只许降不许升**：基线 `scripts/part_name_baseline.txt`。
   文件行数硬顶驱动的机械切片一旦回潮，这里的计数就会涨。
2. **`src/cm_config/env_overrides/` 的 `CM_*` 键集合必须与
   `fixtures/env_override_keys.txt` 完全一致，且每个键只出现一次**。
   搬动键或改绑目标字段时，golden 会逼出一次显式确认——现有 `cargo test`
   只覆盖少数几个键（`env_overrides_win_over_toml` 等），拦不住其余键的错绑。

用法：
  python3 scripts/env_override_guard.py                  # 检查（pre-commit 入口）
  python3 scripts/env_override_guard.py --write-golden    # 有意变动后重写键 golden
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
KEY_DIR = ROOT / "src" / "cm_config" / "env_overrides"
KEY_GOLDEN = ROOT / "fixtures" / "env_override_keys.txt"
PART_BASELINE = ROOT / "scripts" / "part_name_baseline.txt"

PART_PATTERN = re.compile(r"\bfn\s+[a-z_0-9]*part_[0-9]")
KEY_PATTERN = re.compile(r'"(CM_[A-Z0-9_]+)"')


def collect_part_hits():
    hits = []
    for path in sorted((ROOT / "src").rglob("*.rs")):
        text = path.read_text(encoding="utf-8", errors="replace")
        for lineno, line in enumerate(text.splitlines(), 1):
            if PART_PATTERN.search(line):
                hits.append(f"{path.relative_to(ROOT)}:{lineno}")
    return hits


def collect_keys():
    """返回 (键 → 所在文件, 重复出现的键)。

    重复键是硬错误：同一个 `CM_*` 出现在两个文件里意味着两处都会写同一字段，
    后写的静默覆盖先写的——这正是「按行号切片」最容易制造的事故。
    """
    owner = {}
    duplicates = []
    for path in sorted(KEY_DIR.glob("*.rs")):
        text = path.read_text(encoding="utf-8", errors="replace")
        for match in KEY_PATTERN.finditer(text):
            key = match.group(1)
            if key in owner:
                duplicates.append((key, owner[key], path.name))
            else:
                owner[key] = path.name
    return owner, duplicates


def read_part_baseline():
    if not PART_BASELINE.exists():
        print(f"错误：缺少基线 {PART_BASELINE.relative_to(ROOT)}", file=sys.stderr)
        sys.exit(2)
    return int(PART_BASELINE.read_text(encoding="utf-8").strip() or "0")


def read_key_golden():
    if not KEY_GOLDEN.exists():
        print(f"错误：缺少键 golden {KEY_GOLDEN.relative_to(ROOT)}", file=sys.stderr)
        sys.exit(2)
    lines = KEY_GOLDEN.read_text(encoding="utf-8").splitlines()
    return [line.strip() for line in lines if line.strip() and not line.startswith("#")]


def write_key_golden(keys):
    KEY_GOLDEN.write_text("\n".join(sorted(keys)) + "\n", encoding="utf-8")
    print(f"已重写 {KEY_GOLDEN.relative_to(ROOT)}（{len(keys)} 个键）")


def check():
    failures = []

    part_hits = collect_part_hits()
    part_baseline = read_part_baseline()
    print(f"part_N 函数: {len(part_hits)} 处（基线上限 {part_baseline}）")
    if len(part_hits) > part_baseline:
        failures.append(
            "新增了 `part_<数字>` 函数名——按职责命名（见 #1353）；命中：\n  "
            + "\n  ".join(part_hits)
        )

    owner, duplicates = collect_keys()
    golden = read_key_golden()
    print(f"CM_* 键: 目录内 {len(owner)} 个 / golden {len(golden)} 个")
    if duplicates:
        detail = "\n  ".join(f"{k}: {a} 与 {b}" for k, a, b in duplicates)
        failures.append(f"`CM_*` 键重复出现（后写会静默覆盖先写）：\n  {detail}")
    missing = sorted(set(golden) - set(owner))
    added = sorted(set(owner) - set(golden))
    if missing or added:
        lines = []
        if missing:
            lines.append("键消失（golden 有、代码无）：\n  " + "\n  ".join(missing))
        if added:
            lines.append("键新增（代码有、golden 无）：\n  " + "\n  ".join(added))
        lines.append(
            "确认这是有意的配置变更后，跑 "
            "`python3 scripts/env_override_guard.py --write-golden` 重写 golden。"
        )
        failures.append("\n".join(lines))

    if failures:
        print("\n环境覆盖守卫未通过：", file=sys.stderr)
        for item in failures:
            print(f"- {item}", file=sys.stderr)
        return 1
    return 0


def main(argv):
    if "--write-golden" in argv:
        owner, _ = collect_keys()
        write_key_golden(owner.keys())
        return 0
    return check()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
