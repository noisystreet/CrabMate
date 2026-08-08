#!/usr/bin/env bash
set -euo pipefail

# SSE 协议回归检查：
# 1) crabmate-sse-protocol 协议单测
# 2) 共享分类器金样（control_classify / fixtures/sse_control_golden.jsonl）
# 3) 前端 AG-UI V2 金样（fixtures/sse_ag_ui_golden.jsonl）
#
# 用法：
#   ./scripts/check-sse-protocol.sh
# 汇总门禁（含 OpenAPI / 外仓消费）：./scripts/check-client-contract.sh

echo "[sse-check] cargo test -p crabmate-sse-protocol sse::protocol::tests"
cargo test -p crabmate-sse-protocol sse::protocol::tests -- --nocapture

echo "[sse-check] cargo test -p crabmate-sse-protocol golden_sse_control"
cargo test -p crabmate-sse-protocol golden_sse_control -- --nocapture

echo "[sse-check] cd frontend && cargo test golden_ag_ui_v2_parser_matches_expected"
(cd frontend && cargo test golden_ag_ui_v2_parser_matches_expected -- --nocapture)

echo "[sse-check] done"
