use crate::agent::per_coord::FinalPlanRequirementMode;

/// `web_search` 工具使用的第三方搜索 API 提供商
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WebSearchProvider {
    /// [Brave Search API](https://brave.com/search/api/)
    #[default]
    Brave,
    /// [Tavily Search API](https://tavily.com/)
    Tavily,
}

impl WebSearchProvider {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "brave" => Ok(Self::Brave),
            "tavily" => Ok(Self::Tavily),
            _ => Err(format!(
                "未知的 web_search_provider: {:?}（支持 brave、tavily）",
                s.trim()
            )),
        }
    }
}

/// 规划器与执行器的运行模式（阶段 1：同进程逻辑双 agent）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerExecutorMode {
    /// 单 agent 外层循环（历史行为）。
    #[default]
    SingleAgent,
    /// 同进程逻辑双 agent：规划轮与执行轮使用不同上下文视图。
    LogicalDualAgent,
}

impl PlannerExecutorMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "single_agent" => Ok(Self::SingleAgent),
            "logical_dual_agent" => Ok(Self::LogicalDualAgent),
            _ => Err(format!(
                "未知的 planner_executor_mode: {:?}（支持 single_agent、logical_dual_agent）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleAgent => "single_agent",
            Self::LogicalDualAgent => "logical_dual_agent",
        }
    }
}

/// Agent 运行配置
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// API 基础 URL，如 https://api.deepseek.com/v1
    pub api_base: String,
    /// 模型 ID，如 deepseek-chat、deepseek-reasoner
    pub model: String,
    /// 保留的最近对话轮数（user+assistant 算一轮）
    pub max_message_history: usize,
    /// 为 `true` 时 CLI REPL 启动从 `.crabmate/tui_session.json` 恢复会话；默认 `false` 仅含当前配置的 `system` 一条（文件名历史兼容）
    pub tui_load_session_on_start: bool,
    /// `tui_load_session_on_start` 为 `true` 时：从会话文件加载的消息条数上限（含 `system`）；超出则丢弃最旧非 system 消息
    pub tui_session_max_messages: usize,
    /// run_command 最长执行时间（秒）
    pub command_timeout_secs: u64,
    /// run_command 输出最大长度（字符），超出则截断
    pub command_max_output_len: usize,
    /// run_command 允许执行的命令白名单
    pub allowed_commands: Vec<String>,
    /// run_command 的工作目录（命令在该目录下执行）
    pub run_command_working_dir: String,
    /// 对话 API 单次请求最大 token 数
    pub max_tokens: u32,
    /// 采样温度，0～2
    pub temperature: f32,
    /// HTTP 请求超时（秒），用于 chat 等 API
    pub api_timeout_secs: u64,
    /// API 失败时最大重试次数（0 = 仅首次，不再重试）
    pub api_max_retries: u32,
    /// 重试前等待秒数（指数退避的基数）
    pub api_retry_delay_secs: u64,
    /// get_weather 工具请求超时（秒）
    pub weather_timeout_secs: u64,
    /// web_search 工具使用的搜索 API 提供商
    pub web_search_provider: WebSearchProvider,
    /// web_search 的 API Key（空字符串表示未启用联网搜索）
    pub web_search_api_key: String,
    /// web_search HTTP 超时（秒）
    pub web_search_timeout_secs: u64,
    /// web_search 默认返回条数上限（工具参数 max_results 可覆盖，整体限制在 1～20）
    pub web_search_max_results: u32,
    /// http_fetch：Web 模式仅允许此前缀列表中的 URL；TUI 未匹配时可人工审批
    pub http_fetch_allowed_prefixes: Vec<String>,
    /// http_fetch GET 超时（秒）
    pub http_fetch_timeout_secs: u64,
    /// http_fetch 响应体截断上限（字节）
    pub http_fetch_max_response_bytes: usize,
    /// workflow 反思：模型未在 `workflow.reflection.max_rounds` 中指定时的默认上限（传给 `WorkflowReflectionController` / `PerCoordinator`）
    pub reflection_default_max_rounds: usize,
    /// 何时强制终答含 `agent_reply_plan` v1（见 `per_coord::FinalPlanRequirementMode`）
    pub final_plan_requirement: FinalPlanRequirementMode,
    /// 终答缺合格规划时，最多追加多少次「请重写」user 消息（达到后结束本轮并发 SSE `plan_rewrite_exhausted`）
    pub plan_rewrite_max_attempts: usize,
    /// 规划器/执行器运行模式（阶段 1：同进程逻辑双 agent）。
    pub planner_executor_mode: PlannerExecutorMode,
    /// 系统提示词（可由 system_prompt 或 system_prompt_file 配置）
    pub system_prompt: String,
    /// 启用后：读取 `cursor_rules_dir` 下的 `*.mdc` 并附加到系统提示词
    pub cursor_rules_enabled: bool,
    /// Cursor-like 规则目录（相对路径相对进程当前目录）
    pub cursor_rules_dir: String,
    /// 启用 cursor-like 规则时，是否附加工作区根 `AGENTS.md`
    pub cursor_rules_include_agents_md: bool,
    /// 规则附加段最大字符数，超出时截断并附提示
    pub cursor_rules_max_chars: usize,
    /// `role: tool` 的 `content` 超过此字符数时截断（每次调模型前应用）
    pub tool_message_max_chars: usize,
    /// 非 system 消息总字符预算（近似）；`0` 表示不启用按字符删旧消息
    pub context_char_budget: usize,
    /// 启用 `context_char_budget` 时，system 之后至少保留的消息条数
    pub context_min_messages_after_system: usize,
    /// 非 system 总字符超过此值时触发一次 LLM 摘要；`0` 表示关闭
    pub context_summary_trigger_chars: usize,
    /// 摘要后保留的尾部消息条数（须 ≥4，与工具轮次衔接）
    pub context_summary_tail_messages: usize,
    /// 摘要请求 `max_tokens`
    pub context_summary_max_tokens: u32,
    /// 送入摘要模型的中间段转写最大字符数（防摘要请求本身过大）
    pub context_summary_transcript_max_chars: usize,
    /// Web `POST /workspace` 允许设置的工作区根路径：规范化为绝对路径后的白名单。
    /// 未在配置中指定 `workspace_allowed_roots` 时，仅含 `run_command_working_dir` 的 canonical 路径。
    pub workspace_allowed_roots: Vec<std::path::PathBuf>,
    /// Web API 的 Bearer 鉴权令牌（为空表示不启用鉴权）。
    pub web_api_bearer_token: String,
    /// 当监听非 loopback 地址且 `web_api_bearer_token` 为空时，是否允许继续启动（不安全，默认 false）。
    pub allow_insecure_no_auth_for_non_loopback: bool,
    /// Web `/chat` 任务最大并发执行数（单进程）
    pub chat_queue_max_concurrent: usize,
    /// Web 对话任务有界等待队列长度（`try_send` 满则 503）
    pub chat_queue_max_pending: usize,
    /// 为 true 时：用户每条消息先经**无工具**规划轮产出 `agent_reply_plan` v1，再按 `steps` 顺序各注入一条 user 并跑完整 Agent 循环直至该步终答。
    pub staged_plan_execution: bool,
    /// 规划轮追加的 **system** 指令；空字符串则使用内置默认文案。
    pub staged_plan_phase_instruction: String,
}
