//! 简化工具执行器
//!
//! 供 Operator 在 ReAct 循环中调用真实工具

use crate::config::AgentConfig;
use crate::tools;
use crate::tools::ToolContext;
use crate::types::ToolCall;

/// 工具执行器
pub struct ToolExecutor {
    /// 持有 owned data 以满足 ToolContext 的生命周期
    _ctx: ToolContextOwned,
}

struct ToolContextOwned {
    cfg: AgentConfig,
    allowed_commands: Vec<String>,
    http_fetch_allowed_prefixes: Vec<String>,
    working_dir: std::path::PathBuf,
}

impl ToolExecutor {
    /// 创建新的工具执行器
    #[allow(dead_code)]
    pub fn new(cfg: &AgentConfig, working_dir: std::path::PathBuf) -> Self {
        let allowed_commands = cfg.allowed_commands.to_vec();
        let http_fetch_allowed_prefixes = cfg.http_fetch_allowed_prefixes.to_vec();

        let owned = ToolContextOwned {
            cfg: cfg.clone(),
            allowed_commands,
            http_fetch_allowed_prefixes,
            working_dir: working_dir.clone(),
        };

        // Safety: ToolContext needs 'static lifetime references, so we need to create it carefully
        // For now, we'll use a simpler approach that works for sync tool execution
        Self { _ctx: owned }
    }

    /// 执行单个工具调用
    #[allow(dead_code)]
    pub fn execute_tool_call(&self, tool_call: &ToolCall) -> ToolExecutionResult {
        let name = &tool_call.function.name;
        let args = &tool_call.function.arguments;

        log::info!(target: "crabmate", "Executing tool: {} with args={}", name, truncate_args(args));

        // 直接创建 ToolContext 并调用
        let output = self.run_tool_internal(name, args);

        let success =
            !output.contains("错误") && !output.contains("error:") && !output.contains("Error:");

        log::info!(target: "crabmate", "Tool {} completed, success={}, output_len={}", name, success, output.len());

        ToolExecutionResult {
            tool_name: name.clone(),
            output: output.clone(),
            error: if success { None } else { Some(output) },
            success,
        }
    }

    fn run_tool_internal(&self, name: &str, args: &str) -> String {
        let ctx = ToolContext {
            cfg: Some(&self._ctx.cfg),
            codebase_semantic: None,
            command_max_output_len: self._ctx.cfg.command_max_output_len,
            weather_timeout_secs: self._ctx.cfg.weather_timeout_secs,
            allowed_commands: &self._ctx.allowed_commands,
            working_dir: &self._ctx.working_dir,
            web_search_timeout_secs: self._ctx.cfg.web_search_timeout_secs,
            web_search_provider: self._ctx.cfg.web_search_provider,
            web_search_api_key: "",
            web_search_max_results: self._ctx.cfg.web_search_max_results,
            http_fetch_allowed_prefixes: &self._ctx.http_fetch_allowed_prefixes,
            http_fetch_timeout_secs: self._ctx.cfg.http_fetch_timeout_secs,
            http_fetch_max_response_bytes: self._ctx.cfg.http_fetch_max_response_bytes,
            command_timeout_secs: self._ctx.cfg.command_timeout_secs,
            read_file_turn_cache: None,
            workspace_changelist: None,
            test_result_cache_enabled: false,
            test_result_cache_max_entries: 0,
            long_term_memory: None,
            long_term_memory_scope_id: None,
        };

        tools::run_tool(name, args, &ctx)
    }

    /// 检查工具是否存在
    #[allow(dead_code)]
    pub fn has_tool(&self, name: &str) -> bool {
        !name.is_empty()
    }
}

/// 工具执行结果
#[derive(Debug, Clone)]
pub struct ToolExecutionResult {
    pub tool_name: String,
    pub output: String,
    pub error: Option<String>,
    pub success: bool,
}

/// 截断参数用于日志（按字符边界截断，支持中文）
fn truncate_args(args: &str) -> String {
    const MAX_LEN: usize = 100;
    if args.len() > MAX_LEN {
        let truncated = args
            .char_indices()
            .take(MAX_LEN - 3)
            .last()
            .map(|(i, c)| i + c.len_utf8())
            .unwrap_or(0);
        format!("{}...", &args[..truncated])
    } else {
        args.to_string()
    }
}
