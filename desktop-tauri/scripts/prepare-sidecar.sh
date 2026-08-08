#!/usr/bin/env bash
# 准备桌面壳静态资源：连接页 / 闪屏；可选同步 frontend/dist（deb 文档与本地调试）。
# 桌面壳**不再**打包或校验 crabmate sidecar 二进制。
set -euo pipefail

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
desktop_root="$(cd "${script_dir}/.." && pwd)"
repo_root="$(cd "${desktop_root}/.." && pwd)"

dist_dest="${desktop_root}/dist"
mkdir -p "${dist_dest}"

# 可选：同步 frontend/dist（deb 仍可安装到 /usr/share/crabmate/frontend/dist；壳本身不 serve）
dist_src="${repo_root}/frontend/dist"
if [[ -f "${dist_src}/index.html" ]]; then
  # 保留已有 splash/connect 时用 rsync 式覆盖：先拷 dist 再补壳资源
  rm -rf "${dist_dest}"
  cp -a "${dist_src}" "${dist_dest}"
  echo "synced frontend dist -> ${dist_dest}"
else
  echo "note: missing ${dist_src}/index.html (ok for shell-only; connect/splash still copied)" >&2
  echo "  for full UI assets: cd frontend && trunk build" >&2
fi

# 启动画面
splash_src="${desktop_root}/splash.html"
if [[ -f "${splash_src}" ]]; then
  cp "${splash_src}" "${dist_dest}/splash.html"
  echo "copied splash.html -> ${dist_dest}"
fi

# 桌面/移动共用连接页（源：crates/crabmate-connect/assets）
connect_src="${repo_root}/crates/crabmate-connect/assets/connect.html"
if [[ -f "${connect_src}" ]]; then
  cp "${connect_src}" "${dist_dest}/connect.html"
  echo "copied connect.html -> ${dist_dest}"
else
  echo "error: missing ${connect_src}" >&2
  exit 1
fi

# 兼容旧路径：不再生成 binaries/crabmate-* sidecar
if [[ -d "${desktop_root}/binaries" ]]; then
  echo "note: desktop-tauri/binaries/ is unused (shell does not spawn serve); safe to delete locally" >&2
fi

echo "prepared desktop shell assets in ${dist_dest}"
