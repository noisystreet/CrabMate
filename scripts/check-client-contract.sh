#!/usr/bin/env bash
# Client 契约发版门禁（路径 A Phase 1 + Phase A）：
# SSE 金样 + OpenAPI 冒烟 + 外仓风格 path 消费（含官方 UI 展示契约 crate）。
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

echo "[client-contract] pin-list crate manifests (frontend migrate Phase A)"
for crate in \
  crabmate-api-contract \
  crabmate-sse-protocol \
  crabmate-types \
  crabmate-display-rules \
  crabmate-turn-layout \
  crabmate-tool-card \
  crabmate-chat-export
do
  manifest="$ROOT/crates/$crate/Cargo.toml"
  test -f "$manifest"
  grep -q "^name = \"$crate\"$" "$manifest"
  grep -q '^version = "' "$manifest"
  echo "  ok $crate"
done

echo "[client-contract] external-style path consumer (UI pin set)"
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
crabmate-types = { path = "$ROOT/crates/crabmate-types" }
crabmate-display-rules = { path = "$ROOT/crates/crabmate-display-rules" }
crabmate-turn-layout = { path = "$ROOT/crates/crabmate-turn-layout" }
crabmate-tool-card = { path = "$ROOT/crates/crabmate-tool-card" }
crabmate-chat-export = { path = "$ROOT/crates/crabmate-chat-export" }
EOF

cat >"$TMP/consumer/src/lib.rs" <<'EOF'
//! 模拟壳仓 / UI 仓：仅 path（或将来 git tag）依赖契约 crate，不加入 CrabMate workspace members。

pub fn smoke_sse_protocol_version() -> u8 {
    let _ = crabmate_api_contract::error_codes::SSE_PROTOCOL_MISMATCH;
    let _ = crabmate_api_contract::error_codes::SSE_CLIENT_TOO_NEW;
    let _ = crabmate_api_contract::error_codes::INVALID_SSE_CLIENT_PROTOCOL;
    let _ = crabmate_display_rules::user_message_should_hide_for_chat_display("");
    let _ = crabmate_tool_card::looks_like_crabmate_tool_envelope("");
    let _ = crabmate_chat_export::CHAT_EXPORT_SCHEMA_VERSION;
    let _ = crabmate_turn_layout::Turn::default();
    let _ = crabmate_types::OPENAI_CHAT_COMPLETIONS_REL_PATH;
    crabmate_sse_protocol::SSE_PROTOCOL_VERSION
}

#[cfg(test)]
mod tests {
    #[test]
    fn consumer_sees_stable_protocol_and_ui_contract_symbols() {
        assert!(super::smoke_sse_protocol_version() >= 1);
    }
}
EOF

cargo test --manifest-path "$TMP/consumer/Cargo.toml" -- --nocapture

# crabmate-connect 仅在 Client 仓（路径 A Phase 4）；本门禁不再校验主仓副本。

echo "[client-contract] ok"
