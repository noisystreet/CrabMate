//! 只读工具 `self_config_info`：把**运行时合并后**的 [`AgentConfig`]（环境变量覆盖、
//! 热重载、Web 密钥注入等均已生效）序列化为模型可读文本，供模型回答「自身配置」
//! 类问题（用什么模型 / 网关地址 / 采样参数 / 超时 / 工作区根等）。
//!
//! 与 [`super::diagnostics::diagnostic_summary`]（环境与工具链状态）互补。
//! **密钥类字段一律只报是否已设置，绝不输出值**（沿用 `diagnostic_summary` 的脱敏约定）。

use crate::cm_config::{AgentConfig, ExposeSecret};

/// 全部可用小节名（唯一真源：过滤、空输出提示、实现分发与一致性测试均由此派生）。
pub const SECTION_NAMES: &[&str] = &[
    "llm",
    "sampling",
    "vendor_flags",
    "http_retry",
    "command_exec",
    "weather",
    "web_search",
    "http_fetch",
    "per_plan_policy",
    "roles",
    "workspace",
    "web_api",
    "context_pipeline",
    "turn_budget",
    "long_term_memory",
    "conversation",
    "mcp",
    "tool_registry",
    "sandbox",
];

/// 汇总运行时配置为模型可读文本。
///
/// `sections` 为 `None` 时输出全部小节；否则仅输出指定小节名（未知名被忽略，
/// 输出为空时调用方应提示可用小节名）。
pub fn self_config_info(cfg: &AgentConfig, sections: Option<&[String]>) -> String {
    let mut out = String::new();
    for name in SECTION_NAMES {
        if sections.is_none_or(|s| s.iter().any(|x| x == name))
            && let Some(body) = section_body(cfg, name)
        {
            out.push_str(&format!("[{name}]\n{body}"));
        }
    }
    out.trim_end().to_string()
}

/// 按小节名取正文；未知名返回 `None`。
fn section_body(cfg: &AgentConfig, name: &str) -> Option<String> {
    match name {
        "llm" => Some(llm_section(cfg)),
        "sampling" => Some(sampling_section(cfg)),
        "vendor_flags" => Some(vendor_flags_section(cfg)),
        "http_retry" => Some(http_retry_section(cfg)),
        "command_exec" => Some(command_exec_section(cfg)),
        "weather" => Some(weather_section(cfg)),
        "web_search" => Some(web_search_section(cfg)),
        "http_fetch" => Some(http_fetch_section(cfg)),
        "per_plan_policy" => Some(per_plan_policy_section(cfg)),
        "roles" => Some(roles_section(cfg)),
        "workspace" => Some(workspace_section(cfg)),
        "web_api" => Some(web_api_section(cfg)),
        "context_pipeline" => Some(context_pipeline_section(cfg)),
        "turn_budget" => Some(turn_budget_section(cfg)),
        "long_term_memory" => Some(long_term_memory_section(cfg)),
        "conversation" => Some(conversation_section(cfg)),
        "mcp" => Some(mcp_section(cfg)),
        "tool_registry" => Some(tool_registry_section(cfg)),
        "sandbox" => Some(sandbox_section(cfg)),
        _ => None,
    }
}

fn llm_section(cfg: &AgentConfig) -> String {
    let l = &cfg.llm;
    format!(
        "model = {}\napi_base = {}\nauth_mode = {}\nplanner_model = {}\nexecutor_model = {}\n",
        l.model,
        l.api_base,
        l.llm_http_auth_mode.as_str(),
        l.planner_model.as_deref().unwrap_or("(继承 model)"),
        l.executor_model.as_deref().unwrap_or("(继承 model)"),
    )
}

fn sampling_section(cfg: &AgentConfig) -> String {
    let s = &cfg.llm_sampling;
    format!(
        "max_tokens = {}\ntemperature = {}\ncontext_tokens = {}\nseed = {}\n",
        s.max_tokens,
        s.temperature,
        s.llm_context_tokens,
        s.llm_seed.map_or("(未设置)".to_string(), |v| v.to_string()),
    )
}

fn vendor_flags_section(cfg: &AgentConfig) -> String {
    let v = &cfg.llm_vendor_flags;
    format!(
        "llm_reasoning_split = {}\nllm_bigmodel_thinking = {}\nllm_kimi_thinking_disabled = {}\n",
        v.llm_reasoning_split, v.llm_bigmodel_thinking, v.llm_kimi_thinking_disabled,
    )
}

fn http_retry_section(cfg: &AgentConfig) -> String {
    let r = &cfg.llm_http_retry;
    format!(
        "api_timeout_secs = {}\napi_max_retries = {}\napi_retry_delay_secs = {}\n",
        r.api_timeout_secs, r.api_max_retries, r.api_retry_delay_secs,
    )
}

fn command_exec_section(cfg: &AgentConfig) -> String {
    let c = &cfg.command_exec;
    format!(
        "command_timeout_secs = {}\ncommand_max_output_len = {}\nrun_command_working_dir = {}\nallowed_commands_count = {}\n",
        c.command_timeout_secs,
        c.command_max_output_len,
        c.run_command_working_dir,
        c.allowed_commands.len(),
    )
}

fn weather_section(cfg: &AgentConfig) -> String {
    format!(
        "weather_timeout_secs = {}\n",
        cfg.weather_tool.weather_timeout_secs
    )
}

fn web_search_section(cfg: &AgentConfig) -> String {
    let w = &cfg.web_search;
    let api_key = if w.web_search_api_key.expose_secret().is_empty() {
        "未设置"
    } else {
        "已设置(值隐藏)"
    };
    format!(
        "provider = {}\napi_key = {}\ntimeout_secs = {}\nmax_results = {}\n",
        w.web_search_provider.as_str(),
        api_key,
        w.web_search_timeout_secs,
        w.web_search_max_results,
    )
}

fn http_fetch_section(cfg: &AgentConfig) -> String {
    let h = &cfg.http_fetch;
    let prefixes = h
        .http_fetch_allowed_prefixes
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "allowed_prefixes = [{prefixes}]\ntimeout_secs = {}\nmax_response_bytes = {}\nuser_agent = {}\n",
        h.http_fetch_timeout_secs, h.http_fetch_max_response_bytes, h.http_fetch_user_agent,
    )
}

fn per_plan_policy_section(cfg: &AgentConfig) -> String {
    let p = &cfg.per_plan_policy;
    format!(
        "planner_executor_mode = {}\nreflection_default_max_rounds = {}\nfinal_plan_requirement = {:?}\nplan_rewrite_max_attempts = {}\n",
        p.planner_executor_mode.as_str(),
        p.reflection_default_max_rounds,
        p.final_plan_requirement,
        p.plan_rewrite_max_attempts,
    )
}

fn roles_section(cfg: &AgentConfig) -> String {
    let r = &cfg.roles_prompts;
    format!(
        "default_agent_role_id = {}\ncoding_workbench_enabled = {}\ndefault_session_mode = {:?}\nagent_roles_count = {}\n",
        r.default_agent_role_id.as_deref().unwrap_or("(未设置)"),
        r.coding_workbench_enabled,
        r.default_session_mode,
        r.agent_roles.len(),
    )
}

fn workspace_section(cfg: &AgentConfig) -> String {
    let w = &cfg.workspace_roots;
    let roots = w
        .workspace_allowed_roots
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let pool = w
        .web_workspace_pool
        .as_ref()
        .map_or("(未设置)".to_string(), |p| p.display().to_string());
    let chat_root = cfg
        .chat_workspace_root
        .as_ref()
        .map_or("(未设置)".to_string(), |p| p.display().to_string());
    format!(
        "allowed_roots = [{roots}]\nweb_workspace_pool = {pool}\nchat_workspace_root = {chat_root}\n"
    )
}

fn web_api_section(cfg: &AgentConfig) -> String {
    let w = &cfg.web_api;
    let bearer = if w.web_api_bearer_token.expose_secret().is_empty() {
        "未设置"
    } else {
        "已设置(值隐藏)"
    };
    let origins = w
        .web_cors_allowed_origins
        .iter()
        .map(|s| format!("{s:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "bearer_token = {bearer}\nrequire_bearer = {}\ncors_allowed_origins = [{origins}]\nallow_insecure_no_auth_for_non_loopback = {}\n",
        w.web_api_require_bearer, w.allow_insecure_no_auth_for_non_loopback,
    )
}

fn context_pipeline_section(cfg: &AgentConfig) -> String {
    let c = &cfg.context_pipeline;
    format!(
        "context_char_budget = {}\ncontext_summary_trigger_chars = {}\ncontext_summary_max_tokens = {}\n",
        c.context_char_budget, c.context_summary_trigger_chars, c.context_summary_max_tokens,
    )
}

fn turn_budget_section(cfg: &AgentConfig) -> String {
    let t = &cfg.turn_budget;
    format!(
        "max_turn_duration_seconds = {}\nmax_turn_tokens = {}\nmax_llm_calls_per_turn = {}\nmax_outer_loop_iterations = {}\n",
        t.max_turn_duration_seconds, t.max_turn_tokens, t.max_llm_calls_per_turn, t.max_outer_loop_iterations,
    )
}

fn long_term_memory_section(cfg: &AgentConfig) -> String {
    let m = &cfg.long_term_memory;
    format!(
        "enabled = {}\nscope_mode = {}\nvector_backend = {}\nmax_entries = {}\nstore_sqlite_path = {}\n",
        m.long_term_memory_enabled,
        m.long_term_memory_scope_mode.as_str(),
        m.long_term_memory_vector_backend.as_str(),
        m.long_term_memory_max_entries,
        m.long_term_memory_store_sqlite_path,
    )
}

fn conversation_section(cfg: &AgentConfig) -> String {
    format!(
        "conversation_store_sqlite_path = {}\nscheduled_agent_tasks_count = {}\n",
        cfg.conversation_persistence.conversation_store_sqlite_path,
        cfg.conversation_persistence.scheduled_agent_tasks.len(),
    )
}

fn mcp_section(cfg: &AgentConfig) -> String {
    let m = &cfg.mcp_client;
    format!(
        "enabled = {}\ncommand = {}\ntool_timeout_secs = {}\n",
        m.mcp_enabled, m.mcp_command, m.mcp_tool_timeout_secs,
    )
}

fn tool_registry_section(cfg: &AgentConfig) -> String {
    let t = &cfg.tool_registry_policy;
    format!(
        "background_jobs_enabled = {}\nparallel_wall_timeout_overrides_count = {}\n",
        t.tool_registry_background_jobs_enabled,
        t.tool_registry_parallel_wall_timeout_secs.len(),
    )
}

fn sandbox_section(cfg: &AgentConfig) -> String {
    let s = &cfg.sync_tool_sandbox;
    let docker_user = s
        .sync_default_tool_sandbox_docker_user
        .as_docker_user_string()
        .map_or("(镜像默认)".to_string(), |u| u.to_string());
    format!(
        "sync_default_tool_sandbox_mode = {}\ndocker_image = {}\ndocker_network = {}\ndocker_timeout_secs = {}\ndocker_user = {docker_user}\n",
        s.sync_default_tool_sandbox_mode.as_str(),
        s.sync_default_tool_sandbox_docker_image,
        s.sync_default_tool_sandbox_docker_network,
        s.sync_default_tool_sandbox_docker_timeout_secs,
    )
}

#[cfg(test)]
mod tests {
    use super::{section_body, self_config_info, SECTION_NAMES};

    #[test]
    fn default_config_lists_llm_and_redacts_secrets() {
        let cfg = crate::config::load_config(None).expect("embed default config");
        let out = self_config_info(&cfg, None);
        assert!(out.contains("[llm]"));
        assert!(out.contains("model ="));
        assert!(out.contains("api_base ="));
        assert!(out.contains("[web_search]"));
        assert!(
            out.contains("api_key = 未设置") || out.contains("api_key = 已设置(值隐藏)"),
            "api_key 只允许报状态，输出: {out}"
        );
    }

    #[test]
    fn sections_filter_limits_output() {
        let cfg = crate::config::load_config(None).expect("embed default config");
        let out = self_config_info(&cfg, Some(&["llm".to_string()]));
        assert!(out.contains("[llm]"));
        assert!(!out.contains("[sampling]"));
        assert!(!out.contains("[web_api]"));
    }

    #[test]
    fn unknown_section_yields_empty_output() {
        let cfg = crate::config::load_config(None).expect("embed default config");
        let out = self_config_info(&cfg, Some(&["no_such_section".to_string()]));
        assert!(out.is_empty());
    }

    #[test]
    fn every_section_name_has_an_implementation() {
        let cfg = crate::config::load_config(None).expect("embed default config");
        for name in SECTION_NAMES {
            assert!(
                section_body(&cfg, name).is_some(),
                "小节 {name} 缺少 section_body 实现"
            );
        }
    }

    #[test]
    fn tool_spec_description_mentions_every_section_name() {
        let specs = crate::cm_tools::tools::tool_specs_registry::tool_specs();
        let spec = specs
            .iter()
            .find(|s| s.name == "self_config_info")
            .expect("self_config_info 应已在工具注册表登记");
        for name in SECTION_NAMES {
            assert!(
                spec.description.contains(name),
                "工具 description 未提及小节 {name}"
            );
        }
    }
}
