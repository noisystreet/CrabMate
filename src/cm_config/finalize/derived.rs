use super::*;

pub(super) fn derive_tool_registry_fields(b: &ConfigBuilder) -> ToolRegistryDerived {
    let tr = &b.tool_registry_policy;
    ToolRegistryDerived {
        tool_registry_http_fetch_wall_timeout_secs: tr
            .tool_registry_http_fetch_wall_timeout_secs
            .map(|s| s.clamp(1, 86_400)),
        tool_registry_http_request_wall_timeout_secs: tr
            .tool_registry_http_request_wall_timeout_secs
            .map(|s| s.clamp(1, 86_400)),
        tool_registry_parallel_wall_timeout_secs: Arc::new(
            tr.tool_registry_parallel_wall_timeout_secs
                .iter()
                .map(|(k, v)| (k.clone(), (*v).clamp(1, 86_400)))
                .collect::<HashMap<_, _>>(),
        ),
        tool_registry_parallel_sync_denied_tools: tr
            .tool_registry_parallel_sync_denied_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_parallel_sync_denied_prefixes: tr
            .tool_registry_parallel_sync_denied_prefixes
            .as_ref()
            .map(|v| {
                let cleaned: Vec<String> = v
                    .iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Arc::from(cleaned.into_boxed_slice())
            }),
        tool_registry_sync_default_inline_tools: tr
            .tool_registry_sync_default_inline_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_write_effect_tools: tr.tool_registry_write_effect_tools.as_ref().map(|v| {
            Arc::new(
                v.iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<HashSet<_>>(),
            )
        }),
        tool_registry_sub_agent_patch_write_extra_tools: tr
            .tool_registry_sub_agent_patch_write_extra_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_sub_agent_test_runner_extra_tools: tr
            .tool_registry_sub_agent_test_runner_extra_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        tool_registry_sub_agent_review_readonly_deny_tools: tr
            .tool_registry_sub_agent_review_readonly_deny_tools
            .as_ref()
            .map(|v| {
                Arc::new(
                    v.iter()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect::<HashSet<_>>(),
                )
            }),
        // 后台工具任务：默认值/范围见 `docs/design/background_tool_jobs_contract.md` §6；
        // validate.rs 先拒绝越界，此处 clamp 仅作兜底。
        // 总开关默认 `true`（模型仍须显式传 `async=true` 才走后台；白名单默认仅 `run_command`）。
        tool_registry_background_jobs_enabled: tr
            .tool_registry_background_jobs_enabled
            .unwrap_or(true),
        tool_registry_background_job_async_tools: Arc::new(
            tr.tool_registry_background_job_async_tools
                .clone()
                .unwrap_or_else(default_background_job_async_tools)
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>(),
        ),
        tool_registry_background_job_max_concurrent: tr
            .tool_registry_background_job_max_concurrent
            .unwrap_or(4)
            .clamp(1, 256),
        tool_registry_background_job_max_queued: tr
            .tool_registry_background_job_max_queued
            .unwrap_or(32)
            .clamp(0, 10_000),
        tool_registry_background_job_ttl_secs: tr
            .tool_registry_background_job_ttl_secs
            .unwrap_or(86_400)
            .clamp(1, 604_800),
        tool_registry_background_job_result_grace_secs: tr
            .tool_registry_background_job_result_grace_secs
            .unwrap_or(300)
            .clamp(0, 86_400),
        tool_registry_background_job_max_entries: tr
            .tool_registry_background_job_max_entries
            .unwrap_or(128)
            .clamp(1, 10_000),
        tool_registry_background_job_output_buffer_bytes: tr
            .tool_registry_background_job_output_buffer_bytes
            .unwrap_or(262_144)
            .clamp(4096, 16_777_216),
        // 工具失败透明重试（默认关闭；默认码集为瞬时可重试类，validate 拒绝越界，此处仅兜底）。
        tool_registry_tool_retry_enabled: tr
            .tool_registry_tool_retry_enabled
            .unwrap_or(false),
        tool_registry_tool_retry_max_attempts: tr
            .tool_registry_tool_retry_max_attempts
            .unwrap_or(2)
            .clamp(1, 5),
        tool_registry_tool_retry_backoff_ms: tr
            .tool_registry_tool_retry_backoff_ms
            .unwrap_or(250)
            .clamp(0, 10_000),
        tool_registry_tool_retry_error_codes: Arc::new(
            tr.tool_registry_tool_retry_error_codes
                .clone()
                .unwrap_or_else(default_tool_retry_error_codes)
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>(),
        ),
        tool_registry_tool_retry_denied_tools: Arc::new(
            tr.tool_registry_tool_retry_denied_tools
                .clone()
                .unwrap_or_default()
                .into_iter()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<HashSet<_>>(),
        ),
        // 工具启停：名单/前缀去空白、剔除空串；`None` 保持 `None`（= 不按名单禁用）。
        tool_registry_disabled_tools: tr
            .tool_registry_disabled_tools
            .as_ref()
            .map(|v| Arc::new(clean_str_list(v).into_iter().collect::<HashSet<_>>())),
        tool_registry_disabled_tool_prefixes: tr
            .tool_registry_disabled_tool_prefixes
            .as_ref()
            .map(|v| Arc::from(clean_str_list(v).into_boxed_slice())),
    }
}

/// 去空白、剔除空串（工具启停名单/前缀类字段共用）。
pub(super) fn clean_str_list(v: &[String]) -> Vec<String> {
    v.iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// 允许 `async=true` 的内建工具默认白名单（用户可用 `background_job_async_tools` 覆盖；空数组 = 全部禁用）。
pub(super) fn default_background_job_async_tools() -> Vec<String> {
    vec![tool_names::RUN_COMMAND.to_string()]
}

/// 工具失败透明重试的默认可重试错误码（瞬时可重试类；用户可用 `tool_retry_error_codes` 覆盖）。
pub(super) fn default_tool_retry_error_codes() -> Vec<String> {
    [
        "timeout",
        "http_timeout",
        "rate_limited",
        "http_network_error",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

pub(super) fn derive_intent_fields(b: &ConfigBuilder) -> Result<IntentDerived, String> {
    let llm_http_auth_mode = match b.llm.llm_http_auth_mode_str.as_deref() {
        Some(s) => types::LlmHttpAuthMode::parse(s)?,
        None => types::LlmHttpAuthMode::default(),
    };
    let llm_reasoning_split = b.llm_vendor.llm_reasoning_split.unwrap_or_else(|| {
        crate::cm_config::gateway_hints::default_llm_reasoning_split_for_gateway(&b.llm.model, &b.llm.api_base)
    });
    Ok(IntentDerived {
        llm_http_auth_mode,
        llm_reasoning_split,
    })
}

pub(super) fn derive_ltm(b: &ConfigBuilder) -> Result<LtmDerived, String> {
    let long_term_memory_enabled = b.long_term_memory.long_term_memory_enabled.unwrap_or(true);
    let long_term_memory_vector_backend = match b
        .long_term_memory
        .long_term_memory_vector_backend_str
        .as_deref()
    {
        Some(s) => LongTermMemoryVectorBackend::parse(s)?,
        None => LongTermMemoryVectorBackend::default(),
    };
    #[cfg(not(feature = "fastembed"))]
    let long_term_memory_vector_backend = if long_term_memory_enabled
        && long_term_memory_vector_backend == LongTermMemoryVectorBackend::Fastembed
    {
        LongTermMemoryVectorBackend::Disabled
    } else {
        long_term_memory_vector_backend
    };
    super::checks::validate_ltm_backend_when_enabled(long_term_memory_enabled, long_term_memory_vector_backend)?;
    Ok(LtmDerived {
        long_term_memory_enabled,
        long_term_memory_vector_backend,
        long_term_memory_max_entries: b
            .long_term_memory
            .long_term_memory_max_entries
            .unwrap_or(256)
            .clamp(1, 100_000) as usize,
        long_term_memory_inject_max_chars: b
            .long_term_memory
            .long_term_memory_inject_max_chars
            .unwrap_or(8000)
            .clamp(256, 500_000) as usize,
        long_term_memory_store_sqlite_path: b
            .long_term_memory
            .long_term_memory_store_sqlite_path
            .clone()
            .unwrap_or_default(),
        long_term_memory_top_k: b
            .long_term_memory
            .long_term_memory_top_k
            .unwrap_or(8)
            .clamp(1, 64) as usize,
        long_term_memory_max_chars_per_chunk: b
            .long_term_memory
            .long_term_memory_max_chars_per_chunk
            .unwrap_or(1024)
            .clamp(256, 32_000) as usize,
        long_term_memory_min_chars_to_index: b
            .long_term_memory
            .long_term_memory_min_chars_to_index
            .unwrap_or(8)
            .clamp(0, 4096) as usize,
        long_term_memory_async_index: b
            .long_term_memory
            .long_term_memory_async_index
            .unwrap_or(true),
        long_term_memory_auto_index_turns: b
            .long_term_memory
            .long_term_memory_auto_index_turns
            .unwrap_or(true),
        long_term_memory_auto_summarize_experience: b
            .long_term_memory
            .long_term_memory_auto_summarize_experience
            .unwrap_or(true),
        long_term_memory_prioritize_experience_recall: b
            .long_term_memory
            .long_term_memory_prioritize_experience_recall
            .unwrap_or(true),
        long_term_memory_default_ttl_secs: b
            .long_term_memory
            .long_term_memory_default_ttl_secs
            .unwrap_or(0)
            .clamp(0, 365 * 86400 * 10),
    })
}

pub(super) fn derive_codebase_semantic(b: &ConfigBuilder) -> CodebaseSemanticDerived {
    #[cfg(feature = "fastembed")]
    let codebase_semantic_search_enabled = b
        .codebase_semantic
        .codebase_semantic_search_enabled
        .unwrap_or(true);
    #[cfg(not(feature = "fastembed"))]
    let codebase_semantic_search_enabled = false;
    let mut codebase_semantic_hybrid_alpha = b
        .codebase_semantic
        .codebase_semantic_hybrid_alpha
        .unwrap_or(0.55_f64) as f32;
    if !codebase_semantic_hybrid_alpha.is_finite() {
        codebase_semantic_hybrid_alpha = 0.55;
    }
    codebase_semantic_hybrid_alpha = codebase_semantic_hybrid_alpha.clamp(0.0, 1.0);
    CodebaseSemanticDerived {
        codebase_semantic_search_enabled,
        codebase_semantic_invalidate_on_workspace_change: b
            .codebase_semantic
            .codebase_semantic_invalidate_on_workspace_change
            .unwrap_or(true),
        codebase_semantic_index_sqlite_path: b
            .codebase_semantic
            .codebase_semantic_index_sqlite_path
            .clone()
            .unwrap_or_default(),
        codebase_semantic_max_file_bytes: b
            .codebase_semantic
            .codebase_semantic_max_file_bytes
            .unwrap_or(512 * 1024)
            .clamp(4096, 4 * 1024 * 1024) as usize,
        codebase_semantic_chunk_max_chars: b
            .codebase_semantic
            .codebase_semantic_chunk_max_chars
            .unwrap_or(1200)
            .clamp(256, 16_000) as usize,
        codebase_semantic_top_k: b
            .codebase_semantic
            .codebase_semantic_top_k
            .unwrap_or(8)
            .clamp(1, 64) as usize,
        codebase_semantic_query_max_chunks: b
            .codebase_semantic
            .codebase_semantic_query_max_chunks
            .unwrap_or(50_000)
            .clamp(0, 2_000_000) as usize,
        codebase_semantic_rebuild_max_files: b
            .codebase_semantic
            .codebase_semantic_rebuild_max_files
            .unwrap_or(2000)
            .clamp(1, 100_000) as usize,
        codebase_semantic_rebuild_incremental: b
            .codebase_semantic
            .codebase_semantic_rebuild_incremental
            .unwrap_or(true),
        codebase_semantic_hybrid_alpha,
        codebase_semantic_fts_top_n: b
            .codebase_semantic
            .codebase_semantic_fts_top_n
            .unwrap_or(400)
            .clamp(1, 10_000) as usize,
        codebase_semantic_hybrid_semantic_pool: b
            .codebase_semantic
            .codebase_semantic_hybrid_semantic_pool
            .unwrap_or(256)
            .clamp(1, 10_000) as usize,
    }
}
