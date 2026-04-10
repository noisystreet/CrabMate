//! 运行配置：API 地址、模型等，从 `config/default_config.toml`、`config/session.toml`、`config/context_inject.toml`、`config/tools.toml`、`config/sandbox.toml`、`config/planning.toml`、`config/memory.toml` 嵌入默认 + 可选覆盖

pub mod agent_role_spec;
mod agent_roles;
mod assembly;
mod builder;
pub mod cli;
mod cursor_rules;
mod env_overrides;
mod finalize;
mod hot_reload;
mod load;
mod source;
mod types;
mod validate;
mod workspace_roots;

pub use hot_reload::apply_hot_reload_config_subset;
pub use load::{load_config, load_config_for_cli};

pub(crate) use finalize::embedded_thinking_avoid_echo_appendix;

#[allow(unused_imports)] // 部分类型仅对外再导出，本文件内不直接使用
pub use agent_role_spec::AgentRoleSpec;
#[allow(unused_imports)]
pub use types::{
    AgentConfig, ExposeSecret, LlmHttpAuthMode, LongTermMemoryScopeMode,
    LongTermMemoryVectorBackend, PlannerExecutorMode, StagedPlanFeedbackMode,
    SyncDefaultToolSandboxMode, WebSearchProvider,
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
mod llm_reasoning_split_default_tests {
    use super::load_config;
    use std::fs;

    #[test]
    fn finalize_respects_omitted_reasoning_split_for_non_minimax() {
        assert!(
            !crate::llm::vendor::default_llm_reasoning_split_for_gateway(
                "deepseek-chat",
                "https://api.deepseek.com/v1",
            )
        );
    }

    #[test]
    fn minimax_user_toml_without_key_defaults_true() {
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
            cfg.llm_reasoning_split,
            "MiniMax 网关未写 llm_reasoning_split 时应默认 true"
        );
        assert!(
            crate::llm::fold_system_into_user_for_config(&cfg),
            "MiniMax 应自动折叠 system→user"
        );
    }

    #[test]
    fn minimax_user_toml_explicit_false() {
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
        assert!(!cfg.llm_reasoning_split);
    }
}

#[cfg(test)]
mod hot_reload_tests {
    use super::{apply_hot_reload_config_subset, load_config};

    #[test]
    fn apply_hot_reload_keeps_conversation_store_path() {
        let base = load_config(None).expect("default config");
        let mut dst = base.clone();
        let frozen = dst.conversation_store_sqlite_path.clone();
        let mut src = dst.clone();
        src.conversation_store_sqlite_path = "/tmp/should_not_apply.sqlite".to_string();
        apply_hot_reload_config_subset(&mut dst, &src);
        assert_eq!(dst.conversation_store_sqlite_path, frozen);
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
        let b = ConfigBuilder {
            temperature: Some(3.0),
            ..Default::default()
        };
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("temperature"), "err: {err}");
    }

    #[test]
    fn parallel_wall_timeout_out_of_range() {
        let mut m = HashMap::new();
        m.insert("http_fetch_spawn_timeout".into(), 100_000u64);
        let b = ConfigBuilder {
            tool_registry_parallel_wall_timeout_secs: m,
            ..Default::default()
        };
        let err = validate::validate_builder_numeric_ranges(&b).unwrap_err();
        assert!(err.contains("parallel_wall_timeout_secs"), "err: {err}");
    }
}
