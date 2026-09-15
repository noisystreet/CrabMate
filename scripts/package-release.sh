#!/usr/bin/env bash
# 一键发布打包：release 构建 + man + tar.gz；在 Linux 上另生成 .deb（需 cargo-deb）。
# 本仓包为 server-only（不附带 UI）；serve 永远纯 API，UI 由 Client 仓自行托管。
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
用法: scripts/package-release.sh [选项]

  --skip-man            跳过 crabmate-gen-man
  --skip-tar            不生成 tar.gz
  --skip-deb            不生成 .deb
  -h, --help            显示本说明

产物目录: dist/
  - crabmate_<version>_<os>_<arch>.tar.gz（含 systemd/ 与 etc/crabmate/ 布局）
  - crabmate_<version>_<arch>.deb（仅 Linux；含 crabmate.service，默认不 enable）

依赖: Rust；.deb 需 cargo install cargo-deb
业务 UI：在 ../crabmate-client 构建与托管（serve 永远纯 API，不托管 UI）
EOF
}

SKIP_MAN=0
SKIP_TAR=0
SKIP_DEB=0

while [[ $# -gt 0 ]]; do
  case "$1" in
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
  cargo run --quiet --features gen-man --bin crabmate-gen-man
else
  echo "==> 跳过 man 生成"
fi

echo "==> server-only 打包（不附带 UI；serve 永远纯 API）"

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
  mkdir -p "$STAGE_DIR/systemd"
  cp packaging/systemd/crabmate.service "$STAGE_DIR/systemd/"
  cp packaging/systemd/crabmate.env.example "$STAGE_DIR/systemd/"
  # Deb-compatible /etc layout for systemd unit (--config /etc/crabmate/config.toml)
  mkdir -p "$STAGE_DIR/etc/crabmate/config/prompts"
  cp packaging/etc/config.toml "$STAGE_DIR/etc/crabmate/config.toml"
  cp packaging/systemd/crabmate.env.example "$STAGE_DIR/etc/crabmate/crabmate.env.example"
  [[ -f config/agent_roles.toml ]] && cp config/agent_roles.toml "$STAGE_DIR/etc/crabmate/"
  cp config/prompts/*.md "$STAGE_DIR/etc/crabmate/config/prompts/"

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
