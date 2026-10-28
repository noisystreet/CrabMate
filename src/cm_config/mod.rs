//! 运行配置：API 地址、模型等，从 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 嵌入默认 + 可选覆盖。
//! 厂商匹配与常用 **`models`** 列表见独立嵌入 **`config/llm_vendors.toml`**（`cm_llm::vendor_catalog`，不并入本模块 `[agent]` 合并）。

pub mod agent_role_spec;
mod agent_roles;
mod assembly;
mod builder;
pub mod cli;
mod env_override_apply;
mod env_overrides;
mod final_plan_requirement_mode;
mod finalize;
mod gateway_hints;
mod hot_reload;
mod load;
mod permission;
mod scheduled_agent_task;
pub mod skills;
pub mod skills_slash;
mod source;
pub(crate) mod text_util;
pub mod tool_write_effect;
mod types;
mod user_config_layers;
mod user_config_xdg;
mod validate;
mod web_cors;
mod workspace_roots;
mod xdg;

pub use web_cors::{DEFAULT_SHELL_CORS_ORIGINS, resolve_web_cors_allowed_origins};
pub use workspace_roots::is_within_allowed_roots;

pub use final_plan_requirement_mode::FinalPlanRequirementMode;
pub use gateway_hints::{
    default_llm_reasoning_split_for_gateway, fold_system_into_user_for_config,
};
pub use hot_reload::apply_hot_reload_config_subset;
pub use load::{load_config, load_config_for_cli};
pub use permission::{
    clamp_permission_ceiling_on_reload, freeze_permission_ceiling_for_bind,
    reconcile_default_level_with_ceiling, resolve_permission_ceiling, validate_permission_config,
};
pub use user_config_xdg::{
    ENV_SKIP_CONFIG_SEED, SYSTEM_CONFIG_DIR, cwd_has_local_user_config,
    cwd_in_crabmate_source_tree, ensure_user_config_seeded_from,
    ensure_user_config_seeded_from_system, resolve_default_user_config_toml_after_seed,
    resolve_interactive_cli_config_path, should_resolve_xdg_user_config, system_config_toml_path,
    user_config_dir, user_config_toml_path,
};
pub use xdg::{
    ENV_CACHE_DIR, ENV_CONFIG_DIR, ensure_fastembed_cache_dir, ensure_user_cache_subdir,
    user_cache_dir,
};

pub use finalize::{
    embedded_context_summary_system, embedded_context_summary_user_template,
    embedded_session_mode_appendix, embedded_thinking_avoid_echo_appendix,
};

#[allow(unused_imports)] // 部分类型仅对外再导出，本文件内不直接使用
pub use agent_role_spec::AgentRoleSpec;
#[allow(unused_imports)]
pub use types::{
    AgentsMdConfigSection, AgentConfig, AgentThinkingTraceConfig, AgentToolStatsConfig,
    ChatQueuesCacheConfig, CodebaseSemanticConfig, CommandExecConfig, ContextBootstrapInjectConfig,
    ContextPipelineConfig, ConversationPersistenceConfig, ExposeSecret, HttpFetchConfigSection,
    LlmConnectionConfig, LlmHttpAuthMode, LlmHttpRetryConfig, LlmSamplingConfig,
    LlmVendorFlagsConfig, LongTermMemoryConfig, LongTermMemoryVectorBackend, McpClientConfig,
    PerPlanPolicyConfig, PermissionConfig, RolesPromptsConfig, ScheduledAgentTask, SessionUiConfig,
    SessionWorkspaceChangelistConfig, SkillsConfigSection, SyncDefaultToolSandboxMode,
    SyncToolSandboxConfig, ThinkingEchoConfig, ToolCallExplainConfig, ToolRegistryPolicyConfig,
    ToolTranscriptConfig, TurnBudgetConfig, WeatherToolConfig, WebApiConfig, WebSearchConfigSection,
    WebSearchProvider, WorkspaceRootsConfig,
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
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use std::fs;

    /// `load_config` 会读 `CM_WEB_API_REQUIRE_BEARER`（优先级高于 TOML）；开发者本机若导出 `0`/`false` 会覆盖嵌入默认，导致「仅测嵌入默认」的断言不稳定。
    ///
    /// 这里用线程局部环境视图屏蔽 `CM_*` 覆盖：既不写进程环境（不干扰并行用例），也不再需要互斥锁。
    fn without_cm_web_api_require_bearer_env<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _env = scoped_env::scope(&[]);
        f()
    }

    #[test]
    fn embedded_default_does_not_require_bearer_without_env_override() {
        without_cm_web_api_require_bearer_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("minimal.toml");
            fs::write(
                &path,
                "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n",
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(
                !cfg.web_api.web_api_require_bearer,
                "embedded default should allow serve without forcing non-empty web_api_bearer_token"
            );
            assert_eq!(
                cfg.web_api.web_cors_allowed_origins,
                crate::cm_config::DEFAULT_SHELL_CORS_ORIGINS
                    .iter()
                    .map(|s| (*s).to_string())
                    .collect::<Vec<_>>(),
                "embedded default should allow official Client shell Origins"
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
                "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\nweb_api_require_bearer = true\n",
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(cfg.web_api.web_api_require_bearer);
        });
    }

    #[test]
    fn toml_web_cors_allowed_origins_are_loaded() {
        without_cm_web_api_require_bearer_env(|| {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join("cors.toml");
            fs::write(
                &path,
                "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\nweb_cors_allowed_origins = [\"http://127.0.0.1:8081\", \"https://ui.example.com\"]\n",
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert_eq!(
                cfg.web_api.web_cors_allowed_origins,
                vec![
                    "http://127.0.0.1:8081".to_string(),
                    "https://ui.example.com".to_string(),
                    "tauri://localhost".to_string(),
                    "http://tauri.localhost".to_string(),
                ]
            );
        });
    }
}

#[cfg(test)]
pub(crate) mod load_config_test_env {
    use super::env_override_apply::scoped_env;

    /// 在**当前线程**内屏蔽经 `env_ok` 读取的 `CM_*` 覆盖后运行 `f`。
    ///
    /// 不写进程环境，故并行用例互不可见，无需互斥锁。
    pub(crate) fn without_cm_env_overrides<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _env = scoped_env::scope(&[]);
        f()
    }
}

#[cfg(test)]
mod removed_config_key_load_tests {
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use std::fs;

    #[test]
    fn load_ignores_removed_env_overrides() {
        // `CM_PLANNER_EXECUTOR_MODE` / `CM_LONG_TERM_MEMORY_SCOPE_MODE` 已移除；
        // 即使仍设置历史值，也不再影响加载。
        let _env = scoped_env::scope(&[
            ("CM_PLANNER_EXECUTOR_MODE", "logical_dual_agent"),
            ("CM_LONG_TERM_MEMORY_SCOPE_MODE", "tenant"),
        ]);
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("removed_env.toml");
        fs::write(
            &path,
            "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n",
        )
        .expect("write");
        load_config(Some(path.to_str().unwrap())).expect("removed env keys must be ignored");
    }
}

#[cfg(test)]
mod http_fetch_user_agent_load_tests {
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use std::fs;

    /// `load_config` 会读 `CM_HTTP_FETCH_USER_AGENT`；用线程局部环境视图覆盖该键，不写进程环境。
    fn with_http_fetch_user_agent_env<F, R>(value: Option<&str>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _env = match value {
            Some(v) => scoped_env::scope(&[("CM_HTTP_FETCH_USER_AGENT", v)]),
            None => scoped_env::scope(&[]),
        };
        f()
    }

    fn write_minimal_config(dir: &tempfile::TempDir) -> String {
        let path = dir.path().join("http_fetch_ua.toml");
        fs::write(
            &path,
            "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n",
        )
        .expect("write");
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn http_fetch_user_agent_defaults_to_crabmate_version() {
        with_http_fetch_user_agent_env(None, || {
            let dir = tempfile::tempdir().expect("tempdir");
            let cfg = load_config(Some(&write_minimal_config(&dir))).expect("load");
            assert_eq!(
                cfg.http_fetch.http_fetch_user_agent,
                format!("crabmate/{}", env!("CARGO_PKG_VERSION"))
            );
        });
    }

    #[test]
    fn http_fetch_user_agent_env_override() {
        with_http_fetch_user_agent_env(Some("Mozilla/5.0 verify"), || {
            let dir = tempfile::tempdir().expect("tempdir");
            let cfg = load_config(Some(&write_minimal_config(&dir))).expect("load");
            assert_eq!(cfg.http_fetch.http_fetch_user_agent, "Mozilla/5.0 verify");
        });
    }
}

#[cfg(test)]
mod http_fetch_allowed_prefixes_load_tests {
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use std::fs;

    /// `load_config` 会读 `CM_HTTP_FETCH_ALLOWED_PREFIXES`；用线程局部环境视图覆盖该键，不写进程环境。
    fn with_http_fetch_allowed_prefixes_env<F, R>(value: Option<&str>, f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _env = match value {
            Some(v) => scoped_env::scope(&[("CM_HTTP_FETCH_ALLOWED_PREFIXES", v)]),
            None => scoped_env::scope(&[]),
        };
        f()
    }

    fn write_agent_toml(dir: &tempfile::TempDir, extra: &str) -> String {
        let path = dir.path().join("http_fetch_prefixes.toml");
        fs::write(
            &path,
            format!(
                "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\n{extra}\n"
            ),
        )
        .expect("write");
        path.to_str().unwrap().to_string()
    }

    #[test]
    fn embed_default_http_fetch_prefixes_is_star() {
        with_http_fetch_allowed_prefixes_env(None, || {
            let dir = tempfile::tempdir().expect("tempdir");
            let cfg = load_config(Some(&write_agent_toml(&dir, ""))).expect("load");
            assert_eq!(
                cfg.http_fetch.http_fetch_allowed_prefixes,
                vec!["*".to_string()]
            );
        });
    }

    #[test]
    fn toml_empty_http_fetch_prefixes_clears_star_default() {
        with_http_fetch_allowed_prefixes_env(None, || {
            let dir = tempfile::tempdir().expect("tempdir");
            let cfg = load_config(Some(&write_agent_toml(
                &dir,
                "http_fetch_allowed_prefixes = []",
            )))
            .expect("load");
            assert!(cfg.http_fetch.http_fetch_allowed_prefixes.is_empty());
        });
    }

    #[test]
    fn env_empty_http_fetch_prefixes_clears_star_default() {
        with_http_fetch_allowed_prefixes_env(Some(""), || {
            let dir = tempfile::tempdir().expect("tempdir");
            let cfg = load_config(Some(&write_agent_toml(&dir, ""))).expect("load");
            assert!(cfg.http_fetch.http_fetch_allowed_prefixes.is_empty());
        });
    }
}

#[cfg(test)]
mod llm_reasoning_split_default_tests {
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use std::fs;

    /// 屏蔽 `CM_LLM_REASONING_SPLIT`（以及其它 `CM_*` 覆盖），避免本机/CI 导出干扰；仅作用于当前线程。
    fn without_cm_llm_reasoning_split_env<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        let _env = scoped_env::scope(&[]);
        f()
    }

    #[test]
    fn finalize_respects_omitted_reasoning_split_for_non_minimax() {
        assert!(
            !crate::cm_config::gateway_hints::default_llm_reasoning_split_for_gateway(
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
                "[agent]\napi_base = \"https://api.minimaxi.com/v1\"\nmodel = \"MiniMax-M2.7\"\n",
            )
            .expect("write");
            let cfg = load_config(Some(path.to_str().unwrap())).expect("load");
            assert!(
                cfg.llm_vendor_flags.llm_reasoning_split,
                "MiniMax 网关未写 llm_reasoning_split 时应默认 true"
            );
            assert!(
                crate::cm_config::gateway_hints::fold_system_into_user_for_config(
                    &cfg.llm.model,
                    &cfg.llm.api_base
                ),
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
                "[agent]\napi_base = \"https://api.minimaxi.com/v1\"\nmodel = \"MiniMax-M2.7\"\nllm_reasoning_split = false\n",
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
    use super::load_config_test_env::without_cm_env_overrides;
    use super::{apply_hot_reload_config_subset, load_config};

    #[test]
    fn apply_hot_reload_keeps_conversation_store_path() {
        let base = without_cm_env_overrides(|| load_config(None).expect("default config"));
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
        let mut dst = without_cm_env_overrides(|| load_config(None).expect("default config"));
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
        let mut dst = without_cm_env_overrides(|| load_config(None).expect("default config"));
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

    /// C3：`context_archive_*` 两键可热更（从 `src` 覆盖 `dst`）。
    #[test]
    fn apply_hot_reload_updates_context_archive_keys() {
        let mut dst = without_cm_env_overrides(|| load_config(None).expect("default config"));
        let mut src = dst.clone();
        src.conversation_persistence
            .context_archive_max_rows_per_conversation = 13;
        src.conversation_persistence.context_archive_codec_min_bytes = 9999;
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(
            dst.conversation_persistence
                .context_archive_max_rows_per_conversation,
            13
        );
        assert_eq!(
            dst.conversation_persistence.context_archive_codec_min_bytes,
            9999
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

    #[test]
    fn background_job_max_concurrent_out_of_range() {
        let mut b = ConfigBuilder::default();
        b.tool_registry_policy
            .tool_registry_background_job_max_concurrent = Some(1000);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(
            err.contains("background_job_max_concurrent"),
            "err: {err}"
        );
    }

    #[test]
    fn tool_retry_ranges_out_of_range() {
        let mut b = ConfigBuilder::default();
        b.tool_registry_policy.tool_registry_tool_retry_max_attempts = Some(9);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("tool_retry_max_attempts"), "err: {err}");

        let mut b = ConfigBuilder::default();
        b.tool_registry_policy.tool_registry_tool_retry_backoff_ms = Some(20_000);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("tool_retry_backoff_ms"), "err: {err}");
    }

    /// C3：归档保留键的范围校验（含 `0` 合法、越界报错）。
    #[test]
    fn context_archive_keys_out_of_range() {
        let mut b = ConfigBuilder::default();
        b.conversation_persistence
            .context_archive_max_rows_per_conversation = Some(10_000_001);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(
            err.contains("context_archive_max_rows_per_conversation"),
            "err: {err}"
        );

        let mut b = ConfigBuilder::default();
        b.conversation_persistence.context_archive_codec_min_bytes = Some(4_194_305);
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(
            err.contains("context_archive_codec_min_bytes"),
            "err: {err}"
        );
    }

    /// `context_archive_max_rows_per_conversation = 0`（不限）不应被判为越界。
    #[test]
    fn context_archive_zero_rows_is_allowed() {
        let mut b = ConfigBuilder::default();
        b.conversation_persistence
            .context_archive_max_rows_per_conversation = Some(0);
        assert!(validate::validate_builder_numeric_ranges(&b).is_ok());
    }
}

/// C3：`context_archive_*` 两键的加载管道回归（默认值 / TOML / env 覆盖）。
#[cfg(test)]
mod context_archive_config_tests {
    use super::env_override_apply::scoped_env;
    use super::load_config;
    use super::load_config_test_env::without_cm_env_overrides;
    use std::fs;

    fn write_agent_toml(body: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("agent.toml");
        fs::write(&path, body).expect("write");
        let p = path.to_str().unwrap().to_string();
        (dir, p)
    }

    #[test]
    fn defaults_come_from_conversation_store_constants() {
        without_cm_env_overrides(|| {
            let cfg = load_config(None).expect("default config");
            assert_eq!(
                cfg.conversation_persistence
                    .context_archive_max_rows_per_conversation,
                crate::conversation_store::ARCHIVE_MAX_ROWS_PER_CONVERSATION_DEFAULT
            );
            assert_eq!(
                cfg.conversation_persistence.context_archive_codec_min_bytes,
                crate::conversation_store::ARCHIVE_CODEC_MIN_BYTES_DEFAULT
            );
        });
    }

    #[test]
    fn agent_toml_overrides_apply() {
        without_cm_env_overrides(|| {
            let (_dir, path) = write_agent_toml(
                "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\ncontext_archive_max_rows_per_conversation = 0\ncontext_archive_codec_min_bytes = 8192\n",
            );
            let cfg = load_config(Some(&path)).expect("load");
            assert_eq!(
                cfg.conversation_persistence
                    .context_archive_max_rows_per_conversation,
                0
            );
            assert_eq!(
                cfg.conversation_persistence.context_archive_codec_min_bytes,
                8192
            );
        });
    }

    #[test]
    fn env_overrides_win_over_toml() {
        let _env = scoped_env::scope(&[
            ("CM_CONTEXT_ARCHIVE_MAX_ROWS_PER_CONVERSATION", "7"),
            ("CM_CONTEXT_ARCHIVE_CODEC_MIN_BYTES", "1024"),
        ]);
        let (_dir, path) = write_agent_toml(
            "[agent]\napi_base = \"https://api.deepseek.com/v1\"\nmodel = \"deepseek-chat\"\ncontext_archive_max_rows_per_conversation = 128\ncontext_archive_codec_min_bytes = 4096\n",
        );
        let cfg = load_config(Some(&path)).expect("load");
        assert_eq!(
            cfg.conversation_persistence
                .context_archive_max_rows_per_conversation,
            7
        );
        assert_eq!(
            cfg.conversation_persistence.context_archive_codec_min_bytes,
            1024
        );
    }
}
