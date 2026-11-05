//! CrabMate 库：OpenAI 兼容多供应商 LLM、Agent 主循环、HTTP 服务、工具与工作流。
//! 二进制入口见 `src/main.rs` 的 [`run`] 包装。
//!
//! # 公开 API（semver）
//!
//! 白名单见仓库 `docs/design/crates_io_single_package.md` §2.4。
//!
//! - **承诺 `protocol`**（Client / WASM）：六个模块 [`cm_types`]、[`cm_display_rules`]、
//!   [`cm_api_contract`]、[`cm_chat_export`]、[`cm_turn_layout`]、[`cm_sse_protocol`]。
//!   **没有** `types` / `sse` / `config` 别名。增量公开面（`/health` 视图
//!   `HealthReportView`、sessions 行瘦投影 `SessionListRow` 与 `/workspace*` 契约
//!   模块 `workspace` 挂 [`cm_api_contract`]；AG-UI `command_approval` data 面
//!   `CommandApprovalData` 挂 [`cm_sse_protocol`]）见 §2.4「`0.5.x` / `0.6.x` / `0.7.x` 增量公开面」。
//! - **承诺 `server`**（默认，含 `protocol`）：组合面模块名 `agent` / `config` / `llm` / `sse` /
//!   `types` 的**存在**；以及 `run`、`run_agent_turn`、`build_tools*`、`ProcessHandles`、
//!   `tool_sandbox` 等根上显式 `pub use`。
//! - **不承诺**：`#[doc(hidden)]` 的 `cm_agent` / `cm_llm` / `cm_config` / `cm_workflow` /
//!   `cm_internal`、`e2e_scenario`、`test_serve`；`agent::agent_turn` 等组合面内部路径。
//!   `cm_tools` / `cmd_mate` 等为实现模块，`pub(crate)`。
//!
//! 本 crate 默认 feature 是带库的 **`serve` 服务器**，不是通用嵌入式 Agent SDK。
//! HTTP 线契约以 `docs/SSE协议.md`、`cm_api_contract` 与 `GET /openapi.json` 为准。
//!
//! 日志由 **`tracing`** 处理；**`observability::init_tracing_subscriber`**（`cm_internal`）安装 **`tracing-subscriber`** 并用 **`tracing-log`** 桥接既有 `log::` 调用。`RUST_LOG` 优先。未设置时：`serve` 默认 **info**；其它 CLI 模式默认 **warn**（不输出 info）；`--log <FILE>` 在未设置 `RUST_LOG` 时默认 **info**。时间戳默认**本机本地时区**（RFC3339）。设 **`CM_LOG_JSON=1`** 时输出 JSON 行（便于 `jq` / 日志平台）。

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

/// `conversation_id` 持久化（可选 SQLite）与 `SaveConversationOutcome`。
///
/// 挂在 **`sqlite`** 而非 `web` / `server` 之下，且**不**随 `lib_server.rs` 一起编译：
/// 存储层只需要 `protocol` 的类型（`cm_types` / `cm_api_contract`）加 `rusqlite`，
/// 与 Axum、回合编排都无关（S1-b，见 #1278）。`web = ["sqlite"]` 保证既有构建不变。
#[cfg(feature = "sqlite")]
mod conversation_store;

/// S2-2：SDK 面 `TurnEvent` 的有损投影（设计 `docs/design/sdk_facade.md` §5.1.1）。
pub(crate) mod cm_sdk_event;
/// 会话存储端口（`SessionStore`）与内存 / SQLite 实现（S1-b，见 `docs/design/sdk_facade.md` §5.3）。
///
/// 与 `conversation_store` 的分层：后者是纯 SQLite 语句层（只挂 `sqlite`，不依赖 tokio /
/// 回合编排），本模块是**门面 + 端口**，需要 `tokio`、`cm_turn_layout` 与 `cm_api_contract`
/// 的类型，因此挂 `server`。搬出 `web/` 的收益是运维 CLI 与 Web 共用同一入口，
/// 而不是「能让它在 `--features protocol,sqlite` 下编译」。
#[cfg(feature = "server")]
pub(crate) mod cm_session_store;

#[cfg(feature = "server")]
include!("lib_server.rs");

/// 金样 fixture 的公共读取工具（仅测试编译）。
///
/// 随 `protocol` 一起 gate：本模块的辅助依赖 `cm_types`（受 `protocol` 门控）
/// 与 `cm_tools` / `cm_agent`（`server` 专属）。两个 feature 都不开时它没有任何
/// 使用者，编进去只会得到一组 `dead_code` 警告。
#[cfg(all(test, feature = "protocol"))]
mod test_fixtures;
