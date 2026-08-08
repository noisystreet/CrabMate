#!/usr/bin/env bash
# Client 契约发版门禁（路径 A Phase 1）：SSE 金样 + OpenAPI 冒烟 + 外仓风格 path 消费。
# 权威说明：docs/design/client_contract_versioning.md
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "[client-contract] SSE goldens (scripts/check-sse-protocol.sh)"
bash scripts/check-sse-protocol.sh

echo "[client-contract] OpenAPI schemars (crabmate-api-contract)"
cargo test -p crabmate-api-contract openapi -- --nocapture

echo "[client-contract] OpenAPI core paths (crabmate)"
cargo test -p crabmate openapi_spec_has_core_paths_and_version -- --nocapture

echo "[client-contract] external-style path consumer (api-contract + sse-protocol)"
TMP="$(mktemp -d)"
cleanup() {
  rm -rf "$TMP"
}
trap cleanup EXIT

mkdir -p "$TMP/consumer/src"
cat >"$TMP/consumer/Cargo.toml" <<EOF
[package]
name = "crabmate-contract-consumer-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
crabmate-api-contract = { path = "$ROOT/crates/crabmate-api-contract" }
crabmate-sse-protocol = { path = "$ROOT/crates/crabmate-sse-protocol" }
EOF

cat >"$TMP/consumer/src/lib.rs" <<'EOF'
//! 模拟壳仓 / UI 仓：仅 path（或将来 git tag）依赖契约 crate，不加入 CrabMate workspace members。

pub fn smoke_sse_protocol_version() -> u8 {
    let _ = crabmate_api_contract::error_codes::SSE_PROTOCOL_MISMATCH;
    let _ = crabmate_api_contract::error_codes::SSE_CLIENT_TOO_NEW;
    let _ = crabmate_api_contract::error_codes::INVALID_SSE_CLIENT_PROTOCOL;
    crabmate_sse_protocol::SSE_PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn consumer_sees_stable_protocol_constant_and_error_codes() {
        assert!(super::smoke_sse_protocol_version() >= 1);
    }
}
EOF

cargo test --manifest-path "$TMP/consumer/Cargo.toml" -- --nocapture

# crabmate-connect 仅在 Client 仓（路径 A Phase 4）；本门禁不再校验主仓副本。

echo "[client-contract] ok"
