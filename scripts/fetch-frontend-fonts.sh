#!/usr/bin/env bash
# 拉取 DM Sans / JetBrains Mono 子集（OFL），供离线桌面与无 Google Fonts CDN 的 Web UI。
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dest="${root}/frontend/fonts"
mkdir -p "${dest}"

base="https://cdn.jsdelivr.net/fontsource/fonts"

fetch() {
  local url="$1"
  local out="$2"
  if [[ -f "${out}" ]]; then
    return 0
  fi
  echo "fetch ${out##*/}"
  curl -fsSL "${url}" -o "${out}"
}

# fontsource@5 拉丁子集（与 tokens.css 字重对齐）
fetch "${base}/dm-sans@5.2.5/latin-400-normal.woff2" "${dest}/dm-sans-latin-400-normal.woff2"
fetch "${base}/dm-sans@5.2.5/latin-500-normal.woff2" "${dest}/dm-sans-latin-500-normal.woff2"
fetch "${base}/dm-sans@5.2.5/latin-600-normal.woff2" "${dest}/dm-sans-latin-600-normal.woff2"
fetch "${base}/dm-sans@5.2.5/latin-700-normal.woff2" "${dest}/dm-sans-latin-700-normal.woff2"
fetch "${base}/jetbrains-mono@5.2.5/latin-400-normal.woff2" "${dest}/jetbrains-mono-latin-400-normal.woff2"
fetch "${base}/jetbrains-mono@5.2.5/latin-500-normal.woff2" "${dest}/jetbrains-mono-latin-500-normal.woff2"

echo "fonts ready under ${dest}"
