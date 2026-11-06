#!/usr/bin/env python3
"""根级公开面清单门禁（S6，见 #1297 与 docs/design/sdk_facade.md §4.1 / §11 Q2）。

**为什么存在**：S6 之前承诺面是散文——`docs/design/crates_io_single_package.md` §2.4
写「以 `src/lib.rs` 为准」，等于「根上所有 `pub` 项」这个**自动集合**，任何 `pub`
都能零摩擦落进 semver 承诺。本脚本把它换成显式清单：根上每个 `pub` 名字都必须被
人工归入 `fixtures/public_api_golden.txt` 的 `promised` 或 `excluded`，否则失败。

**抽取口径**（刻意保守，宁漏不误）：
- 只看 `src/lib.rs` 与 `src/lib_server.rs`（后者由 `include!` 落在 crate 根）。
- 只认**顶格**（第 0 列）的 `pub mod|fn|struct|enum|trait|type|use`。非顶格项属于嵌套
  模块（`mod tests { … }` 等），不在根面。
- `#[cfg(test)]` 项跳过：它们不构成对外面。
- `pub(crate)` 不算公开。
- `pub use a::{b, c as d}` 记 `b` / `d`（别名即公开名）；`pub use path::X` 记 `X`。

**不覆盖什么**：源码级抽取看不到「类型经别处 re-export 的传递闭包」，也不识别
`macro_rules!` 生成的项。它防的是「根上无意识多冒出一个名字」，**不**证明签名等价。
真要签名级比对得上 rustdoc JSON（`cargo +nightly rustdoc -- -Zunstable-options
--output-format json`）或 `cargo public-api`，但两者都不该塞进 pre-commit。

用法：
    python3 scripts/check-public-api.py            # 校验（pre-commit 用）
    python3 scripts/check-public-api.py --list     # 打印实测根面
    CRABMATE_PUBLIC_API_BLESS=1 python3 scripts/check-public-api.py
                                                  # 把新出现的项以 unclassified 追加进清单
"""

from __future__ import annotations

import os
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FIXTURE = ROOT / "fixtures" / "public_api_golden.txt"
ROOT_FILES = ("src/lib.rs", "src/lib_server.rs")

CLASSES = ("promised", "excluded", "unclassified")
ITEM_RE = re.compile(
    r"^pub (?:async\s+|unsafe\s+|const\s+|extern\s+\"[^\"]*\"\s+)*"
    r"(mod|fn|struct|enum|trait|type) ([A-Za-z_][A-Za-z0-9_]*)"
)
ATTR_RE = re.compile(r"^#\[(.*)\]$")


def _surface(attrs: dict[str, str]) -> str:
    """公开面归属：`protocol` 子集 ⊆ `server` 面（`server` feature 含 `protocol`）。"""
    cfg = attrs.get("cfg", "")
    if 'feature = "protocol"' in cfg:
        return "protocol"
    if 'feature = "web"' in cfg:
        return "server+web"
    return "server"


def _scan_attrs(lines: list[str], idx: int) -> dict[str, str]:
    """向上收集紧邻的属性与 doc 注释，返回 cfg / doc_hidden / deprecated / cfg_test。"""
    attrs: dict[str, str] = {}
    j = idx - 1
    while j >= 0:
        s = lines[j].strip()
        if s.startswith("///") or s.startswith("//!"):
            j -= 1
            continue
        m = ATTR_RE.match(s)
        if not m:
            break
        body = m.group(1)
        if body == "doc(hidden)":
            attrs["doc_hidden"] = "1"
        elif body.startswith("deprecated"):
            attrs["deprecated"] = "1"
        elif body.startswith("cfg("):
            attrs["cfg"] = attrs.get("cfg", "") + body
            if "test" in body:
                attrs["cfg_test"] = "1"
        j -= 1
    return attrs


def _use_names(line: str, lines: list[str], idx: int) -> tuple[list[str], int]:
    """解析一条顶格 `pub use`，返回 (公开名列表, 消费到的最后一行下标)。"""
    buf, depth, j = line, line.count("{") - line.count("}"), idx
    while depth > 0 and j + 1 < len(lines):
        j += 1
        buf += " " + lines[j]
        depth += lines[j].count("{") - lines[j].count("}")
    body = buf[len("pub use "):].rstrip(";").strip()
    if "{" not in body:
        # `pub use a::b::C` → `C`；`pub use a::b as c` → `c`（别名即公开名）
        tail = body.split(" as ")[-1].strip() if " as " in body else body
        return [tail.split("::")[-1].strip()], j
    prefix, inner = body.split("{", 1)
    inner = inner.rsplit("}", 1)[0]
    inner = re.sub(r"//.*", "", inner)
    names = []
    for part in inner.split(","):
        part = part.strip()
        if not part:
            continue
        names.append(part.split(" as ")[-1].strip())
    return names, j


def extract() -> dict[str, dict[str, str]]:
    """返回 {`kind name`: {surface, attrs…}}，仅含根级公开项。"""
    found: dict[str, dict[str, str]] = {}
    for rel in ROOT_FILES:
        path = ROOT / rel
        if not path.is_file():
            print(f"check-public-api: 缺少 {rel}", file=sys.stderr)
            sys.exit(1)
        lines = path.read_text(encoding="utf-8").split("\n")
        i = 0
        while i < len(lines):
            line = lines[i]
            if line.startswith("pub use "):
                names, last = _use_names(line, lines, i)
                attrs = _scan_attrs(lines, i)
                if attrs.get("cfg_test") or not names:
                    i = last + 1
                    continue
                for name in names:
                    found[f"use {name}"] = {"surface": _surface(attrs), **attrs}
                i = last + 1
                continue
            m = ITEM_RE.match(line)
            if m and not line.startswith("pub(crate)"):
                attrs = _scan_attrs(lines, i)
                if not attrs.get("cfg_test"):
                    found[f"{m.group(1)} {m.group(2)}"] = {
                        "surface": _surface(attrs),
                        **attrs,
                    }
            i += 1
    return found


def load_fixture() -> tuple[dict[str, str], list[str]]:
    """返回 ({item: class}, 错误列表)。"""
    if not FIXTURE.is_file():
        return {}, [f"清单文件不存在：{FIXTURE.relative_to(ROOT)}"]
    classes: dict[str, str] = {}
    errors: list[str] = []
    for lineno, raw in enumerate(FIXTURE.read_text(encoding="utf-8").split("\n"), 1):
        line = raw.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        parts = line.split("\t")
        if len(parts) != 2:
            errors.append(f"{FIXTURE.name}:{lineno} 语法应为 `<分类>\\t<种类> <名字>`，实为 {raw!r}")
            continue
        cls, item = parts[0].strip(), parts[1].strip()
        if cls not in CLASSES:
            errors.append(f"{FIXTURE.name}:{lineno} 未知分类 {cls!r}（可选 {', '.join(CLASSES)}）")
            continue
        if item in classes:
            errors.append(f"{FIXTURE.name}:{lineno} 重复条目 {item!r}")
            continue
        classes[item] = cls
    return classes, errors


def render(observed: dict[str, dict[str, str]], classes: dict[str, str]) -> str:
    """按分类渲染清单：promised → excluded → unclassified，同类按名字排序。"""
    out = [
        "# crabmate 根级公开面清单（S6，见 #1297）",
        "#",
        "# 每行：<分类>\\t<种类> <名字>；分类仅 promised / excluded / unclassified。",
        "# - promised：进 semver 承诺，改动即 breaking，须同步 CHANGELOG 与设计文档。",
        "# - excluded：根上可见但明确不承诺（多为 #[doc(hidden)]、CLI 内部或过渡期遗留）。",
        "# - unclassified：待人工归类；**存在即失败**，BLESS 只会把人没归类的项放这里。",
        "#",
        "# 由 scripts/check-public-api.py 校验；新增根级 pub 项必须在此显式归类。",
        "# 重生成：CRABMATE_PUBLIC_API_BLESS=1 python3 scripts/check-public-api.py",
        "",
    ]
    for cls in CLASSES:
        items = sorted(k for k, v in classes.items() if v == cls)
        if not items:
            continue
        out.append(f"# --- {cls} ---")
        for item in items:
            out.append(f"{cls}\t{item}")
        out.append("")
    return "\n".join(out).rstrip("\n") + "\n"


def main() -> int:
    observed = extract()

    if "--list" in sys.argv:
        print(f"根级公开项：{len(observed)}")
        for item in sorted(observed):
            meta = observed[item]
            flags = ",".join(
                k for k in ("doc_hidden", "deprecated") if meta.get(k)
            ) or "-"
            print(f"  {meta['surface']:<11} {flags:<16} {item}")
        return 0

    classes, errors = load_fixture()

    if os.environ.get("CRABMATE_PUBLIC_API_BLESS"):
        for item in observed:
            classes.setdefault(item, "unclassified")
        for item in list(classes):
            if item not in observed:
                del classes[item]
        FIXTURE.write_text(render(observed, classes), encoding="utf-8")
        pending = sum(1 for v in classes.values() if v == "unclassified")
        print(f"check-public-api: 已重写 {FIXTURE.relative_to(ROOT)}（{len(classes)} 项，{pending} 项待归类）")
        return 1 if pending else 0

    for item in sorted(observed):
        if item not in classes:
            errors.append(
                f"新增根级公开项未归类：{item}（{observed[item]['surface']}）"
                f" —— 请在 {FIXTURE.name} 里标 promised 或 excluded"
            )
    for item in sorted(classes):
        if item not in observed:
            errors.append(f"清单项已不存在：{item} —— 请从 {FIXTURE.name} 删除")
        elif classes[item] == "unclassified":
            errors.append(f"清单项待归类：{item} —— BLESS 不会替你决定承诺与否")
        elif classes[item] == "promised" and (
            observed[item].get("doc_hidden") or observed[item].get("deprecated")
        ):
            errors.append(
                f"矛盾：{item} 标为 promised，却带 #[doc(hidden)] 或 #[deprecated]"
            )

    if errors:
        print("check-public-api: 根级公开面与清单不一致", file=sys.stderr)
        for err in errors:
            print(f"  - {err}", file=sys.stderr)
        print(
            f"\n共 {len(errors)} 处。清单：{FIXTURE.relative_to(ROOT)}；"
            "\n新增项须人工归类（刻意的摩擦，见 #1297）；确属误删可 "
            "CRABMATE_PUBLIC_API_BLESS=1 重生成后再补归类。",
            file=sys.stderr,
        )
        return 1

    promised = sum(1 for v in classes.values() if v == "promised")
    print(f"check-public-api: ok（{len(observed)} 项根面，其中 promised {promised} 项）")
    return 0


if __name__ == "__main__":
    sys.exit(main())
