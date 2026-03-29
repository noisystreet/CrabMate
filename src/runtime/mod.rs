//! 命令行运行时，以及与 `api` 共用的终端侧能力（`latex_unicode`）、会话文件/导出（`chat_export`）；**`cli_wait_spinner`** 为可选首包等待动效（**`AGENT_CLI_WAIT_SPINNER`**，由 **`llm::api::stream_chat`** 触发）。
//! `benchmark` 子模块提供批量无人值守测评能力（SWE-bench / GAIA / HumanEval 等）。
pub mod benchmark;
pub mod chat_export;
pub mod cli;
pub(crate) mod cli_approval;
pub mod cli_doctor;
pub mod cli_exit;
pub(crate) mod cli_mcp;
pub(crate) mod cli_repl_ui;
pub(crate) mod cli_wait_spinner;
pub mod latex_unicode;
pub(crate) mod message_display;
pub(crate) mod plan_section;
pub(crate) mod repl_reedline;
pub(crate) mod terminal_cli_transcript;
pub(crate) mod terminal_labels;
pub mod workspace_session;
