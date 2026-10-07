#!/usr/bin/env python3
"""校验 docs/ 与根 README 中的相对 Markdown / 目录链接是否存在。

仅检查相对链接；以下情形跳过：http(s) / mailto / 锚点 / 以 `/` 开头的站点根路径。
判定规则：
- 以 `.md` 结尾：校验文件存在；
- 以 `/` 结尾：校验目录存在；
- 其余（图片等资源、无扩展名的占位示例）不检查。
代码围栏（``` / ~~~）内的内容不参与解析。
"""
import os
import re
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SCAN_DIRS = ["docs"]
SCAN_ROOT_FILES = ["README.md", "README.zh.md", "CHANGELOG.md"]
INLINE_LINK = re.compile(r"\[[^\]]*\]\(([^)]+)\)")


def iter_markdown_files():
    for d in SCAN_DIRS:
        for dp, _dn, fn in os.walk(os.path.join(REPO_ROOT, d)):
            for f in fn:
                if f.endswith(".md"):
                    yield os.path.join(dp, f)
    for f in SCAN_ROOT_FILES:
        p = os.path.join(REPO_ROOT, f)
        if os.path.exists(p):
            yield p


def visible_lines(text):
    """去掉代码围栏内容，保留行号对齐。"""
    out = []
    in_fence = False
    for line in text.splitlines():
        s = line.lstrip()
        if s.startswith("```") or s.startswith("~~~"):
            in_fence = not in_fence
            out.append("")
            continue
        out.append("" if in_fence else line)
    return out


def check_target(path, raw):
    """返回被引用的相对目标（若不存在），否则返回 None。"""
    t = raw.strip().strip("<>").strip()
    parts = t.split()
    if parts:
        t = parts[0]  # 去掉可选的 "title"
    if not t or t.startswith(("http://", "https://", "mailto:", "#", "/")):
        return None
    t = t.split("#", 1)[0]
    if not t:
        return None
    base = os.path.dirname(path)
    if t.endswith(".md"):
        if not os.path.isfile(os.path.normpath(os.path.join(base, t))):
            return t
    elif t.endswith("/"):
        if not os.path.isdir(os.path.normpath(os.path.join(base, t))):
            return t
    return None


def main():
    broken = []
    for path in iter_markdown_files():
        with open(path, encoding="utf-8") as fh:
            lines = visible_lines(fh.read())
        for i, line in enumerate(lines, 1):
            for m in INLINE_LINK.finditer(line):
                bad = check_target(path, m.group(1))
                if bad:
                    broken.append(f"{os.path.relpath(path, REPO_ROOT)}:{i} -> {bad}")
    if broken:
        print("docs relative links broken:")
        for b in broken:
            print("  " + b)
        return 1
    print("docs relative links: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
