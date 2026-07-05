#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
desktop_root="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${desktop_root}/.." && pwd)"

target_triple="$(rustc -vV | sed -n 's/^host: //p')"
if [[ -z "${target_triple}" ]]; then
  echo "failed to detect rust target triple" >&2
  exit 1
fi

if [[ -n "${CM_DESKTOP_BACKEND_BIN:-}" ]]; then
  source_bin="${CM_DESKTOP_BACKEND_BIN}"
else
  # Prefer release build, then gracefully fall back to debug for local packaging convenience.
  if [[ -f "${repo_root}/target/release/crabmate" ]]; then
    source_bin="${repo_root}/target/release/crabmate"
  elif [[ -f "${repo_root}/target/debug/crabmate" ]]; then
    source_bin="${repo_root}/target/debug/crabmate"
  else
    echo "backend binary not found in:" >&2
    echo "  - ${repo_root}/target/release/crabmate" >&2
    echo "  - ${repo_root}/target/debug/crabmate" >&2
    echo "build backend first (cargo build or cargo build --release) or set CM_DESKTOP_BACKEND_BIN" >&2
    exit 1
  fi
fi

if [[ ! -f "${source_bin}" ]]; then
  echo "backend binary not found: ${source_bin}" >&2
  exit 1
fi

if ! "${source_bin}" serve --help 2>&1 | grep -q 'desktop-ready-json'; then
  echo "backend binary lacks 'serve --desktop-ready-json' (too old for current desktop shell)" >&2
  echo "  source: ${source_bin}" >&2
  echo "rebuild from repo root: cargo build --release" >&2
  echo "then re-run this script or cargo tauri build" >&2
  exit 1
fi

output_dir="${desktop_root}/binaries"
mkdir -p "${output_dir}"

dest_bin="${output_dir}/crabmate-${target_triple}"
cp "${source_bin}" "${dest_bin}"
chmod +x "${dest_bin}"

echo "prepared tauri sidecar: ${dest_bin}"

# Tauri deb 将 ../dist/ 安装到 /usr/share/crabmate/frontend/dist/；sidecar serve 依赖该目录。
dist_src="${repo_root}/frontend/dist"
dist_dest="${desktop_root}/dist"
if [[ ! -f "${dist_src}/index.html" ]]; then
  echo "error: missing ${dist_src}/index.html" >&2
  echo "build frontend first: cd frontend && trunk build --release" >&2
  exit 1
fi
if [[ ! -f "${dist_src}/vendor/ide-codemirror.js" ]]; then
  echo "error: missing ${dist_src}/vendor/ide-codemirror.js (IDE editor bundle)" >&2
  echo "rebuild frontend: cd frontend && trunk build --release" >&2
  exit 1
fi
rm -rf "${dist_dest}"
cp -a "${dist_src}" "${dist_dest}"
echo "synced frontend dist -> ${dist_dest}"

# 启动画面：补充到 dist（Trunk build 不会产出）
splash_src="${desktop_root}/splash.html"
if [[ -f "${splash_src}" ]]; then
  cp "${splash_src}" "${dist_dest}/splash.html"
  echo "copied splash.html -> ${dist_dest}"
fi
