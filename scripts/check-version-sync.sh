#!/usr/bin/env bash
# 版本号衍生文件守卫（#1269）：
#   Cargo.toml → [package] version 是**唯一源头**，但它有三个由版本号派生的产物：
#     - docs/openapi.json → info.version   （openapi_docs_snapshot_matches_spec 也守护，
#       但那条测试只在 client-contract job 跑，失败信息是「drifted (fields: ["info"])」，
#       看不出与版本号有关；#1267 就是这样红的。）
#     - man/crabmate.1   → .TH 的 source 串与 VERSION 段
#     - man/crabmatectl.1→ 同上       （此前**没有**任何信号：没人跑 gen-man 就永远绿）
#     - Cargo.lock       → 本地成员版本（漏了会让离线构建与 lockfile 检查漂移）
#
# 本脚本刻意把失败信息写成「derived file X says A, Cargo.toml says B」，
# 不再需要从契约漂移的措辞里反推根因。修法只有一条：`make set-version VERSION=…`。
#
# 用法：bash scripts/check-version-sync.sh
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

fail=0

# Cargo.toml 的 `[package] version`（只取第一个 `[package]` 段内的 version 行；
# 文件里还有 `[workspace.dependencies]` 与 deb 元数据，不能全局 grep）。
cargo_version() {
  awk '
    /^\[package\]$/ { in_pkg = 1; next }
    /^\[/ { in_pkg = 0 }
    in_pkg && /^version[[:space:]]*=/ {
      gsub(/^version[[:space:]]*=[[:space:]]*"?|".*$/, "")
      print
      exit
    }
  ' Cargo.toml
}

# docs/openapi.json 的 info.version（按 JSON 解析，避免在长 description 里正则猜）。
openapi_version() {
  python3 - <<'PY'
import json
import pathlib
import sys

path = pathlib.Path("docs/openapi.json")
try:
    doc = json.loads(path.read_text(encoding="utf-8"))
except FileNotFoundError:
    print("<missing>")
    sys.exit(0)
except json.JSONDecodeError as exc:
    # 快照坏了就是另一类失败：直接把它当作「说的版本是什么」报出去。
    print(f"<invalid json: {exc.msg}>")
    sys.exit(0)
print(doc.get("info", {}).get("version", "<missing info.version>"))
PY
}

# man 页 `.TH` 行的 source 串（如 `crabmate 0.8.0`）里去掉二进制名后的版本。
man_th_version() {
  local file="$1"
  local bin="$2"
  awk -v bin="$bin" '
    /^\.TH / {
      n = split($0, parts, "\"")
      if (n >= 2) {
        src = parts[2]
        sub("^" bin " ", "", src)
        print src
      }
      exit
    }
  ' "$file"
}

# man 页 `VERSION` 段的第一行（形如 `v0.8.0`）。
man_version_section() {
  local file="$1"
  awk '
    /^\.SH VERSION$/ { want = 1; next }
    want && NF > 0 { print; exit }
  ' "$file"
}

# troff 把 `-` 转义成 `\-`：预发布版本（`v0.8.1\-alpha.0`）若不还原就会被误报成漂移。
detroff() {
  local s="$1"
  printf '%s' "${s//\\-/-}"
}

report() {
  # report <derived file> <what> <found>
  local file="$1"
  local what="$2"
  local found="$3"
  if [[ "$found" != "$CARGO_VER" ]]; then
    echo "MISMATCH: derived file ${file} ${what} says '${found}', Cargo.toml [package] version says '${CARGO_VER}'" >&2
    fail=1
  else
    echo "ok: ${file} ${what} = ${CARGO_VER}"
  fi
}

CARGO_VER="$(cargo_version)"
if [[ -z "$CARGO_VER" ]]; then
  echo "ERROR: 读不到 Cargo.toml 的 [package] version" >&2
  exit 1
fi
echo "== version sync (source: Cargo.toml [package] version = ${CARGO_VER}) =="

report "docs/openapi.json" "info.version" "$(openapi_version)"

for pair in "man/crabmate.1:crabmate" "man/crabmatectl.1:crabmatectl"; do
  file="${pair%%:*}"
  bin="${pair##*:}"
  if [[ ! -f "$file" ]]; then
    echo "MISMATCH: derived file ${file} is missing (regenerate with \`cargo run --features gen-man --bin crabmate-gen-man\`)" >&2
    fail=1
    continue
  fi
  report "$file" ".TH source" "$(detroff "$(man_th_version "$file" "$bin")")"
  # VERSION 段是 clap 的 `--version` 长文本（带 `v` 前缀），单独比对。
  section="$(detroff "$(man_version_section "$file")")"
  if [[ "$section" != "v${CARGO_VER}" ]]; then
    echo "MISMATCH: derived file ${file} VERSION section says '${section}', Cargo.toml [package] version says '${CARGO_VER}' (expected 'v${CARGO_VER}')" >&2
    fail=1
  else
    echo "ok: ${file} VERSION section = ${section}"
  fi
done

lock_ver="$(awk '
  /^name = "crabmate"$/ { want = 1; next }
  want && /^version = "/ {
    gsub(/^version = "|"$/, "")
    print
    exit
  }
  want && /^\[\[package\]\]$/ { want = 0 }
' Cargo.lock)"
report "Cargo.lock" "crabmate package version" "${lock_ver:-<missing>}"

if [[ "$fail" -ne 0 ]]; then
  cat >&2 <<EOF

version sync FAILED.
一条命令修好全部衍生文件（只写文件，不 commit、不打 tag、不推送）：
  make set-version VERSION=${CARGO_VER}     # 版本号不变，仅重生成派生文件
  make set-version VERSION=X.Y.Z[-pre]      # 真正 bump（含 Cargo.lock / openapi.json / man/*.1）
EOF
  exit 1
fi
echo "version sync OK"
