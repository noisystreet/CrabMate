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

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Brave => "brave",
            Self::Tavily => "tavily",
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

/// 分阶段规划在单步执行失败或工具报错时的反馈模式（第二模式：短规划补丁）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum StagedPlanFeedbackMode {
    /// 与历史一致：步级 `run_agent_outer_loop` 返回 `Err` 时整轮计划失败并向上传播。
    #[default]
    FailFast,
    /// 将失败信号回灌 planner：追加 user 说明后发起无工具规划轮，产出补丁 `agent_reply_plan` 与未完成步后缀合并再继续。
    PatchPlanner,
}

/// `HandlerId::SyncDefault` 工具是否在隔离环境中执行（默认宿主进程内 `spawn_blocking`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SyncDefaultToolSandboxMode {
    /// 与历史一致：在 Agent 进程内执行。
    #[default]
    None,
    /// 每个工具调用 `docker run` 一次，挂载工作区与宿主 `crabmate` 二进制。
    Docker,
}

impl SyncDefaultToolSandboxMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" | "off" | "false" | "0" => Ok(Self::None),
            "docker" => Ok(Self::Docker),
            _ => Err(format!(
                "未知的 sync_default_tool_sandbox_mode: {:?}（支持 none、docker）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Docker => "docker",
        }
    }
}

impl StagedPlanFeedbackMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "fail_fast" | "failfast" => Ok(Self::FailFast),
            "patch_planner" | "patchplanner" => Ok(Self::PatchPlanner),
            _ => Err(format!(
                "未知的 staged_plan_feedback_mode: {:?}（支持 fail_fast、patch_planner）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::FailFast => "fail_fast",
            Self::PatchPlanner => "patch_planner",
        }
    }
}

/// 对 OpenAI 兼容 **`POST …/chat/completions`**（及同基址 **`GET …/models`**）的 HTTP 鉴权方式。
///
/// 本地 **Ollama** 等默认无需密钥时可设为 [`Self::None`]，进程可不设 **`API_KEY`** 且不发送 `Authorization`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmHttpAuthMode {
    /// `Authorization: Bearer {API_KEY}`（云端 OpenAI 兼容服务默认）。
    #[default]
    Bearer,
    /// 不附加 `Authorization`；**`API_KEY` 环境变量可省略**。
    None,
}

impl LlmHttpAuthMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "bearer" => Ok(Self::Bearer),
            "none" | "off" | "false" | "no" | "no_auth" => Ok(Self::None),
            _ => Err(format!(
                "未知的 llm_http_auth_mode: {:?}（支持 bearer、none）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Bearer => "bearer",
            Self::None => "none",
        }
    }
}

#[cfg(test)]
mod llm_http_auth_mode_tests {
    use super::LlmHttpAuthMode;

    #[test]
    fn parse_bearer_and_none_aliases() {
        assert_eq!(
            LlmHttpAuthMode::parse("bearer").unwrap(),
            LlmHttpAuthMode::Bearer
        );
        assert_eq!(
            LlmHttpAuthMode::parse("NONE").unwrap(),
            LlmHttpAuthMode::None
        );
        assert_eq!(
            LlmHttpAuthMode::parse("no_auth").unwrap(),
            LlmHttpAuthMode::None
        );
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

/// 长期记忆向量检索后端（`qdrant` / `pgvector` 在 `finalize` 仍会报错直至接入外部服务）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LongTermMemoryVectorBackend {
    /// 不使用向量索引（按时间取最近片段；检索侧与 `fastembed` 失败时的降级路径一致）。
    Disabled,
    /// 本地 CPU 嵌入（fastembed-rs / ONNX）；**配置缺省向量后端时**与长期记忆默认启用一致。
    #[default]
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
    /// 模型 HTTP 是否带 `Authorization: Bearer`（本地 Ollama 等可 `none`）。
    pub llm_http_auth_mode: LlmHttpAuthMode,
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
    /// 系统提示词：默认自 `system_prompt_file` 读盘；无文件路径时使用合并后的内联（见 `config::load_config` 与文档）
    pub system_prompt: String,
    /// 启用后：读取 `cursor_rules_dir` 下的 `*.mdc` 并附加到系统提示词
    pub cursor_rules_enabled: bool,
    /// Cursor-like 规则目录（相对路径相对进程当前目录）
    pub cursor_rules_dir: String,
    /// 启用 cursor-like 规则时，是否附加工作区根 `AGENTS.md`
    pub cursor_rules_include_agents_md: bool,
    /// 规则附加段最大字符数，超出时截断并附提示
    pub cursor_rules_max_chars: usize,
    /// `role: tool` 的 `content` 超过此字符数时压缩（每次调模型前应用）。信封形态下对 `output` 做首尾采样并写 `output_truncated` 等元数据，见 `tool_result::maybe_compress_tool_message_content`。
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
    /// 单轮内并行只读工具（`SyncDefault` + `http_fetch` + `get_weather` + `web_search` 等 eligible 批）时 `spawn_blocking` 的最大并发（默认等于 `chat_queue_max_concurrent`）
    pub parallel_readonly_tools_max: usize,
    /// 单轮 `run_agent_turn` 内 `read_file` 磁盘缓存最大条数；`0` 关闭。写类工具或 `workspace_changed` 后整表清空。
    pub read_file_turn_cache_max_entries: usize,
    /// 为 true 时：用户每条消息先经**无工具**规划轮产出 `agent_reply_plan` v1，再按 `steps` 顺序各注入一条 user 并跑完整 Agent 循环直至该步终答。
    pub staged_plan_execution: bool,
    /// 规划轮追加的 **system** 指令；空字符串则使用内置默认文案。
    pub staged_plan_phase_instruction: String,
    /// 为 true 时：内置规划说明包含「无具体任务则 `no_task` + 空 `steps`」；为 false 时省略该段（模型仍可能返回 `no_task`，服务端仍会尊重）。
    pub staged_plan_allow_no_task: bool,
    /// 分阶段单步失败或步内工具报错时的处理：`fail_fast`（默认）或 `patch_planner`（短规划补丁）。
    pub staged_plan_feedback_mode: StagedPlanFeedbackMode,
    /// `patch_planner` 下对单步连续规划补丁的最大次数（含首次补丁）；达到后仍按 `fail_fast` 结束。
    pub staged_plan_patch_max_attempts: usize,
    /// 为 true（默认）时：CLI（无 SSE、`out: None`）在**无工具规划轮**与**补丁规划轮**向 stdout 流式/整段打印模型原文（与常规助手轮一致）。为 false 时关闭该轮终端输出，仍保留 `staged_plan_notice` 队列摘要与分步注入等转录；带 `out` 的 Web 路径不受影响。
    pub staged_plan_cli_show_planner_stream: bool,
    /// 分阶段规划首轮 JSON 解析成功后，再跑一轮无工具「步骤优化」（合并无依赖只读探查步、提示单轮内可并行批处理工具）。为 false 时跳过，省一次 API。
    pub staged_plan_optimizer_round: bool,
    /// `HandlerId::SyncDefault` 工具沙盒模式；`docker` 时依赖宿主 `docker` CLI 与镜像。
    pub sync_default_tool_sandbox_mode: SyncDefaultToolSandboxMode,
    /// `sync_default_tool_sandbox_mode = docker` 时使用的镜像（如 `crabmate-tools:dev`）。
    pub sync_default_tool_sandbox_docker_image: String,
    /// 为空则 `docker run --network none`；否则为网络名（如 `bridge`）以允许容器内联网。
    pub sync_default_tool_sandbox_docker_network: String,
    /// 单次 `docker run` 等待上限（秒），含镜像拉取与工具执行。
    pub sync_default_tool_sandbox_docker_timeout_secs: u64,
    /// Web 会话持久化：非空则使用 SQLite（`conversation_id` 跨重启保留）；空则仅进程内内存。
    pub conversation_store_sqlite_path: String,
    /// 为 true 时：首轮在 `system` 与当前用户消息之间注入工作区内备忘文件（见 `agent_memory_file`）。
    pub agent_memory_file_enabled: bool,
    /// 相对**当前 Web 工作区根**的备忘文件路径（如 `.crabmate/agent_memory.md`）。
    pub agent_memory_file: String,
    /// 注入备忘正文的最大字符数（超出截断）。
    pub agent_memory_file_max_chars: usize,
    /// Web 新会话首轮：在备忘（若有）之外注入**自动生成的项目画像**（只读扫描 + 可选 `cargo metadata --no-deps`）。
    pub project_profile_inject_enabled: bool,
    /// 项目画像注入正文的字符上限（与备忘合并后仍受此上限约束的片段各自在生成时截断）。
    pub project_profile_inject_max_chars: usize,
    /// 为 true 时：对**非只读**内置工具（含 `run_command` / `run_executable`、写文件、`http_request`、git 写操作等）要求 JSON 中带 `crabmate_explain_why` 一句目的说明；执行前剥离该字段。与审批互补。
    pub tool_call_explain_enabled: bool,
    /// `crabmate_explain_why` 最少字符数（按 Unicode 标量计数）。
    pub tool_call_explain_min_chars: usize,
    /// `crabmate_explain_why` 最多字符数。
    pub tool_call_explain_max_chars: usize,
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
    fn vector_backend_default_is_fastembed() {
        assert_eq!(
            LongTermMemoryVectorBackend::default(),
            LongTermMemoryVectorBackend::Fastembed
        );
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
