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

output_dir="${desktop_root}/binaries"
mkdir -p "${output_dir}"

dest_bin="${output_dir}/crabmate-${target_triple}"
cp "${source_bin}" "${dest_bin}"
chmod +x "${dest_bin}"

echo "prepared tauri sidecar: ${dest_bin}"
