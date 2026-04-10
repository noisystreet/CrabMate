#!/usr/bin/env bash
# 一键发布打包：release 构建 + 前端 trunk --release + man + tar.gz；在 Linux 上另生成 .deb（需 cargo-deb）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
用法: scripts/package-release.sh [选项]

  --skip-frontend   跳过 frontend-leptos 的 trunk build（需已有 dist/）
  --skip-man        跳过 crabmate-gen-man
  --skip-tar        不生成 tar.gz
  --skip-deb        不生成 .deb
  -h, --help        显示本说明

产物目录: dist/
  - crabmate_<version>_<os>_<arch>.tar.gz
  - crabmate_<version>_<arch>.deb（仅 Linux 且未 --skip-deb、且已安装 cargo-deb）

依赖: Rust、trunk、wasm32 目标；.deb 需 cargo install cargo-deb
EOF
}

SKIP_FRONTEND=0
SKIP_MAN=0
SKIP_TAR=0
SKIP_DEB=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --skip-frontend) SKIP_FRONTEND=1 ;;
    --skip-man) SKIP_MAN=1 ;;
    --skip-tar) SKIP_TAR=1 ;;
    --skip-deb) SKIP_DEB=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "未知参数: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

if ! command -v cargo >/dev/null 2>&1; then
  echo "错误: 未找到 cargo，请先安装 Rust 工具链。" >&2
  exit 1
fi

VERSION="$(grep -m1 '^version' Cargo.toml | sed -E 's/^version[[:space:]]*=[[:space:]]*"([^"]+)".*/\1/')"
if [[ -z "$VERSION" ]]; then
  echo "错误: 无法从 Cargo.toml 解析 version。" >&2
  exit 1
fi

OS_RAW="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH_RAW="$(uname -m | tr '[:upper:]' '[:lower:]')"
# 常见别名，便于 tarball 命名
case "$ARCH_RAW" in
  aarch64 | arm64) ARCH_RAW="aarch64" ;;
  x86_64 | amd64) ARCH_RAW="x86_64" ;;
esac

STAGE_PARENT="$(mktemp -d "${TMPDIR:-/tmp}/crabmate-pkg.XXXXXX")"
STAGE_NAME="crabmate-${VERSION}-${OS_RAW}-${ARCH_RAW}"
STAGE_DIR="${STAGE_PARENT}/${STAGE_NAME}"
mkdir -p "$STAGE_DIR"

cleanup() {
  rm -rf "$STAGE_PARENT"
}
trap cleanup EXIT

echo "==> 版本: ${VERSION} (${OS_RAW}-${ARCH_RAW})"

if [[ "$SKIP_MAN" -eq 0 ]]; then
  echo "==> 生成 man 页 (crabmate-gen-man)"
  cargo run --quiet --bin crabmate-gen-man
else
  echo "==> 跳过 man 生成"
fi

if [[ "$SKIP_FRONTEND" -eq 0 ]]; then
  echo "==> 前端 trunk build --release"
  if ! command -v trunk >/dev/null 2>&1; then
    echo "错误: 未找到 trunk。请安装: https://trunkrs.dev/ 或 cargo install trunk" >&2
    exit 1
  fi
  (cd frontend-leptos && trunk build --release)
else
  echo "==> 跳过前端构建"
  if [[ ! -d frontend-leptos/dist ]]; then
    echo "错误: frontend-leptos/dist 不存在，请去掉 --skip-frontend 或先手动 trunk build。" >&2
    exit 1
  fi
fi

echo "==> cargo build --release -p crabmate"
cargo build --release -p crabmate

if [[ ! -f target/release/crabmate ]]; then
  echo "错误: 未找到 target/release/crabmate" >&2
  exit 1
fi

mkdir -p dist

if [[ "$SKIP_TAR" -eq 0 ]]; then
  echo "==> 组装 tar 内容"
  cp target/release/crabmate "$STAGE_DIR/"
  chmod 755 "$STAGE_DIR/crabmate"
  [[ -f LICENSE ]] && cp LICENSE "$STAGE_DIR/"
  [[ -f README.md ]] && cp README.md "$STAGE_DIR/"
  cp config.toml.example "$STAGE_DIR/"
  cp -R config "$STAGE_DIR/"
  mkdir -p "$STAGE_DIR/man"
  cp man/crabmate.1 "$STAGE_DIR/man/"
  mkdir -p "$STAGE_DIR/frontend-leptos"
  cp -R frontend-leptos/dist "$STAGE_DIR/frontend-leptos/"

  TAR_NAME="crabmate_${VERSION}_${OS_RAW}_${ARCH_RAW}.tar.gz"
  TAR_PATH="dist/${TAR_NAME}"
  echo "==> 写入 ${TAR_PATH}"
  tar -czf "$TAR_PATH" -C "$STAGE_PARENT" "$STAGE_NAME"
  echo "    完成: ${TAR_PATH}"
else
  echo "==> 跳过 tar.gz"
fi

if [[ "$SKIP_DEB" -eq 0 ]] && [[ "$OS_RAW" == "linux" ]]; then
  if cargo deb --version >/dev/null 2>&1; then
    echo "==> cargo deb"
    cargo deb
    shopt -s nullglob
    deb_files=(target/debian/crabmate_*.deb)
    shopt -u nullglob
    if [[ ${#deb_files[@]} -eq 0 ]]; then
      echo "警告: cargo deb 未在 target/debian/ 下产生 .deb，请检查 cargo-deb 输出。" >&2
    else
      for f in "${deb_files[@]}"; do
        base="$(basename "$f")"
        cp "$f" "dist/${base}"
        echo "    完成: dist/${base}"
      done
    fi
  else
    echo "提示: 未安装 cargo-deb，已跳过 .deb。安装: cargo install cargo-deb" >&2
  fi
elif [[ "$SKIP_DEB" -ne 0 ]]; then
  echo "==> 跳过 .deb"
else
  echo "==> 非 Linux 环境，跳过 .deb（deb 包仅在 Linux 上构建）"
fi

echo "==> 全部完成。输出目录: dist/"
