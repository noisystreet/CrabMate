//! `apply_env_overrides_part_9`：Docker 用户余项、Web API、会话库、`agent_memory_file`。

use crate::config::builder::ConfigBuilder;
use crate::config::source::parse_bool_like;

pub(super) fn apply_env_overrides_part_9(b: &mut ConfigBuilder) {
    env_override_sync_default_docker_user_tail(b);
    env_override_web_api_security_fields(b);
    env_override_conversation_sqlite_path(b);
    env_override_agent_memory_file(b);
}

fn env_override_sync_default_docker_user_tail(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.sync_tool_sandbox.sync_default_tool_sandbox_docker_user = Some(v);
        }
    }
}

fn env_override_web_api_security_fields(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_WEB_API_BEARER_TOKEN") {
        b.web_api.web_api_bearer_token = Some(v.trim().to_string());
    }
    if let Ok(v) = std::env::var("CM_WEB_API_REQUIRE_BEARER")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_api_require_bearer = Some(val);
    }
    if let Ok(v) = std::env::var("CM_WEB_AUDIT_LOG_WRITE_TOOLS")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_audit_log_write_tools = Some(val);
    }
    if let Ok(v) = std::env::var("CM_WEB_AUDIT_TRUST_X_FORWARDED_FOR")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.web_audit_trust_x_forwarded_for = Some(val);
    }
    if let Ok(v) = std::env::var("CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK")
        && let Some(val) = parse_bool_like(&v)
    {
        b.web_api.allow_insecure_no_auth_for_non_loopback = Some(val);
    }
}

fn env_override_conversation_sqlite_path(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CONVERSATION_STORE_SQLITE_PATH") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.conversation_persistence.conversation_store_sqlite_path = Some(v);
        }
    }
}

fn env_override_agent_memory_file(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_MEMORY_FILE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.context_bootstrap_inject.agent_memory_file_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_MEMORY_FILE") {
        let v = v.trim().to_string();
        if !v.is_empty() {
            b.context_bootstrap_inject.agent_memory_file = Some(v);
        }
    }
    if let Ok(v) = std::env::var("CM_MEMORY_FILE_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.context_bootstrap_inject.agent_memory_file_max_chars = Some(n);
    }
}
