//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! # 公开 API
//!
//! - **`protocol`**（Client / WASM）：仅六个模块 [`cm_types`]、[`cm_display_rules`]、
//!   [`cm_api_contract`]、[`cm_chat_export`]、[`cm_turn_layout`]、[`cm_sse_protocol`]。
//!   **没有** `types` / `sse` / `config` 别名。
//! - **`server`**（默认，含 `protocol`）：组合面 `agent` / `config` / `llm` / `sse` / `types`，
//!   以及 `run`、`run_agent_turn`、`build_tools*`、`ProcessHandles`、`tool_sandbox` 等显式符号。
//!   `cm_agent` / `cm_internal` 等因组合面再导出模块而保持 `pub` + `#[doc(hidden)]`；
//!   `cm_tools` / `cmd_mate` 等为实现模块，`pub(crate)`，不承诺 semver。
//!
//! 日志由 **`tracing`** 处理；**`observability::init_tracing_subscriber`**（`cm_internal`）安装 **`tracing-subscriber`** 并用 **`tracing-log`** 桥接既有 `log::` 调用。`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

#[cfg(feature = "protocol")]
pub mod cm_api_contract;
#[cfg(feature = "protocol")]
pub mod cm_chat_export;
#[cfg(feature = "protocol")]
pub mod cm_display_rules;
#[cfg(feature = "protocol")]
pub mod cm_sse_protocol;
#[cfg(feature = "protocol")]
pub mod cm_turn_layout;
#[cfg(feature = "protocol")]
pub mod cm_types;

#[cfg(feature = "server")]
include!("lib_server.rs");
