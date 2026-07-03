//! 运行配置：API 地址、模型等，从 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 嵌入默认 + 可选覆盖

pub mod agent_role_spec;
mod agent_roles;
mod assembly;
mod builder;
pub mod cli;
mod cursor_rules;
mod env_overrides;
mod final_plan_requirement_mode;
mod finalize;
mod gateway_hints;
mod hot_reload;
mod load;
mod orchestration_profile;
mod scheduled_agent_task;
pub mod skills;
mod source;
mod text_util;
mod types;
mod user_config_layers;
mod validate;
mod workspace_roots;

pub use final_plan_requirement_mode::FinalPlanRequirementMode;
pub use gateway_hints::{
    default_llm_reasoning_split_for_gateway, fold_system_into_user_for_config,
    is_minimax_family_model_id,
};
pub use hot_reload::apply_hot_reload_config_subset;
pub use load::{load_config, load_config_for_cli};
pub use orchestration_profile::{OrchestrationProfile, effective_orchestration_path_summary};

pub use finalize::embedded_thinking_avoid_echo_appendix;

#[allow(unused_imports)] // 部分类型仅对外再导出，本文件内不直接使用
pub use agent_role_spec::AgentRoleSpec;
#[allow(unused_imports)]
pub use types::{
    AgentConfig, AgentThinkingTraceConfig, AgentToolStatsConfig, ChatQueuesCacheConfig,
    CodebaseSemanticConfig, CommandExecConfig, ContextBootstrapInjectConfig, ContextPipelineConfig,
    ConversationPersistenceConfig, CursorRulesConfigSection, DsmlMaterializeConfig, ExposeSecret,
    HierarchyRoutingConfig, HttpFetchConfigSection, IntentRoutingConfig, LlmConnectionConfig,
    LlmHttpAuthMode, LlmHttpRetryConfig, LlmSamplingConfig, LlmVendorFlagsConfig,
    LongTermMemoryConfig, LongTermMemoryScopeMode, LongTermMemoryVectorBackend, McpClientConfig,
    PerPlanPolicyConfig, PlannerExecutorMode, RolesPromptsConfig, ScheduledAgentTask,
    SessionUiConfig, SessionWorkspaceChangelistConfig, SkillsConfigSection, StagedPlanBaselineMode,
    StagedPlanFeedbackMode, StagedPlanningConfig, SyncDefaultToolSandboxMode,
    SyncToolSandboxConfig, ThinkingEchoConfig, ToolCallExplainConfig, ToolRegistryPolicyConfig,
    ToolTranscriptConfig, TurnBudgetConfig, WeatherToolConfig, WebApiConfig,
    WebSearchConfigSection, WebSearchProvider, WorkspaceRootsConfig,
};

/// 进程内共享的 [`AgentConfig`]（`serve` / `repl` / `chat` / `bench`）；热重载时 `write` 更新，回合开始时 `read`+`clone` 得快照传入 `run_agent_turn`。
pub type SharedAgentConfig = std::sync::Arc<tokio::sync::RwLock<AgentConfig>>;

#[cfg(test)]
mod embedded_shard_parse_tests {
    use super::assembly;
    use super::builder::ConfigBuilder;

    #[test]
    fn malformed_embedded_toml_returns_err_naming_shard() {
        let mut b = ConfigBuilder::default();
        let err =
            assembly::apply_embedded_agent_shard_for_test(&mut b, "test_shard.toml", "[[[not toml")
                .unwrap_err();
        assert!(
            err.contains("test_shard.toml"),
            "expected shard label in error: {err}"
        );
        assert!(
            err.contains("嵌入默认配置"),
            "expected Chinese prefix in error: {err}"
        );
    }
}

#[cfg(test)]
mod web_api_require_bearer_defaults_tests {
    use super::load_config;
    use std::fs;
    use std::sync::Mutex;

    /// `load_config` 会读 `CM_WEB_API_REQUIRE_BEARER`（优先级高于 TOML）；开发者本机若导出 `0`/`false` 会覆盖嵌入默认，导致「仅测嵌入默认」的断言不稳定。
    static CM_WEB_API_REQUIRE_BEARER_LOCK: Mutex<()> = Mutex::new(());

    fn without_cm_web_api_require_bearer_env<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _g = CM_WEB_API_REQUIRE_BEARER_LOCK
            .lock()
            .expect("web_api_require_bearer defaults tests must run serialized");
        let prev = std::env::var("CM_WEB_API_REQUIRE_BEARER").ok();
        // SAFETY: `remove_var`/`set_var` are unsafe in Rust 2024; we hold the mutex so no other
        // test in this module touches `CM_WEB_API_REQUIRE_BEARER` during `f()`.
        unsafe {
            std::env::remove_var("CM_WEB_API_REQUIRE_BEARER");
        }
        let out = f();
        unsafe {
            match prev.as_ref() {
                Some(v) => std::env::set_var("CM_WEB_API_REQUIRE_BEARER", v),
                None => std::env::remove_var("CM_WEB_API_REQUIRE_BEARER"),
            }
        }
        out
    }

    #[test]
    fn embedded_default_does_not_require_bearer_without_env_override() {
        without_cm_web_api_require_bearer_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("minimal.toml");
            fs::write(
                &path,
                r#"[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-chat"
"#,
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(
                !cfg.web_api.web_api_require_bearer,
                "embedded default should allow serve without forcing non-empty web_api_bearer_token"
            );
        });
    }

    #[test]
    fn explicit_true_requires_bearer_secret_at_serve() {
        without_cm_web_api_require_bearer_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("minimal.toml");
            fs::write(
                &path,
                r#"[agent]
api_base = "https://api.deepseek.com/v1"
model = "deepseek-chat"
web_api_require_bearer = true
"#,
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(cfg.web_api.web_api_require_bearer);
        });
    }
}

#[cfg(test)]
mod llm_reasoning_split_default_tests {
    use super::load_config;
    use std::fs;
    use std::sync::Mutex;

    /// `load_config` 会读 `CM_LLM_REASONING_SPLIT`；本机/CI 若导出 `0` 会覆盖「省略键时按网关推断」的断言。
    static CM_LLM_REASONING_SPLIT_LOCK: Mutex<()> = Mutex::new(());

    /// 临时清除 `CM_LLM_REASONING_SPLIT`，避免本机/CI 导出干扰；结束或 panic 后恢复原值。
    ///
    /// 使用 `catch_unwind`：断言失败时仍释放 [`Mutex`]，避免毒化导致后续用例 `PoisonError`。
    fn without_cm_llm_reasoning_split_env<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let result = {
            let _guard = CM_LLM_REASONING_SPLIT_LOCK
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            let prev = std::env::var("CM_LLM_REASONING_SPLIT").ok();
            // SAFETY: `remove_var`/`set_var` are unsafe in Rust 2024.
            unsafe {
                std::env::remove_var("CM_LLM_REASONING_SPLIT");
            }
            struct RestoreEnv(Option<String>);
            impl Drop for RestoreEnv {
                fn drop(&mut self) {
                    unsafe {
                        match self.0.take() {
                            Some(v) => std::env::set_var("CM_LLM_REASONING_SPLIT", v),
                            None => std::env::remove_var("CM_LLM_REASONING_SPLIT"),
                        }
                    }
                }
            }
            let _restore = RestoreEnv(prev);
            let mut f_opt = Some(f);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let fun = f_opt
                    .take()
                    .expect("without_cm_llm_reasoning_split_env closure");
                fun()
            }))
        };

        match result {
            Ok(v) => v,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    }

    #[test]
    fn finalize_respects_omitted_reasoning_split_for_non_minimax() {
        assert!(
            !crate::gateway_hints::default_llm_reasoning_split_for_gateway(
                "deepseek-chat",
                "https://api.deepseek.com/v1",
            )
        );
    }

    #[test]
    fn minimax_user_toml_without_key_defaults_true() {
        without_cm_llm_reasoning_split_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("agent.toml");
            fs::write(
                &path,
                r#"[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"
"#,
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(
                cfg.llm_vendor_flags.llm_reasoning_split,
                "MiniMax 网关未写 llm_reasoning_split 时应默认 true"
            );
            assert!(
                crate::gateway_hints::fold_system_into_user_for_config(&cfg),
                "MiniMax 应自动折叠 system→user"
            );
        });
    }

    #[test]
    fn minimax_user_toml_explicit_false() {
        without_cm_llm_reasoning_split_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("agent.toml");
            fs::write(
                &path,
                r#"[agent]
api_base = "https://api.minimaxi.com/v1"
model = "MiniMax-M2.7"
llm_reasoning_split = false
"#,
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(!cfg.llm_vendor_flags.llm_reasoning_split);
        });
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::ScheduledAgentTask;
    use super::{apply_hot_reload_config_subset, load_config};

    #[test]
    fn apply_hot_reload_keeps_conversation_store_path() {
        let base = load_config(None).expect("default config");
        let mut dst = base.clone();
        let frozen = dst
            .conversation_persistence
            .conversation_store_sqlite_path
            .clone();
        let mut src = dst.clone();
        src.conversation_persistence.conversation_store_sqlite_path =
            "/tmp/should_not_apply.sqlite".to_string();
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(
            dst.conversation_persistence.conversation_store_sqlite_path,
            frozen
        );
    }

    /// 组合式配置下：各子结构应从 `src` 整段替换；仅会话库路径保留 `dst` 原值。
    #[test]
    fn apply_hot_reload_updates_sections_but_not_sqlite_path() {
        let mut dst = load_config(None).expect("default config");
        let frozen_store = dst
            .conversation_persistence
            .conversation_store_sqlite_path
            .clone();
        let mut src = dst.clone();
        src.llm.model = "hot-reload-model-snapshot".to_string();
        src.session_ui.max_message_history = src.session_ui.max_message_history.saturating_add(3);
        src.weather_tool.weather_timeout_secs =
            src.weather_tool.weather_timeout_secs.saturating_add(11);
        src.conversation_persistence.conversation_store_sqlite_path =
            "/tmp/ignored_on_hot_reload.sqlite".to_string();
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(
            dst.conversation_persistence.conversation_store_sqlite_path,
            frozen_store
        );
        assert_eq!(dst.llm.model, src.llm.model);
        assert_eq!(
            dst.session_ui.max_message_history,
            src.session_ui.max_message_history
        );
        assert_eq!(
            dst.weather_tool.weather_timeout_secs,
            src.weather_tool.weather_timeout_secs
        );
    }

    #[test]
    fn apply_hot_reload_updates_scheduled_tasks_from_src() {
        let mut dst = load_config(None).expect("default config");
        dst.conversation_persistence.scheduled_agent_tasks.clear();
        let mut src = dst.clone();
        src.conversation_persistence.scheduled_agent_tasks = vec![ScheduledAgentTask {
            id: "cron_smoke".to_string(),
            schedule: "0 0 * * * *".to_string(),
            message: "hello".to_string(),
            conversation_id: None,
            new_conversation: false,
            agent_role: None,
        }];
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(dst.conversation_persistence.scheduled_agent_tasks.len(), 1);
        assert_eq!(
            dst.conversation_persistence.scheduled_agent_tasks[0].id,
            "cron_smoke"
        );
    }
}

#[cfg(test)]
mod context_budget_warning_tests {
    use super::finalize::context_budget_vs_history_suspicious;

    #[test]
    fn suspicious_when_budget_on_and_min_ge_max_history() {
        assert!(context_budget_vs_history_suspicious(8, 100_000, 8));
        assert!(context_budget_vs_history_suspicious(8, 1, 10));
    }

    #[test]
    fn not_suspicious_when_budget_off() {
        assert!(!context_budget_vs_history_suspicious(8, 0, 100));
    }

    #[test]
    fn not_suspicious_when_min_below_max_history() {
        assert!(!context_budget_vs_history_suspicious(32, 50_000, 4));
    }
}

#[cfg(test)]
mod numeric_validate_tests {
    use super::builder::ConfigBuilder;
    use super::validate;
    use std::collections::HashMap;

    #[test]
    fn rejects_temperature_above_two() {
        let mut b = ConfigBuilder::default();
        b.llm_sampling.temperature = Some(3.0);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("temperature"), "err: {err}");
    }

    #[test]
    fn parallel_wall_timeout_out_of_range() {
        let mut m = HashMap::new();
        m.insert("http_fetch_spawn_timeout".into(), 100_000u64);
        let mut b = ConfigBuilder::default();
        b.tool_registry_policy
            .tool_registry_parallel_wall_timeout_secs = m;
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("parallel_wall_timeout_secs"), "err: {err}");
    }
}
