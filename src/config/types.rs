use crate::agent::per_coord::FinalPlanRequirementMode;

/// 敏感字符串（`Debug` / 结构化日志默认脱敏）；取值请用 [`ExposeSecret::expose_secret`]。
pub use secrecy::{ExposeSecret, SecretString};

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
    /// 每个工具调用经 Docker Engine API 创建一次性容器，挂载工作区与宿主 `crabmate` 二进制。
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

/// Docker 沙盒容器内进程身份（`docker run --user` / API `Config.user`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxDockerContainerUser {
    /// 不设置，沿用镜像 `USER`（常为 root）。
    ImageDefault,
    /// 使用本字段字符串原样传入 Docker（`uid[:gid]`、`user[:group]` 等）。
    Spec(String),
}

impl SandboxDockerContainerUser {
    /// `current` / 空 → 有效用户 `uid:gid`（Unix）；非 Unix 返回 `ImageDefault`。
    /// `image` / `default` → `ImageDefault`；否则整段 trim 后作为 `Spec`。
    pub fn resolve_from_config_str(s: &str) -> Self {
        let t = s.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("current") || t.eq_ignore_ascii_case("host") {
            return effective_current_uid_gid_spec();
        }
        if t.eq_ignore_ascii_case("image") || t.eq_ignore_ascii_case("default") {
            return Self::ImageDefault;
        }
        Self::Spec(t.to_string())
    }

    /// 写入 bollard `Config.user`：`None` 表示不设置。
    pub fn as_docker_user_string(&self) -> Option<&str> {
        match self {
            Self::ImageDefault => None,
            Self::Spec(s) => Some(s.as_str()),
        }
    }
}

#[cfg(unix)]
fn effective_current_uid_gid_spec() -> SandboxDockerContainerUser {
    // SAFETY: `geteuid` / `getegid` 为 POSIX，无指针参数，仅返回当前有效 id。
    let uid = unsafe { libc::geteuid() };
    let gid = unsafe { libc::getegid() };
    SandboxDockerContainerUser::Spec(format!("{uid}:{gid}"))
}

#[cfg(not(unix))]
fn effective_current_uid_gid_spec() -> SandboxDockerContainerUser {
    SandboxDockerContainerUser::ImageDefault
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
mod sandbox_docker_user_tests {
    use super::SandboxDockerContainerUser;

    #[test]
    fn resolve_image_and_default_alias() {
        assert_eq!(
            SandboxDockerContainerUser::resolve_from_config_str("image"),
            SandboxDockerContainerUser::ImageDefault
        );
        assert_eq!(
            SandboxDockerContainerUser::resolve_from_config_str("DEFAULT"),
            SandboxDockerContainerUser::ImageDefault
        );
    }

    #[test]
    fn resolve_literal_uid_gid() {
        let u = SandboxDockerContainerUser::resolve_from_config_str("1001:1002");
        assert_eq!(u.as_docker_user_string(), Some("1001:1002"));
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

/// 角色 id → 已合并 cursor rules 的 system 正文（`Arc` 便于配置热更共享）
pub type AgentRoleCatalog =
    std::sync::Arc<std::collections::HashMap<String, super::agent_role_spec::AgentRoleSpec>>;

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
    /// 为 `true` 时 REPL 在后台运行 [`crate::runtime::workspace_session::initial_workspace_messages`]（项目画像、依赖摘要、`tui_load_session_on_start` 时的会话恢复等）；默认 `false` 不调用，启动仅一条 `system`。
    pub repl_initial_workspace_messages_enabled: bool,
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
    /// MiniMax OpenAI 兼容：为 `true` 时请求体带 **`reasoning_split`**，流式思维链可走 **`delta.reasoning_details`**（并入 [`crate::types::Message::reasoning_content`]）。未在配置中显式设置时，**MiniMax 网关默认为 `true`**，其余为 **`false`**（见 [`crate::llm::vendor::default_llm_reasoning_split_for_gateway`]）。
    pub llm_reasoning_split: bool,
    /// 智谱 **open.bigmodel.cn**（GLM-5 等）：为 `true` 时请求体带 **`thinking: { "type": "enabled" }`**，与 [GLM-5 调用示例](https://docs.bigmodel.cn/cn/guide/models/text/glm-5) 一致；流式 **`delta.reasoning_content`** 由现有解析路径消费。
    pub llm_bigmodel_thinking: bool,
    /// Moonshot **Kimi**（**kimi-k2.5**）：为 `true` 时请求体带 **`thinking: { "type": "disabled" }`**，关闭文档所述默认思考行为（见 [Kimi Chat API](https://platform.moonshot.cn/docs/api/chat) **`thinking`** 字段）。**仅**在对接 Moonshot 且模型为 **kimi-k2.5** 时使用；其它网关勿开。
    pub llm_kimi_thinking_disabled: bool,
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
    /// web_search 的 API Key（空表示未启用联网搜索）；勿 `Debug` 打印裸值。
    pub web_search_api_key: SecretString,
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
    /// 为 true 时：若 `agent_reply_plan` 中**任一步**填写 `workflow_node_id`，则须覆盖最近一次工作流工具结果中的**全部** `nodes[].id`。
    pub final_plan_require_strict_workflow_node_coverage: bool,
    /// 为 true 时（且 `final_plan_requirement = workflow_reflection` 且本轮已置位终答规划需求）：在静态规则通过后追加一次极短无工具 LLM，对比规划与最近工具摘要；默认 false。
    pub final_plan_semantic_check_enabled: bool,
    /// 语义侧向校验摘要中最多收录几条**非只读**工具（0 表示仅只读 + 内置高风险工具名）。
    pub final_plan_semantic_check_max_non_readonly_tools: usize,
    /// 侧向校验 `chat/completions` 请求的 `max_tokens`（钳制 32..=1024）。
    pub final_plan_semantic_check_max_tokens: u32,
    /// 规划器/执行器运行模式（阶段 1：同进程逻辑双 agent）。
    pub planner_executor_mode: PlannerExecutorMode,
    /// 系统提示词：默认自 `system_prompt_file` 读盘；无文件路径时使用合并后的内联（见 `config::load_config` 与文档）
    pub system_prompt: String,
    /// Web/CLI 未传 `agent_role` 时使用的默认角色 id（`None` 表示用 [`Self::system_prompt`]）
    pub default_agent_role_id: Option<String>,
    /// 命名角色表（`config/agent_roles.toml` 等）；空表表示未启用多角色
    pub agent_roles: AgentRoleCatalog,
    /// 为 true（默认）时：读取 `cursor_rules_dir` 下的 `*.mdc` 并附加到系统提示词
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
    /// 为 true 时：SSE **`tool_call`** 事件除 **`arguments_preview`** 外另含脱敏后的 **`arguments`**（更长上限，仍非全文保证）；默认 false，避免浏览器/共享屏泄露参数。
    pub sse_tool_call_include_arguments: bool,
    /// 为 true 时：进程内记录各工具完结的 `ok`/`error_code`（滑动窗口），并在**新会话**首条 `system` 末尾可选附加短提示；不按会话分桶、不落盘。
    pub agent_tool_stats_enabled: bool,
    /// 上述统计滑动窗口最多保留的事件条数（单进程全局）。
    pub agent_tool_stats_window_events: usize,
    /// 某工具在窗口内总调用次数 ≥ 此值才参与提示。
    pub agent_tool_stats_min_samples: usize,
    /// 附加 Markdown 段的最大 Unicode 标量数（超出截断）。
    pub agent_tool_stats_max_chars: usize,
    /// 成功率（成功次数/总次数）**低于**该阈值且满足 `min_samples` 时输出提示；有失败时也会提示。
    pub agent_tool_stats_warn_below_success_ratio: f64,
    /// 为 true（默认）时：若 API 未给出**可用的**原生 `tool_calls`，从助手 `content`/`reasoning_content` 中的 DeepSeek DSML 解析并写入 `tool_calls`。
    /// 为 false 时：**不**做 DSML 物化，仅信任 API `tool_calls`（与「仅一段 JSON 约定工具调用」等结构化网关更一致）。
    pub materialize_deepseek_dsml_tool_calls: bool,
    /// 为 true（默认）时：在经 `augment_system_prompt` 处理的首条 `system` 末尾附加「思考纪律」正文（见 [`Self::thinking_avoid_echo_appendix`]）。
    pub thinking_avoid_echo_system_prompt: bool,
    /// `finalize` 解析后的附录全文：来自内联、`thinking_avoid_echo_appendix_file` 读盘，或编译嵌入默认（见 `config/prompts/thinking_avoid_echo_appendix.md`）。
    pub thinking_avoid_echo_appendix: String,
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
    /// Web API 的 Bearer 鉴权令牌（为空表示不启用鉴权）；勿 `Debug` 打印裸值。
    pub web_api_bearer_token: SecretString,
    /// 当监听非 loopback 地址且 `web_api_bearer_token` 为空时，是否允许继续启动（不安全，默认 false）。
    pub allow_insecure_no_auth_for_non_loopback: bool,
    /// 为 `true` 时 `GET /health` 对当前 `api_base` 可选发起 **GET …/models**（仅列表 HTTP，无 chat/completions 计费）；默认 `false`，避免探活风暴。
    pub health_llm_models_probe: bool,
    /// [`Self::health_llm_models_probe`] 开启时，探测结果在进程内缓存的秒数（降低频繁 `/health` 对上游的请求频率）。
    pub health_llm_models_probe_cache_secs: u64,
    /// Web `/chat` 任务最大并发执行数（单进程）
    pub chat_queue_max_concurrent: usize,
    /// Web 对话任务有界等待队列长度（`try_send` 满则 503）
    pub chat_queue_max_pending: usize,
    /// 单轮内并行只读工具（`SyncDefault` + `http_fetch` + `get_weather` + `web_search` 等 eligible 批）时 `spawn_blocking` 的最大并发（默认等于 `chat_queue_max_concurrent`）
    pub parallel_readonly_tools_max: usize,
    /// 单轮 `run_agent_turn` 内 `read_file` 磁盘缓存最大条数；`0` 关闭。写类工具或 `workspace_changed` 后整表清空。
    pub read_file_turn_cache_max_entries: usize,
    /// 进程内缓存 **`cargo_test` / `rust_test_one` / `npm run test`** 及部分 **`run_command cargo test …`** 的截断后输出；指纹基于工作区内 `.rs`/`.toml`/`Cargo.lock`（Rust）或 `package.json`/lock（npm）。
    pub test_result_cache_enabled: bool,
    /// LRU 条数上限（仅进程内；重启清空）。
    pub test_result_cache_max_entries: usize,
    /// 为 true（默认）时：按 `long_term_memory_scope_id`（Web 为 `conversation_id`）累积本会话工具写入路径，并在每次调模型前注入 unified diff 摘要（`user.name=crabmate_workspace_changelist`）。
    pub session_workspace_changelist_enabled: bool,
    /// 上述注入正文近似字符上限（防撑爆上下文）；`0` 表示用默认 12000。
    pub session_workspace_changelist_max_chars: usize,
    /// 为 true 时：用户每条消息先经**无工具**规划轮产出 `agent_reply_plan` v1，再按 `steps` 顺序各注入一条 user 并跑完整 Agent 循环直至该步终答。
    pub staged_plan_execution: bool,
    /// 规划轮追加的 **system** 指令；空字符串则使用内置默认文案。
    pub staged_plan_phase_instruction: String,
    /// **兼容保留**：旧版曾用该键切换规划轮是否追加「无任务则 `no_task`」**硬提示**；现已移除硬提示，`no_task` 语义仅以 [`crate::agent::plan_artifact::PLAN_V1_SCHEMA_RULES`]（拼入默认规划 **system**）为准。配置项仍解析/热重载，**无运行时效果**。
    #[allow(dead_code)]
    pub staged_plan_allow_no_task: bool,
    /// 分阶段单步失败或步内工具报错时的处理：`fail_fast`（默认）或 `patch_planner`（短规划补丁）。
    pub staged_plan_feedback_mode: StagedPlanFeedbackMode,
    /// `patch_planner` 下对单步连续规划补丁的最大次数（含首次补丁）；达到后仍按 `fail_fast` 结束。
    pub staged_plan_patch_max_attempts: usize,
    /// 为 true（默认）时：CLI（无 SSE、`out: None`）在**无工具规划轮**与**补丁规划轮**向 stdout 流式/整段打印模型原文（与常规助手轮一致）。为 false 时关闭该轮终端输出，仍保留 `staged_plan_notice` 队列摘要与分步注入等转录；带 `out` 的 Web 路径不受影响。
    pub staged_plan_cli_show_planner_stream: bool,
    /// 分阶段规划首轮 JSON 解析成功后，再跑一轮无工具「步骤优化」（合并无依赖只读探查步、提示单轮内可并行批处理工具）。为 false 时跳过，省一次 API。
    pub staged_plan_optimizer_round: bool,
    /// 为 true（默认）时：仅当本会话 `tools_defs` 中至少有一个「可同轮并行批处理」内建工具名时才跑优化轮；否则跳过（优化轮正文主要围绕并行只读工具列表）。为 false 时恢复旧行为（只要 `steps.len()>=2` 且开启优化轮即调用）。
    pub staged_plan_optimizer_requires_parallel_tools: bool,
    /// 逻辑多规划员：首轮后的**独立**无工具规划份数上限（1=关闭；2=首轮+A 再合并；3=首轮+A+B 再合并）。串行同模型，**显著增加 API 调用**。
    pub staged_plan_ensemble_count: u8,
    /// 为 true（默认）且 `staged_plan_ensemble_count>1` 时：若触发本轮规划的用户正文经启发式判定为寒暄/极短，则跳过逻辑多规划员与合并轮以省 API。
    pub staged_plan_skip_ensemble_on_casual_prompt: bool,
    /// 为 true 时：分阶段规划各 **JSON 规划轮**不向用户侧 SSE/终端流式输出；定稿后**再**追加一轮无工具补全，仅将自然语言流式给用户（历史仍保留 JSON 助手条 + 桥接 user + NL 助手条）。默认 false。
    pub staged_plan_two_phase_nl_display: bool,
    /// `HandlerId::SyncDefault` 工具沙盒模式；`docker` 时依赖宿主 `docker` CLI 与镜像。
    pub sync_default_tool_sandbox_mode: SyncDefaultToolSandboxMode,
    /// `sync_default_tool_sandbox_mode = docker` 时使用的镜像（如 `crabmate-tools:dev`）。
    pub sync_default_tool_sandbox_docker_image: String,
    /// 为空则 `docker run --network none`；否则为网络名（如 `bridge`）以允许容器内联网。
    pub sync_default_tool_sandbox_docker_network: String,
    /// 单次 `docker run` 等待上限（秒），含镜像拉取与工具执行。
    pub sync_default_tool_sandbox_docker_timeout_secs: u64,
    /// Docker 沙盒容器 `user`：`current`（默认，Unix 为有效 `uid:gid`）、`image`（镜像默认）、或 Docker 接受的 `uid[:gid]` 等字面量。
    pub sync_default_tool_sandbox_docker_user: SandboxDockerContainerUser,
    /// Web 会话持久化：非空则使用 SQLite（`conversation_id` 跨重启保留）；空则仅进程内内存。
    pub conversation_store_sqlite_path: String,
    /// 为 true 时：首轮在 `system` 与当前用户消息之间注入工作区内备忘文件（见 `agent_memory_file`）。
    pub agent_memory_file_enabled: bool,
    /// 相对**当前 Web 工作区根**的备忘文件路径（如 `.crabmate/agent_memory.md`）。
    pub agent_memory_file: String,
    /// 注入备忘正文的最大字符数（超出截断）。
    pub agent_memory_file_max_chars: usize,
    /// 首轮在备忘 / 项目画像之前注入 `.crabmate/living_docs/` 摘要（`SUMMARY.md`、`map.md` 等）；`0` 关闭。
    pub living_docs_inject_enabled: bool,
    /// 活文档目录（相对工作区根），默认 `.crabmate/living_docs`。
    pub living_docs_relative_dir: String,
    /// 活文档注入总字符上限。
    pub living_docs_inject_max_chars: usize,
    /// 每个活文档文件读入时的字符上限。
    pub living_docs_file_max_each_chars: usize,
    /// Web 新会话首轮：在备忘（若有）之外注入**自动生成的项目画像**（只读扫描 + 可选 `cargo metadata --no-deps`）。
    pub project_profile_inject_enabled: bool,
    /// 项目画像注入正文的字符上限（与备忘合并后仍受此上限约束的片段各自在生成时截断）。
    pub project_profile_inject_max_chars: usize,
    /// 首轮在「项目画像 / 备忘」合并条目中追加 **`cargo metadata` + package.json** 的结构化 JSON 与 Mermaid workspace 图（独立预算）。
    pub project_dependency_brief_inject_enabled: bool,
    /// 依赖摘要注入正文最大字符数；`0` 关闭生成。
    pub project_dependency_brief_inject_max_chars: usize,
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
    /// 回合结束是否自动把 user/assistant 终答写入长期记忆；`false` 时仅 `long_term_remember` 等显式路径写入。
    pub long_term_memory_auto_index_turns: bool,
    /// 自动索引条目的默认存活秒数；`0` 表示不过期（仍受 `long_term_memory_max_entries` 淘汰）。
    pub long_term_memory_default_ttl_secs: u64,
    /// 是否启用 MCP（stdio 子进程）；与 `mcp_command` 配合使用。
    pub mcp_enabled: bool,
    /// 启动 MCP server 的命令行（空格分词，无引号转义）；等效于允许执行任意子进程，须来自可信配置。
    pub mcp_command: String,
    /// `tools/call` 超时（秒）。
    pub mcp_tool_timeout_secs: u64,
    /// 是否注册并允许 `codebase_semantic_search`（本地 fastembed + SQLite；`rebuild_index` 会写 `.crabmate/`）。
    pub codebase_semantic_search_enabled: bool,
    /// 写工具成功或 `workspace_changed` 后是否删除语义索引中相关块（与 `read_file` 缓存清空对齐）。
    pub codebase_semantic_invalidate_on_workspace_change: bool,
    /// 相对工作区的语义索引 SQLite 路径；空则使用 `.crabmate/codebase_semantic.sqlite`。
    pub codebase_semantic_index_sqlite_path: String,
    /// 参与索引的单文件最大字节数（超出则跳过）。
    pub codebase_semantic_max_file_bytes: usize,
    /// 单块最大字符数（分块嵌入）。
    pub codebase_semantic_chunk_max_chars: usize,
    /// 默认检索 Top-K（工具参数可覆盖）。
    pub codebase_semantic_top_k: usize,
    /// 单次 `query` 最多扫描多少个向量块；`0` 表示不限制（大索引慎用）。
    pub codebase_semantic_query_max_chunks: usize,
    /// `rebuild_index` 时最多索引多少个文件（防超大仓拖死进程）。
    pub codebase_semantic_rebuild_max_files: usize,
    /// 整库 `rebuild_index` 时是否按文件指纹跳过未改文件（默认 true）；`incremental:false` 可单次强制全量。
    pub codebase_semantic_rebuild_incremental: bool,
    /// `http_fetch`：`spawn_blocking` 外圈 `tokio::time::timeout`（秒）；`None` 表示 `max(command_timeout_secs, http_fetch_timeout_secs)`。
    pub tool_registry_http_fetch_wall_timeout_secs: Option<u64>,
    /// `http_request`：同上。
    pub tool_registry_http_request_wall_timeout_secs: Option<u64>,
    /// 按执行类蛇形键覆盖 **并行批 / SyncDefault spawn** 墙上时钟；空表表示无覆盖。
    pub tool_registry_parallel_wall_timeout_secs:
        std::sync::Arc<std::collections::HashMap<String, u64>>,
    /// 禁止与其它只读工具同批并行的工具名；`None` 用内建默认。
    pub tool_registry_parallel_sync_denied_tools:
        Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// 禁止并行批的前缀；`None` 用内建默认。
    pub tool_registry_parallel_sync_denied_prefixes: Option<std::sync::Arc<[String]>>,
    /// SyncDefault 内联执行（不 `spawn_blocking`）；`None` 用内建默认。
    pub tool_registry_sync_default_inline_tools:
        Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// 视为写副作用的工具集合；`None` 用内建默认（`is_readonly_tool`）。
    pub tool_registry_write_effect_tools: Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// `executor_kind: patch_write` 在默认补丁工具名之外额外允许的工具（须仍在会话 `tools` 中注册）。
    pub tool_registry_sub_agent_patch_write_extra_tools:
        Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// `executor_kind: test_runner` 在默认测试运行器名之外额外允许的工具。
    pub tool_registry_sub_agent_test_runner_extra_tools:
        Option<std::sync::Arc<std::collections::HashSet<String>>>,
    /// `executor_kind: review_readonly` 下显式拒绝的工具名（精确匹配）。
    pub tool_registry_sub_agent_review_readonly_deny_tools:
        Option<std::sync::Arc<std::collections::HashSet<String>>>,
}

impl AgentConfig {
    /// 新建 Web/CLI 会话首条 `system` 的正文来源。
    ///
    /// - 显式 `agent_role`：须在 [`Self::agent_roles`] 中存在，否则返回 `Err`。
    /// - 未指定：使用 [`Self::default_agent_role_id`] 对应条目（若配置且存在），否则 [`Self::system_prompt`]。
    pub fn system_prompt_for_new_conversation(
        &self,
        agent_role: Option<&str>,
    ) -> Result<&str, String> {
        match agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => self
                .agent_roles
                .get(id)
                .map(|s| s.system_prompt.as_str())
                .ok_or_else(|| format!("未知的 agent_role: {id}（请在配置中定义该 id）")),
            None => Ok(self
                .default_agent_role_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|id| self.agent_roles.get(id))
                .map(|s| s.system_prompt.as_str())
                .unwrap_or(self.system_prompt.as_str())),
        }
    }
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

#[cfg(test)]
mod secret_string_tests {
    use super::{ExposeSecret, SecretString};

    #[test]
    fn debug_does_not_echo_plaintext() {
        let s = SecretString::new("not-for-debug-logs-xyzzy".into());
        let d = format!("{s:?}");
        assert!(
            !d.contains("not-for-debug-logs-xyzzy"),
            "Debug leaked secret: {d}"
        );
    }

    #[test]
    fn expose_secret_roundtrip() {
        let s = SecretString::new("k".into());
        assert_eq!(s.expose_secret(), "k");
    }
}
