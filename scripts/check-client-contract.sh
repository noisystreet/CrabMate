#!/usr/bin/env bash
# Client 契约发版门禁（路径 A Phase 1 + Phase A）：
# SSE 金样 + OpenAPI 冒烟 + 外仓风格 path 消费（单包 `crabmate` + `protocol`）。
# 权威说明：docs/design/client_contract_versioning.md 、 docs/design/crates_io_single_package.md
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

echo "[client-contract] SSE goldens (scripts/check-sse-protocol.sh)"
bash scripts/check-sse-protocol.sh

echo "[client-contract] OpenAPI schemars (cm_api_contract)"
cargo test --lib openapi -- --nocapture

echo "[client-contract] OpenAPI core paths (crabmate)"
cargo test --lib openapi_spec_has_core_paths_and_version -- --nocapture

echo "[client-contract] single-crate protocol feature (Cargo.toml)"
grep -q '^name = "crabmate"$' "$ROOT/Cargo.toml"
grep -q '^protocol = \[\]$' "$ROOT/Cargo.toml"
test -d "$ROOT/src/cm_sse_protocol"
test -d "$ROOT/src/cm_api_contract"
test -d "$ROOT/src/cm_types"
test -d "$ROOT/src/cm_display_rules"
test -d "$ROOT/src/cm_turn_layout"
test -d "$ROOT/src/cm_chat_export"
echo "  ok crabmate protocol modules"

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
crabmate = { path = "$ROOT", default-features = false, features = ["protocol"] }
EOF

cat >"$TMP/consumer/src/lib.rs" <<'EOF'
//! 模拟壳仓 / UI 仓：只依赖 `crabmate` + `protocol`，不加入 CrabMate workspace members。

pub fn smoke_sse_protocol_version() -> u8 {
    let _ = crabmate::cm_api_contract::error_codes::SSE_PROTOCOL_MISMATCH;
    let _ = crabmate::cm_api_contract::error_codes::SSE_CLIENT_TOO_NEW;
    let _ = crabmate::cm_api_contract::error_codes::INVALID_SSE_CLIENT_PROTOCOL;
    let _ = crabmate::cm_display_rules::user_message_should_hide_for_chat_display("");
    let _ = crabmate::cm_chat_export::CHAT_EXPORT_SCHEMA_VERSION;
    let _ = crabmate::cm_turn_layout::Turn::default();
    let _ = crabmate::cm_types::OPENAI_CHAT_COMPLETIONS_REL_PATH;
    let _ = crabmate::cm_sse_protocol::classify_sse_control_outcome;
    crabmate::cm_sse_protocol::SSE_PROTOCOL_VERSION
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

assert_cargo_check_fails_unresolved() {
  local dir="$1"
  local label="$2"
  local log="$TMP/${dir}_check.log"
  if cargo check --manifest-path "$TMP/$dir/Cargo.toml" >"$log" 2>&1; then
    echo "FORBIDDEN: $label compiled but must be unresolved" >&2
    cat "$log" >&2
    exit 1
  fi
  if ! grep -qE 'could not find `(types|sse|cmd_mate|cm_tools)`|unresolved import|is private' "$log"; then
    echo "unexpected cargo check failure ($label):" >&2
    cat "$log" >&2
    exit 1
  fi
  echo "  ok $label is not public"
}

echo "[client-contract] protocol-only must not export types/sse aliases"
mkdir -p "$TMP/alias_leak/src"
cat >"$TMP/alias_leak/Cargo.toml" <<EOF
[package]
name = "crabmate-alias-leak-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
crabmate = { path = "$ROOT", default-features = false, features = ["protocol"] }
EOF
cat >"$TMP/alias_leak/src/lib.rs" <<'EOF'
pub fn leak_server_aliases() {
    let _ = crabmate::types::OPENAI_CHAT_COMPLETIONS_REL_PATH;
    let _ = crabmate::sse::protocol::SsePayload;
}
EOF
assert_cargo_check_fails_unresolved alias_leak "protocol-only types/sse aliases"

echo "[client-contract] server must not export crate-private domain modules"
mkdir -p "$TMP/internal_leak/src"
cat >"$TMP/internal_leak/Cargo.toml" <<EOF
[package]
name = "crabmate-internal-leak-smoke"
version = "0.0.0"
edition = "2024"
publish = false

[dependencies]
crabmate = { path = "$ROOT" }
EOF
cat >"$TMP/internal_leak/src/lib.rs" <<'EOF'
pub fn leak_crate_private_modules() {
    let _ = crabmate::cmd_mate::split_command_line;
    let _ = crabmate::cm_tools::redact::single_line_preview;
}
EOF
assert_cargo_check_fails_unresolved internal_leak "server cmd_mate/cm_tools"

echo "[client-contract] ok"
