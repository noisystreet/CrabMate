#!/usr/bin/env bash
# 将共用连接页同步到桌面 dist 与移动端入口。
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SRC="${ROOT}/crates/crabmate-connect/assets/connect.html"
if [[ ! -f "${SRC}" ]]; then
  echo "missing ${SRC}" >&2
  exit 1
fi
mkdir -p "${ROOT}/mobile-tauri/dist"
cp "${SRC}" "${ROOT}/mobile-tauri/dist/index.html"
echo "synced connect.html -> mobile-tauri/dist/index.html"
if [[ -d "${ROOT}/desktop-tauri/dist" ]]; then
  cp "${SRC}" "${ROOT}/desktop-tauri/dist/connect.html"
  echo "synced connect.html -> desktop-tauri/dist/connect.html"
fi
