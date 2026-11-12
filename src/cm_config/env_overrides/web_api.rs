//! `web_api.*` 与 `conversation_persistence.*` 的 `CM_*` 覆盖。
//!
//! 两者都是 Web 侧的对外面：前者是认证 / CORS / 健康探测，后者是会话与上下文的持久化。
//!
//! 两组字段彼此不共享结构，原先同处一个「第 9 块」分片只是因为行数上限（见 #1158）；
//! 这里按既有配置段分函数——`web_api`（认证 / CORS / 审计 / 健康探测）与
//! `conversation_persistence`（会话库与上下文归档）。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_bool, apply_csv_allow_empty, apply_nonempty_opt, apply_parse, apply_trimmed_opt,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    security_fields(b);
    health_probe(b);
    conversation_store(b);
}

fn security_fields(b: &mut ConfigBuilder) {
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

fn health_probe(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.web_api.health_llm_models_probe,
        "CM_HEALTH_LLM_MODELS_PROBE",
    );
    apply_parse(
        &mut b.web_api.health_llm_models_probe_cache_secs,
        "CM_HEALTH_LLM_MODELS_PROBE_CACHE_SECS",
    );
}

fn conversation_store(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.conversation_persistence.conversation_store_sqlite_path,
        "CM_CONVERSATION_STORE_SQLITE_PATH",
    );
    apply_parse(
        &mut b.conversation_persistence.conversation_store_ttl_secs,
        "CM_CONVERSATION_STORE_TTL_SECS",
    );
    apply_parse(
        &mut b.conversation_persistence.conversation_store_max_entries,
        "CM_CONVERSATION_STORE_MAX_ENTRIES",
    );
    apply_parse(
        &mut b
            .conversation_persistence
            .context_archive_max_rows_per_conversation,
        "CM_CONTEXT_ARCHIVE_MAX_ROWS_PER_CONVERSATION",
    );
    apply_parse(
        &mut b.conversation_persistence.context_archive_codec_min_bytes,
        "CM_CONTEXT_ARCHIVE_CODEC_MIN_BYTES",
    );
}
