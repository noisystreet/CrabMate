//! Web/CLI 共用：外层循环与分阶段规划的运行期参数。

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use crate::config::AgentConfig;
use crate::long_term_memory::LongTermMemoryRuntime;
use crate::tool_registry;
use crate::types::{LlmSeedOverride, Message};

/// Web/CLI 共用：外层循环与分阶段规划注入共用的一套运行期参数。
pub(crate) struct RunLoopParams<'a> {
    pub llm_backend: &'a (dyn crate::llm::ChatCompletionsBackend + 'static),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a Arc<AgentConfig>,
    pub tools_defs: &'a [crate::types::Tool],
    pub messages: &'a mut Vec<Message>,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub effective_working_dir: &'a Path,
    pub workspace_is_set: bool,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub render_to_terminal: bool,
    /// 见 [`crate::llm::api::stream_chat`] 的 `plain_terminal_stream`；仅 CLI 入口为 `true`。
    pub plain_terminal_stream: bool,
    pub web_tool_ctx: Option<&'a tool_registry::WebToolRuntime>,
    /// 与 [`WebExecuteCtx::cli_tool_ctx`] 相同；Web 队列传 `None`。
    pub cli_tool_ctx: Option<&'a tool_registry::CliToolRuntime>,
    pub per_flight: Option<Arc<crate::chat_job_queue::PerTurnFlight>>,
    /// `None` 时使用 `cfg.temperature`。
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    /// 长期记忆运行时（Web 或 CLI）；`None` 时不注入/不索引。
    pub long_term_memory: Option<Arc<LongTermMemoryRuntime>>,
    /// `conversation_id` 或 CLI 固定 `cli`；`None` 时不按会话隔离（跳过记忆）。
    pub long_term_memory_scope_id: Option<String>,
    /// MCP stdio 会话；`None` 时不处理 `mcp__*` 工具名。
    pub mcp_session: Option<Arc<tokio::sync::Mutex<crate::mcp::McpClientSession>>>,
}
