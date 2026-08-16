//! CrabMate **HTTP JSON 契约**（纯 `serde`；无 axum / Leptos）。
//!
//! - **服务端**：`serve` 的 handler（`web` / `cm_web_host`）使用本模块类型。
//! - **前端 WASM**：Client 仓钉 **`crabmate`** + `features = ["protocol"]`，避免手写重复 DTO。
//! - **OpenAPI**：[`openapi_component_schemas`] 从本模块的 `schemars` 定义生成。
//!
//! Semver / 外仓钉法：仓库 **`docs/design/client_contract_versioning.md`**、
//! **`docs/design/crates_io_single_package.md`**。

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
