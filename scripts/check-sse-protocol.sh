#!/usr/bin/env bash
set -euo pipefail

# SSE 协议回归检查：
# 1) Rust 侧协议属性测试（src/sse/protocol.rs）
# 2) 共享分类器金样与属性测试（crabmate-sse-protocol/control_classify.rs）
#
# 用法：
#   ./scripts/check-sse-protocol.sh
# 如需代理，可先 export http_proxy/https_proxy 再执行。

echo "[sse-check] cargo test -p crabmate sse::protocol::tests"
cargo test -p crabmate sse::protocol::tests -- --nocapture

echo "[sse-check] cargo test -p crabmate-sse-protocol control_classify"
cargo test -p crabmate-sse-protocol control_classify -- --nocapture

echo "[sse-check] done"
