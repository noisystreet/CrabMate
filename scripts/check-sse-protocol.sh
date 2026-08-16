#!/usr/bin/env bash
set -euo pipefail

# SSE 协议回归检查（单包后：`crabmate` + `protocol` / default `server`）：
# 1) protocol 分类/帧单测（无 tokio）
# 2) 共享分类器金样（control_classify / fixtures/sse_control_golden.jsonl）
# 3) AG-UI classify 金样（fixtures/sse_ag_ui_golden.jsonl；V2 parser 在 Client 仓 frontend）
# 4) HTTP/SSE 失败路径契约金样（fixtures/http_sse_failure_path_golden.jsonl）
#
# 用法：
#   ./scripts/check-sse-protocol.sh
# 汇总门禁（含 OpenAPI / 外仓消费）：./scripts/check-client-contract.sh

echo "[sse-check] cargo test --lib --no-default-features --features protocol cm_sse_protocol"
cargo test --lib --no-default-features --features protocol cm_sse_protocol -- --nocapture

echo "[sse-check] protocol graph has no tokio"
if cargo tree --no-default-features --features protocol -e normal --prefix none \
  | grep -qE '^tokio '; then
  echo "FORBIDDEN: crabmate --no-default-features --features protocol must not depend on tokio" >&2
  cargo tree --no-default-features --features protocol -e normal -i tokio --prefix indent | head -40
  exit 1
fi
echo "  ok: no tokio"

echo "[sse-check] cargo test --lib golden_sse_control"
cargo test --lib golden_sse_control -- --nocapture

echo "[sse-check] cargo test --lib golden_ag_ui_classify_matches_expected"
cargo test --lib golden_ag_ui_classify_matches_expected -- --nocapture

echo "[sse-check] wasm32 protocol (no server)"
if rustup target list --installed 2>/dev/null | grep -qx 'wasm32-unknown-unknown'; then
  cargo check --no-default-features --features protocol --target wasm32-unknown-unknown --lib
else
  echo "  skip: rustup target wasm32-unknown-unknown not installed"
fi

echo "[sse-check] cargo test --lib golden_http_sse_failure_path"
cargo test --lib golden_http_sse_failure_path -- --nocapture

echo "[sse-check] done"
