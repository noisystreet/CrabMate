//! `apply_env_overrides_part_9`：Docker 用户余项、Web API、会话库、`agent_memory_file`。

use crate::builder::ConfigBuilder;
use crate::env_override_apply::{
    apply_bool, apply_csv_allow_empty, apply_nonempty_opt, apply_parse, apply_trimmed_opt,
};

pub(super) fn apply_env_overrides_part_9(b: &mut ConfigBuilder) {
    env_override_sync_default_docker_user_tail(b);
    env_override_web_api_security_fields(b);
    env_override_conversation_sqlite_path(b);
    env_override_agent_memory_file(b);
}

fn env_override_sync_default_docker_user_tail(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_user,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER",
    );
}

fn env_override_web_api_security_fields(b: &mut ConfigBuilder) {
    apply_trimmed_opt(
        &mut b.web_api.web_api_bearer_token,
        "CM_WEB_API_BEARER_TOKEN",
    );
    apply_bool(
        &mut b.web_api.web_api_require_bearer,
        "CM_WEB_API_REQUIRE_BEARER",
    );
    // 显式空串（或仅空白/逗号）→ Some([])，finalize 时关闭 CORS（含默认壳 Origin）。
    // 非空 → 与默认壳 Origin 合并（见 `resolve_web_cors_allowed_origins`）。
    apply_csv_allow_empty(
        &mut b.web_api.web_cors_allowed_origins,
        "CM_WEB_CORS_ALLOWED_ORIGINS",
    );
    apply_bool(
        &mut b.web_api.web_audit_log_write_tools,
        "CM_WEB_AUDIT_LOG_WRITE_TOOLS",
    );
    apply_bool(
        &mut b.web_api.web_audit_trust_x_forwarded_for,
        "CM_WEB_AUDIT_TRUST_X_FORWARDED_FOR",
    );
    apply_bool(
        &mut b.web_api.allow_insecure_no_auth_for_non_loopback,
        "CM_ALLOW_INSECURE_NO_AUTH_FOR_NON_LOOPBACK",
    );
}

fn env_override_conversation_sqlite_path(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.conversation_persistence.conversation_store_sqlite_path,
        "CM_CONVERSATION_STORE_SQLITE_PATH",
    );
}

fn env_override_agent_memory_file(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.context_bootstrap_inject.agent_memory_file_enabled,
        "CM_MEMORY_FILE_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.context_bootstrap_inject.agent_memory_file,
        "CM_MEMORY_FILE",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.agent_memory_file_max_chars,
        "CM_MEMORY_FILE_MAX_CHARS",
    );
}
