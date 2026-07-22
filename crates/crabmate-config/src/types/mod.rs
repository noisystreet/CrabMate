mod agent_config_sections;

pub use agent_config_sections::{
    AgentThinkingTraceConfig, AgentToolStatsConfig, ChatQueuesCacheConfig, CodebaseSemanticConfig,
    CommandExecConfig, ContextBootstrapInjectConfig, ContextPipelineConfig,
    ConversationPersistenceConfig, CursorRulesConfigSection, DsmlMaterializeConfig,
    HierarchyRoutingConfig, HttpFetchConfigSection, IntentRoutingConfig, LongTermMemoryConfig,
    McpClientConfig, PerPlanPolicyConfig, RolesPromptsConfig, SessionUiConfig,
    SessionWorkspaceChangelistConfig, SkillsConfigSection, SyncToolSandboxConfig,
    ThinkingEchoConfig, ToolCallExplainConfig, ToolRegistryPolicyConfig, ToolTranscriptConfig,
    TurnBudgetConfig, WeatherToolConfig, WebApiConfig, WebSearchConfigSection,
    WorkspaceRootsConfig,
};

pub use crabmate_types::llm_config::{
    LlmConnectionConfig, LlmHttpAuthMode, LlmHttpRetryConfig, LlmSamplingConfig,
    LlmVendorFlagsConfig,
};

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

/// 规划器与执行器的运行模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PlannerExecutorMode {
    /// 单 agent 外层循环（当前统一强制走 ReAct）。
    SingleAgent,
    /// 分层多 Agent：Manager 分解任务 + Operator 执行子目标。
    #[default]
    Hierarchical,
}

impl PlannerExecutorMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "single_agent" => Ok(Self::SingleAgent),
            "hierarchical" => Ok(Self::Hierarchical),
            _ => Err(format!(
                "未知的 planner_executor_mode: {:?}（支持 single_agent、hierarchical）",
                s.trim()
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::SingleAgent => "single_agent",
            Self::Hierarchical => "hierarchical",
        }
    }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LongTermMemoryVectorBackend {
    /// 不使用向量索引（按时间取最近片段；检索侧与 `fastembed` 失败时的降级路径一致）。
    Disabled,
    /// 本地 CPU 嵌入（fastembed-rs / ONNX）；**配置缺省向量后端时**与长期记忆默认启用一致（需 **`fastembed`** Cargo feature）。
    Fastembed,
    Qdrant,
    Pgvector,
}

// 缺省后端随 `fastembed` feature 变化；不能仅用 `#[derive(Default)]` + 单变体 `#[default]`。
#[allow(clippy::derivable_impls)]
impl Default for LongTermMemoryVectorBackend {
    fn default() -> Self {
        #[cfg(feature = "fastembed")]
        {
            Self::Fastembed
        }
        #[cfg(not(feature = "fastembed"))]
        {
            Self::Disabled
        }
    }
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

/// `[[scheduled_agent_task]]` 经校验后的运行态项（`serve` + `tokio-cron-scheduler`）。
#[derive(Debug, Clone)]
pub struct ScheduledAgentTask {
    pub id: String,
    /// **croner** 六段秒级 cron，UTC
    pub schedule: String,
    /// 与 `POST /chat` 的 `message` 等价的用户正文
    pub message: String,
    pub conversation_id: Option<String>,
    pub new_conversation: bool,
    pub agent_role: Option<String>,
}

/// Agent 运行配置（组合式；各子域字段含义见对应 `*Config` 结构体文档）。
#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub llm: LlmConnectionConfig,
    pub session_ui: SessionUiConfig,
    pub command_exec: CommandExecConfig,
    pub llm_sampling: LlmSamplingConfig,
    pub llm_vendor_flags: LlmVendorFlagsConfig,
    pub llm_http_retry: LlmHttpRetryConfig,
    pub weather_tool: WeatherToolConfig,
    pub web_search: WebSearchConfigSection,
    pub http_fetch: HttpFetchConfigSection,
    pub per_plan_policy: PerPlanPolicyConfig,
    pub roles_prompts: RolesPromptsConfig,
    pub cursor_rules: CursorRulesConfigSection,
    pub skills: SkillsConfigSection,
    pub tool_transcript: ToolTranscriptConfig,
    pub agent_thinking_trace: AgentThinkingTraceConfig,
    pub agent_tool_stats: AgentToolStatsConfig,
    pub dsml_materialize: DsmlMaterializeConfig,
    pub thinking_echo: ThinkingEchoConfig,
    pub context_pipeline: ContextPipelineConfig,
    pub workspace_roots: WorkspaceRootsConfig,
    pub web_api: WebApiConfig,
    pub chat_queues_cache: ChatQueuesCacheConfig,
    pub session_workspace_changelist: SessionWorkspaceChangelistConfig,
    pub sync_tool_sandbox: SyncToolSandboxConfig,
    pub conversation_persistence: ConversationPersistenceConfig,
    pub context_bootstrap_inject: ContextBootstrapInjectConfig,
    pub tool_call_explain: ToolCallExplainConfig,
    pub long_term_memory: LongTermMemoryConfig,
    pub mcp_client: McpClientConfig,
    pub codebase_semantic: CodebaseSemanticConfig,
    pub tool_registry_policy: ToolRegistryPolicyConfig,
    pub turn_budget: TurnBudgetConfig,
    pub hierarchy_routing: HierarchyRoutingConfig,
    pub intent_routing: IntentRoutingConfig,
}

impl AgentConfig {
    /// 将会话同步管道使用的「非 system 近似字符预算」：显式 `context_char_budget` 与按 [`Self::llm_sampling.llm_context_tokens`] 推导值取**更小者**（任一者为 `0` 则忽略该侧上限）。
    ///
    /// 推导：`≈ max(0, llm_context_tokens − max_tokens) × 4`（字节近似 UTF-8，偏保守），并封顶 `50_000_000`。
    #[must_use]
    pub fn effective_context_char_budget_for_pipeline(&self) -> usize {
        let explicit = self.context_pipeline.context_char_budget;
        let ctx = u64::from(self.llm_sampling.llm_context_tokens);
        let derived = Self::approx_non_system_chars_budget_from_context_tokens(
            ctx,
            u64::from(self.llm_sampling.max_tokens),
        );
        match (explicit, derived) {
            (0, 0) => 0,
            (0, d) => d,
            (e, 0) => e,
            (e, d) => e.min(d),
        }
    }

    /// 供 [`maybe_summarize_with_llm`] 使用的摘要触发阈值：若配置中 `context_summary_trigger_chars > 0` 则沿用；否则在 [`Self::llm_sampling.llm_context_tokens`] 非零时取推导字符预算的约一半（下限 8192，且不超过推导预算）。
    #[must_use]
    pub fn effective_context_summary_trigger_chars(&self) -> usize {
        let explicit = self.context_pipeline.context_summary_trigger_chars;
        if explicit > 0 {
            return explicit;
        }
        let ctx = u64::from(self.llm_sampling.llm_context_tokens);
        if ctx == 0 {
            return 0;
        }
        let cap = Self::approx_non_system_chars_budget_from_context_tokens(
            ctx,
            u64::from(self.llm_sampling.max_tokens),
        );
        if cap == 0 {
            return 0;
        }
        let half = cap / 2;
        half.max(8_192).min(cap)
    }

    fn approx_non_system_chars_budget_from_context_tokens(
        context_tokens: u64,
        max_output_tokens: u64,
    ) -> usize {
        if context_tokens == 0 {
            return 0;
        }
        let ctx = context_tokens.max(1024);
        let mo = max_output_tokens.min(ctx.saturating_sub(256));
        let input_tokens = ctx.saturating_sub(mo).max(256);
        let chars = (input_tokens as u128).saturating_mul(4);
        let cap = 50_000_000usize;
        chars.min(cap as u128) as usize
    }

    /// 新建 Web/CLI 会话首条 `system` 的正文来源。
    ///
    /// - 显式 `agent_role`：须在 [`Self::roles_prompts.agent_roles`] 中存在，否则返回 `Err`。
    /// - 未指定：使用 [`Self::roles_prompts.default_agent_role_id`] 对应条目（若配置且存在），否则 [`Self::roles_prompts.system_prompt`]。
    pub fn system_prompt_for_new_conversation(
        &self,
        agent_role: Option<&str>,
    ) -> Result<&str, String> {
        let rp = &self.roles_prompts;
        match agent_role.map(str::trim).filter(|s| !s.is_empty()) {
            Some(id) => rp
                .agent_roles
                .get(id)
                .map(|s| s.system_prompt.as_str())
                .ok_or_else(|| format!("未知的 agent_role: {id}（请在配置中定义该 id）")),
            None => Ok(rp
                .default_agent_role_id
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .and_then(|id| rp.agent_roles.get(id))
                .map(|s| s.system_prompt.as_str())
                .unwrap_or(rp.system_prompt.as_str())),
        }
    }
}

#[cfg(test)]
mod llm_context_budget_tests {
    #[test]
    fn effective_context_char_budget_is_min_of_explicit_and_derived() {
        let mut cfg = crate::load_config(None).expect("embed default config");
        cfg.llm_sampling.max_tokens = 2048;
        cfg.llm_sampling.llm_context_tokens = 100_000;
        cfg.context_pipeline.context_char_budget = 1000;
        assert_eq!(cfg.effective_context_char_budget_for_pipeline(), 1000);
        cfg.context_pipeline.context_char_budget = 0;
        assert!(
            cfg.effective_context_char_budget_for_pipeline() > 1000,
            "derived budget should exceed 1000 when explicit is off"
        );
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

    #[cfg(feature = "fastembed")]
    #[test]
    fn vector_backend_default_is_fastembed() {
        assert_eq!(
            LongTermMemoryVectorBackend::default(),
            LongTermMemoryVectorBackend::Fastembed
        );
    }

    #[cfg(not(feature = "fastembed"))]
    #[test]
    fn vector_backend_default_is_disabled_without_fastembed() {
        assert_eq!(
            LongTermMemoryVectorBackend::default(),
            LongTermMemoryVectorBackend::Disabled
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
