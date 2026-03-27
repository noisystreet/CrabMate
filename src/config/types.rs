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

/// 长期记忆条目的隔离作用域（向量检索上线后必须与会话/鉴权一致，见 README 安全说明）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LongTermMemoryScopeMode {
    /// 按 Web `conversation_id`（及等价 CLI 会话键）隔离；无多租户鉴权时不要指望跨用户安全。
    #[default]
    Conversation,
}

impl LongTermMemoryScopeMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "conversation" => Ok(Self::Conversation),
            _ => Err(format!(
                "未知的 long_term_memory_scope_mode: {:?}（当前仅支持 conversation）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
        }
    }
}

/// 长期记忆向量检索后端（分阶段实现：`disabled` 先占位，非 `disabled` 在对应阶段落地前会在 `finalize` 报错）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LongTermMemoryVectorBackend {
    /// 不使用向量索引（显式记忆 / 后续纯文本检索路径）。
    #[default]
    Disabled,
    /// 本地 CPU 嵌入（如 fastembed-rs），见路线图阶段 B。
    Fastembed,
    Qdrant,
    Pgvector,
}

impl LongTermMemoryVectorBackend {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "disabled" | "off" | "none" => Ok(Self::Disabled),
            "fastembed" => Ok(Self::Fastembed),
            "qdrant" => Ok(Self::Qdrant),
            "pgvector" => Ok(Self::Pgvector),
            _ => Err(format!(
                "未知的 long_term_memory_vector_backend: {:?}（支持 disabled、fastembed、qdrant、pgvector）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Fastembed => "fastembed",
            Self::Qdrant => "qdrant",
            Self::Pgvector => "pgvector",
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
    /// run_command 允许执行的命令白名单（`Arc` 共享，避免每轮工具调用整表克隆）
    pub allowed_commands: std::sync::Arc<[String]>,
    /// run_command 的工作目录（命令在该目录下执行）
    pub run_command_working_dir: String,
    /// 对话 API 单次请求最大 token 数
    pub max_tokens: u32,
    /// 采样温度，0～2
    pub temperature: f32,
    /// 可选：写入 `chat/completions` 的 **`seed`**（OpenAI 兼容；`None` 则请求 JSON 省略该字段）。
    pub llm_seed: Option<i64>,
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
    /// 为 true（默认）时：写入历史的 `role: tool` 使用 `crabmate_tool` JSON 信封（含 `summary`/`ok`/`output` 等），便于聚合解析；为 false 时保持纯工具原文。
    pub tool_result_envelope_v1: bool,
    /// 为 true（默认）时：若 API 未给出**可用的**原生 `tool_calls`，从助手 `content`/`reasoning_content` 中的 DeepSeek DSML 解析并写入 `tool_calls`。
    /// 为 false 时：**不**做 DSML 物化，仅信任 API `tool_calls`（与「仅一段 JSON 约定工具调用」等结构化网关更一致）。
    pub materialize_deepseek_dsml_tool_calls: bool,
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
    /// 单轮内并行只读 `SyncDefault` 工具时，`spawn_blocking` 的最大并发（默认等于 `chat_queue_max_concurrent`）
    pub parallel_readonly_tools_max: usize,
    /// 为 true 时：用户每条消息先经**无工具**规划轮产出 `agent_reply_plan` v1，再按 `steps` 顺序各注入一条 user 并跑完整 Agent 循环直至该步终答。
    pub staged_plan_execution: bool,
    /// 规划轮追加的 **system** 指令；空字符串则使用内置默认文案。
    pub staged_plan_phase_instruction: String,
    /// 为 true 时：内置规划说明包含「无具体任务则 `no_task` + 空 `steps`」；为 false 时省略该段（模型仍可能返回 `no_task`，服务端仍会尊重）。
    pub staged_plan_allow_no_task: bool,
    /// Web 会话持久化：非空则使用 SQLite（`conversation_id` 跨重启保留）；空则仅进程内内存。
    pub conversation_store_sqlite_path: String,
    /// 为 true 时：首轮在 `system` 与当前用户消息之间注入工作区内备忘文件（见 `agent_memory_file`）。
    pub agent_memory_file_enabled: bool,
    /// 相对**当前 Web 工作区根**的备忘文件路径（如 `.crabmate/agent_memory.md`）。
    pub agent_memory_file: String,
    /// 注入备忘正文的最大字符数（超出截断）。
    pub agent_memory_file_max_chars: usize,
    /// 是否启用长期记忆管线（显式条目 + 后续向量检索）；默认关闭。
    pub long_term_memory_enabled: bool,
    /// 记忆条目按何种键隔离（当前仅 `conversation`）。
    pub long_term_memory_scope_mode: LongTermMemoryScopeMode,
    /// 向量索引后端；非 `disabled` 需在对应里程碑实现后方可启动。
    pub long_term_memory_vector_backend: LongTermMemoryVectorBackend,
    /// 每个作用域内保留的长期记忆条数上限（供后续阶段写入路径使用）。
    pub long_term_memory_max_entries: usize,
    /// 每轮注入模型上下文的长期记忆正文总字符上限（供后续阶段使用）。
    pub long_term_memory_inject_max_chars: usize,
    /// 长期记忆 SQLite 路径；空则与会话库同文件（`conversation_store_sqlite_path` 非空时），否则独立文件；Web 内存会话且无路径时不持久化。
    pub long_term_memory_store_sqlite_path: String,
    /// 向量检索或时间序取用的条数上限。
    pub long_term_memory_top_k: usize,
    /// 单条记忆 chunk 最大字符数（索引与分块）。
    pub long_term_memory_max_chars_per_chunk: usize,
    /// 用户与助手片段均短于此则跳过索引（减少噪声）。
    pub long_term_memory_min_chars_to_index: usize,
    /// 回合结束后异步写入索引（不阻塞 SSE）。
    pub long_term_memory_async_index: bool,
    /// 是否启用 MCP（stdio 子进程）；与 `mcp_command` 配合使用。
    pub mcp_enabled: bool,
    /// 启动 MCP server 的命令行（空格分词，无引号转义）；等效于允许执行任意子进程，须来自可信配置。
    pub mcp_command: String,
    /// `tools/call` 超时（秒）。
    pub mcp_tool_timeout_secs: u64,
}

#[cfg(test)]
mod long_term_memory_parse_tests {
    use super::{LongTermMemoryScopeMode, LongTermMemoryVectorBackend};

    #[test]
    fn scope_mode_parse_conversation() {
        assert_eq!(
            LongTermMemoryScopeMode::parse("conversation").expect("parse"),
            LongTermMemoryScopeMode::Conversation
        );
        assert!(LongTermMemoryScopeMode::parse("tenant").is_err());
    }

    #[test]
    fn vector_backend_parse_variants() {
        assert_eq!(
            LongTermMemoryVectorBackend::parse("disabled").expect("parse"),
            LongTermMemoryVectorBackend::Disabled
        );
        assert_eq!(
            LongTermMemoryVectorBackend::parse("OFF").expect("parse"),
            LongTermMemoryVectorBackend::Disabled
        );
        assert_eq!(
            LongTermMemoryVectorBackend::parse("FastEmbed").expect("parse"),
            LongTermMemoryVectorBackend::Fastembed
        );
        assert_eq!(
            LongTermMemoryVectorBackend::parse("qdrant").expect("parse"),
            LongTermMemoryVectorBackend::Qdrant
        );
        assert_eq!(
            LongTermMemoryVectorBackend::parse("pgvector").expect("parse"),
            LongTermMemoryVectorBackend::Pgvector
        );
        assert!(LongTermMemoryVectorBackend::parse("unknown").is_err());
    }
}
