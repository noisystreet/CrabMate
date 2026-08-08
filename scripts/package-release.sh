#!/usr/bin/env bash
# 一键发布打包：release 构建 + man + tar.gz；在 Linux 上另生成 .deb（需 cargo-deb）。
# 业务 UI 源码已迁 Client 仓；可选随包附带已构建 dist（CM_WEB_STATIC_DIR 或 --frontend-dist）。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
用法: scripts/package-release.sh [选项]

  --frontend-dist DIR   将已构建 UI dist 打入包内 frontend/dist（默认读 CM_WEB_STATIC_DIR）
  --skip-frontend       不附带 UI 产物（server-only；与未设置 dist 相同）
  --skip-man            跳过 crabmate-gen-man
  --skip-tar            不生成 tar.gz
  --skip-deb            不生成 .deb
  -h, --help            显示本说明

产物目录: dist/
  - crabmate_<version>_<os>_<arch>.tar.gz
  - crabmate_<version>_<arch>.deb（仅 Linux 且未 --skip-deb、且已安装 cargo-deb）

依赖: Rust；.deb 需 cargo install cargo-deb
业务 UI：在 ../crabmate-client 执行 make frontend，再传 --frontend-dist 或 CM_WEB_STATIC_DIR
EOF
}

SKIP_FRONTEND=0
SKIP_MAN=0
SKIP_TAR=0
SKIP_DEB=0
FRONTEND_DIST="${CM_WEB_STATIC_DIR:-}"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --frontend-dist)
      shift
      FRONTEND_DIST="${1:-}"
      [[ -n "${FRONTEND_DIST}" ]] || { echo "错误: --frontend-dist 需要目录参数" >&2; exit 2; }
      ;;
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

INCLUDE_UI=0
if [[ "$SKIP_FRONTEND" -eq 0 && -n "${FRONTEND_DIST}" && -f "${FRONTEND_DIST}/index.html" ]]; then
  INCLUDE_UI=1
  echo "==> 将附带 UI dist: ${FRONTEND_DIST}"
elif [[ "$SKIP_FRONTEND" -eq 0 && -n "${FRONTEND_DIST}" ]]; then
  echo "错误: FRONTEND_DIST/CM_WEB_STATIC_DIR 无效（缺 index.html）: ${FRONTEND_DIST}" >&2
  exit 1
else
  echo "==> server-only（未附带 UI；运行时用 --no-web 或 CM_WEB_STATIC_DIR）"
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
  cp -R config "$STAGE_DIR/"
  mkdir -p "$STAGE_DIR/man"
  cp man/crabmate.1 "$STAGE_DIR/man/"
  if [[ "$INCLUDE_UI" -eq 1 ]]; then
    mkdir -p "$STAGE_DIR/frontend"
    cp -R "${FRONTEND_DIST}" "$STAGE_DIR/frontend/dist"
  fi

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
