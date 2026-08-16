//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! 日志由 **`tracing`** 处理；**`observability::init_tracing_subscriber`**（`crabmate-internal`）安装 **`tracing-subscriber`** 并用 **`tracing-log`** 桥接既有 `log::` 调用。`RUST_LOG` 优先。未设置时：`--serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

// `web/openapi.rs` 中 `serde_json::json!` 体量较大，默认递归深度不足会无法编译。
#![recursion_limit = "512"]

pub mod cm_api_contract;
pub mod cm_chat_export;
pub mod cm_display_rules;
pub mod cm_sse_protocol;
pub mod cm_turn_layout;
pub mod cm_types;

pub use cm_sse_protocol::sse;
pub use cm_types as types;

#[cfg(feature = "server")]
include!("lib_server.rs");
