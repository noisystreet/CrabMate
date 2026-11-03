#!/usr/bin/env bash
# 一条命令改版本（#1269）：把「记得住的一组约定」压成一个入口。
#
#   Cargo.toml → [package] version 仍是唯一**源头**；本脚本负责所有**派生**产物：
#     1. [package] version 值本身；
#     2. Cargo.lock 里本地成员的版本（`cargo metadata` 重解）；
#     3. docs/openapi.json 的 info.version（`CRABMATE_BLESS=1 cargo test --lib
#        web::openapi::tests::openapi_docs_snapshot_matches_spec`）；
#     4. man/crabmate.1 与 man/crabmatectl.1 的 .TH / VERSION 段
#        （`cargo run --features gen-man --bin crabmate-gen-man`）；
#     5. 打印 CHANGELOG.md 需要的那两行骨架（**正文仍由人写**）。
#
# 只写文件：**不** commit、**不** 打 tag、**不** 推远端——发布仍走现有流程。
# 最后用 scripts/check-version-sync.sh 自证同步（pre-commit / ci.yml 用的是同一个守卫）。
#
# 用法：scripts/set-version.sh X.Y.Z[-pre]     （或 make set-version VERSION=…）
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

NEW_VER="${1:-}"
if [[ -z "$NEW_VER" || "$NEW_VER" == "-h" || "$NEW_VER" == "--help" ]]; then
  usage
  exit 1
fi

# 与 .github/workflows/release.yml 的 tag 校验保持同一语义（tag 为 `v` + 本字符串）。
if [[ ! "$NEW_VER" =~ ^[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?$ ]]; then
  echo "ERROR: 版本号格式非法：'${NEW_VER}'（期望 X.Y.Z 或 X.Y.Z-pre，如 0.8.1-alpha.0）" >&2
  exit 1
fi

current_version() {
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

OLD_VER="$(current_version)"
if [[ -z "$OLD_VER" ]]; then
  echo "ERROR: 读不到 Cargo.toml 的 [package] version" >&2
  exit 1
fi

# core 版本（去掉 -pre 后缀）：release.yml 抽 CHANGELOG 用的就是它。
CORE_VER="${NEW_VER%%[-+]*}"

echo "[set-version] Cargo.toml: ${OLD_VER} -> ${NEW_VER}"

# 1) 只改 [package] 段里的那一行：文件里还有 [workspace.dependencies] 与 deb 元数据，
#    全局 sed 会改错地方。
TMP_TOML="$(mktemp)"
trap 'rm -f "$TMP_TOML"' EXIT
awk -v new="$NEW_VER" '
  /^\[package\]$/ { in_pkg = 1; print; next }
  /^\[/ { in_pkg = 0 }
  # 只替换 [package] 段内**第一处** version 行。
  in_pkg && !done && /^version[[:space:]]*=/ { print "version = \"" new "\""; done = 1; next }
  { print }
' Cargo.toml >"$TMP_TOML"
# 保持原文件权限位（mktemp 建的是 0600）。
chmod --reference=Cargo.toml "$TMP_TOML"
mv "$TMP_TOML" Cargo.toml

# 2) 重解 Cargo.lock：`cargo metadata` 会按新 manifest 重写本地成员条目（不需要网络）。
echo "[set-version] re-resolve Cargo.lock"
cargo metadata --format-version 1 >/dev/null

# 3) 重生成 docs/openapi.json（info.version 来自 env!("CARGO_PKG_VERSION")）。
echo "[set-version] regenerate docs/openapi.json"
CRABMATE_BLESS=1 cargo test --lib web::openapi::tests::openapi_docs_snapshot_matches_spec

# 4) 重生成两个 man 页（版本串来自同一个 env!("CARGO_PKG_VERSION")）。
echo "[set-version] regenerate man/crabmate.1 + man/crabmatectl.1"
cargo run --quiet --features gen-man --bin crabmate-gen-man

# 5) CHANGELOG 骨架：标题必须用 **core** 版本，否则 release.yml 的
#    `^## \[<core>\]` 抽不到正文，Release body 静默退化成 stub。
PREV_TAG="$(git describe --tags --abbrev=0 2>/dev/null || true)"
# 重跑同一个版本（只重生成派生文件）时 `describe` 会指向刚切的那个 tag，
# 那样 compare 链接会变成 `v0.8.1-alpha.0...v0.8.1-alpha.0`——退回它的上一个 tag。
if [[ "$PREV_TAG" == "v${NEW_VER}" ]]; then
  PREV_TAG="$(git describe --tags --abbrev=0 "v${NEW_VER}^" 2>/dev/null || true)"
fi
cat <<EOF

[set-version] 下一步（CHANGELOG 正文仍由人写）：

  1) CHANGELOG.md 分区标题（core=${CORE_VER}，**不要**带 -pre 后缀）：
       ## [${CORE_VER}] - $(date +%F)

  2) CHANGELOG.md 底部链接引用行：
EOF
if [[ -n "$PREV_TAG" ]]; then
  echo "       [${CORE_VER}]: https://github.com/noisystreet/CrabMate/compare/${PREV_TAG}...v${NEW_VER}"
else
  echo "       [${CORE_VER}]: https://github.com/noisystreet/CrabMate/compare/<previous-tag>...v${NEW_VER}"
fi
cat <<EOF

  3) git tag：v${NEW_VER}（tag 的 core 必须等于 Cargo.toml 的 ${CORE_VER}）

[set-version] verify（与 pre-commit / ci.yml 同一个守卫）
EOF
bash scripts/check-version-sync.sh

echo
echo "[set-version] 已改动的文件（未提交）："
git status --short -- Cargo.toml Cargo.lock docs/openapi.json man/ || true
