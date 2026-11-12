//! `context_pipeline.*` 与 `context_bootstrap_inject.*` 的 `CM_*` 覆盖。
//!
//! 两段都属上下文管线：前者是裁剪/摘要的预算与阈值，后者是启动期注入的内容源。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_nonempty_opt, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    pipeline_budget_and_summary(b);
    pipeline_transcript_files(b);
    bootstrap_memory_file(b);
    bootstrap_living_docs_and_project_profile(b);
    bootstrap_dependency_brief(b);
}

fn pipeline_budget_and_summary(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.context_pipeline.context_char_budget,
        "CM_CONTEXT_CHAR_BUDGET",
    );
    apply_parse(
        &mut b.context_pipeline.context_min_messages_after_system,
        "CM_CONTEXT_MIN_MESSAGES_AFTER_SYSTEM",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_trigger_percent,
        "CM_CONTEXT_TOKEN_TRIGGER_PERCENT",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_target_percent,
        "CM_CONTEXT_TOKEN_TARGET_PERCENT",
    );
    apply_parse(
        &mut b.context_pipeline.context_token_safety_margin_tokens,
        "CM_CONTEXT_TOKEN_SAFETY_MARGIN_TOKENS",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_trigger_chars,
        "CM_CONTEXT_SUMMARY_TRIGGER_CHARS",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_tail_messages,
        "CM_CONTEXT_SUMMARY_TAIL_MESSAGES",
    );
    apply_parse(
        &mut b.context_pipeline.context_summary_max_tokens,
        "CM_CONTEXT_SUMMARY_MAX_TOKENS",
    );
    apply_bool(
        &mut b.context_pipeline.context_btw_summary_enabled,
        "CM_CONTEXT_BTW_SUMMARY_ENABLED",
    );
}

fn pipeline_transcript_files(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.context_pipeline.context_summary_transcript_max_chars,
        "CM_CONTEXT_SUMMARY_TRANSCRIPT_MAX_CHARS",
    );
    apply_nonempty_opt(
        &mut b.context_pipeline.context_summary_system_file,
        "CM_CONTEXT_SUMMARY_SYSTEM_FILE",
    );
    apply_nonempty_opt(
        &mut b.context_pipeline.context_summary_user_file,
        "CM_CONTEXT_SUMMARY_USER_FILE",
    );
}

fn bootstrap_memory_file(b: &mut ConfigBuilder) {
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

fn bootstrap_living_docs_and_project_profile(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.context_bootstrap_inject.living_docs_inject_enabled,
        "CM_LIVING_DOCS_INJECT_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.context_bootstrap_inject.living_docs_relative_dir,
        "CM_LIVING_DOCS_RELATIVE_DIR",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.living_docs_inject_max_chars,
        "CM_LIVING_DOCS_INJECT_MAX_CHARS",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.living_docs_file_max_each_chars,
        "CM_LIVING_DOCS_FILE_MAX_EACH_CHARS",
    );
    apply_bool(
        &mut b.context_bootstrap_inject.project_profile_inject_enabled,
        "CM_PROJECT_PROFILE_INJECT_ENABLED",
    );
    apply_parse(
        &mut b.context_bootstrap_inject.project_profile_inject_max_chars,
        "CM_PROJECT_PROFILE_INJECT_MAX_CHARS",
    );
}

fn bootstrap_dependency_brief(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b
            .context_bootstrap_inject
            .project_dependency_brief_inject_enabled,
        "CM_PROJECT_DEPENDENCY_BRIEF_INJECT_ENABLED",
    );
    apply_parse(
        &mut b
            .context_bootstrap_inject
            .project_dependency_brief_inject_max_chars,
        "CM_PROJECT_DEPENDENCY_BRIEF_INJECT_MAX_CHARS",
    );
}
