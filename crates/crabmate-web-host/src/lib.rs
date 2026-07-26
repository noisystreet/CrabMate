//! Axum Web **宿主** crate：HTTP 契约、`GET /web-ui`、serve 静态挂载壳。
//!
//! **不是** Leptos WASM 前端包（workspace 成员名仍为 **`crabmate-web`** / `frontend/`）。
//! 依赖策略见 `docs/design/web_host_extract.md`：本 crate **不得**依赖 `crabmate-internal`。
//!
//! ## 阶段 B / C（边界说明）
//! - **B**：HTTP DTO / `chat_keys` / `limits` / `web_ui` 在本 crate；带 `AppState` 的 handler 因
//!   axum `FromRef` 孤儿规则仍在根包。
//! - **C**：根包 `build_app` 只装配路由与 `AppState`，静态挂载与体积分层调用 [`serve`]。
//! - 回合队列 / `run_agent_turn` 留在根包，避免 `web-host ↔ crabmate` 循环依赖。

pub mod http_types;
pub mod routes;
pub mod serve;
pub mod web_ui;

pub use serve::{
    PROTECTED_API_BODY_LIMIT_BYTES, layer_protected_body_limit, mount_uploads_and_spa,
};

/// 包身份（门禁/诊断用）。
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[cfg(test)]
mod tests {
    #[test]
    fn crate_name_is_web_host() {
        assert_eq!(super::crate_name(), "crabmate-web-host");
    }
}
