//! CrabMate **HTTP JSON 契约**（纯 `serde`；无 axum / Leptos）。
//!
//! - **服务端**：[`crabmate-web-host`] 再导出本 crate 类型供 handler 使用。
//! - **前端 WASM**：Client 仓 **`crabmate-web`** 经 git/path 依赖本 crate，避免手写重复 DTO。
//! - **OpenAPI**：[`openapi_component_schemas`] 从本 crate 的 `schemars` 定义生成。
//!
//! Semver / 外仓 git tag 钉法：仓库 **`docs/design/client_contract_versioning.md`**。

pub mod api;
pub mod chat;
pub mod chat_keys;
pub mod error_codes;
pub mod openapi;
pub mod status;
pub mod web_ui;

pub use api::ApiError;
pub use status::StatusShellView;
pub use web_ui::WebUiConfigResponse;
