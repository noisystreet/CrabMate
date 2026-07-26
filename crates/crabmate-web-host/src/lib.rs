//! Axum Web **宿主** crate（HTTP 契约、部分无状态 handler / routes；后续 handlers 下沉目标）。
//!
//! **不是** Leptos WASM 前端包（workspace 成员名仍为 **`crabmate-web`** / `frontend/`）。
//! 依赖策略见 `docs/design/web_host_extract.md`：本 crate **不得**依赖 `crabmate-internal`。

pub mod http_types;
pub mod routes;
pub mod web_ui;

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
