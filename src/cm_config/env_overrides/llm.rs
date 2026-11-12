//! `llm.*` / `llm_sampling.*` / `llm_vendor.*` / `llm_http_retry.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_bool, apply_nonempty_opt, apply_nonempty_string, apply_parse,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    api_base_models_auth(b);
    sampling_numeric(b);
    vendor_bool_flags(b);
    http_retry(b);
}

fn api_base_models_auth(b: &mut ConfigBuilder) {
    apply_nonempty_string(&mut b.llm.api_base, "CM_API_BASE");
    apply_nonempty_string(&mut b.llm.model, "CM_MODEL");
    apply_nonempty_opt(&mut b.llm.planner_model, "CM_PLANNER_MODEL");
    apply_nonempty_opt(&mut b.llm.executor_model, "CM_EXECUTOR_MODEL");
    apply_nonempty_opt(&mut b.llm.llm_http_auth_mode_str, "CM_LLM_HTTP_AUTH_MODE");
}

fn sampling_numeric(b: &mut ConfigBuilder) {
    apply_parse(&mut b.llm_sampling.max_tokens, "CM_MAX_TOKENS");
    apply_parse(
        &mut b.llm_sampling.llm_context_tokens,
        "CM_LLM_CONTEXT_TOKENS",
    );
    apply_parse(&mut b.llm_sampling.temperature, "CM_TEMPERATURE");
    apply_parse(&mut b.llm_sampling.llm_seed, "CM_LLM_SEED");
}

fn vendor_bool_flags(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.llm_vendor.llm_reasoning_split,
        "CM_LLM_REASONING_SPLIT",
    );
    apply_bool(
        &mut b.llm_vendor.llm_bigmodel_thinking,
        "CM_LLM_BIGMODEL_THINKING",
    );
    apply_bool(
        &mut b.llm_vendor.llm_kimi_thinking_disabled,
        "CM_LLM_KIMI_THINKING_DISABLED",
    );
}

fn http_retry(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.llm_http_retry.api_timeout_secs,
        "CM_API_TIMEOUT_SECS",
    );
    apply_parse(&mut b.llm_http_retry.api_max_retries, "CM_API_MAX_RETRIES");
    apply_parse(
        &mut b.llm_http_retry.api_retry_delay_secs,
        "CM_API_RETRY_DELAY_SECS",
    );
}
