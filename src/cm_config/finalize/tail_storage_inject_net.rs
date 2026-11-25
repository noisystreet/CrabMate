//! `finalize` 尾段里「会话存储 / 文档注入 / 出网」标量的解析与 clamp。
//!
//! 从 `tail.rs` 拆出：`tail.rs` 在按 2024 样式排版后越过单文件行数上限，而本组
//! `struct` + `derive_*` 自成一体、与尾段其余部分无共享状态。

use super::*;

#[allow(clippy::struct_excessive_bools)]
pub(super) struct TailStorageInjectNetScalars {
    pub(super) conversation_store_sqlite_path: String,
    pub(super) conversation_store_ttl_secs: u64,
    pub(super) conversation_store_max_entries: usize,
    pub(super) context_archive_max_rows_per_conversation: usize,
    pub(super) context_archive_codec_min_bytes: usize,
    pub(super) agent_memory_file_enabled: bool,
    pub(super) agent_memory_file: String,
    pub(super) agent_memory_file_max_chars: usize,
    pub(super) living_docs_inject_enabled: bool,
    pub(super) living_docs_relative_dir: String,
    pub(super) living_docs_inject_max_chars: usize,
    pub(super) living_docs_file_max_each_chars: usize,
    pub(super) project_profile_inject_enabled: bool,
    pub(super) project_profile_inject_max_chars: usize,
    pub(super) project_dependency_brief_inject_enabled: bool,
    pub(super) project_dependency_brief_inject_max_chars: usize,
    pub(super) tool_call_explain_enabled: bool,
    pub(super) tool_call_explain_min_chars: usize,
    pub(super) tool_call_explain_max_chars: usize,
    pub(super) mcp_enabled: bool,
    pub(super) mcp_tool_timeout_secs: u64,
    pub(super) web_search_provider: WebSearchProvider,
    pub(super) web_search_api_key: types::SecretString,
    pub(super) web_search_timeout_secs: u64,
    pub(super) web_search_max_results: u32,
    /// worbrow 浏览器/静态抓取代理（空 = 直连/系统代理）。
    pub(super) web_search_proxy: String,
    pub(super) http_fetch_allowed_prefixes: Vec<String>,
    pub(super) http_fetch_timeout_secs: u64,
    pub(super) http_fetch_max_response_bytes: usize,
    pub(super) http_fetch_user_agent: String,
}

pub(super) fn derive_tail_storage_inject_net_scalars(
    b: &ConfigBuilder,
    command_timeout_secs: u64,
) -> Result<TailStorageInjectNetScalars, String> {
    let conversation_store_sqlite_path = b
        .conversation_persistence
        .conversation_store_sqlite_path
        .clone()
        .unwrap_or_default();
    let conversation_store_ttl_secs = b
        .conversation_persistence
        .conversation_store_ttl_secs
        .unwrap_or(86400);
    let conversation_store_max_entries = b
        .conversation_persistence
        .conversation_store_max_entries
        .unwrap_or(512) as usize;
    let context_archive_max_rows_per_conversation = b
        .conversation_persistence
        .context_archive_max_rows_per_conversation
        .unwrap_or(crate::conversation_store::ARCHIVE_MAX_ROWS_PER_CONVERSATION_DEFAULT as u64)
        as usize;
    let context_archive_codec_min_bytes = b
        .conversation_persistence
        .context_archive_codec_min_bytes
        .unwrap_or(crate::conversation_store::ARCHIVE_CODEC_MIN_BYTES_DEFAULT as u64)
        .clamp(0, 4_194_304) as usize;
    let agent_memory_file_enabled = b
        .context_bootstrap_inject
        .agent_memory_file_enabled
        .unwrap_or(false);
    let agent_memory_file = b
        .context_bootstrap_inject
        .agent_memory_file
        .clone()
        .unwrap_or_else(|| ".crabmate/agent_memory.md".to_string());
    let agent_memory_file_max_chars = b
        .context_bootstrap_inject
        .agent_memory_file_max_chars
        .unwrap_or(8000)
        .clamp(256, 500_000) as usize;
    let living_docs_inject_enabled = b
        .context_bootstrap_inject
        .living_docs_inject_enabled
        .unwrap_or(false);
    let living_docs_relative_dir = b
        .context_bootstrap_inject
        .living_docs_relative_dir
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| ".crabmate/living_docs".to_string());
    let living_docs_inject_max_chars = b
        .context_bootstrap_inject
        .living_docs_inject_max_chars
        .unwrap_or(4000)
        .clamp(0, 500_000) as usize;
    let living_docs_file_max_each_chars = b
        .context_bootstrap_inject
        .living_docs_file_max_each_chars
        .unwrap_or(1200)
        .clamp(0, 500_000) as usize;
    let project_profile_inject_enabled = b
        .context_bootstrap_inject
        .project_profile_inject_enabled
        .unwrap_or(true);
    let project_profile_inject_max_chars = b
        .context_bootstrap_inject
        .project_profile_inject_max_chars
        .unwrap_or(6000)
        .clamp(0, 500_000) as usize;
    let project_dependency_brief_inject_enabled = b
        .context_bootstrap_inject
        .project_dependency_brief_inject_enabled
        .unwrap_or(true);
    let project_dependency_brief_inject_max_chars = b
        .context_bootstrap_inject
        .project_dependency_brief_inject_max_chars
        .unwrap_or(4000)
        .clamp(0, 500_000) as usize;
    let tool_call_explain_enabled = b
        .tool_call_explain
        .tool_call_explain_enabled
        .unwrap_or(false);
    let tool_call_explain_min_chars = b
        .tool_call_explain
        .tool_call_explain_min_chars
        .unwrap_or(8)
        .clamp(1, 256) as usize;
    let max_chars_raw = b
        .tool_call_explain
        .tool_call_explain_max_chars
        .unwrap_or(400)
        .clamp(1, 4000) as usize;
    let tool_call_explain_max_chars = max_chars_raw.max(tool_call_explain_min_chars);

    let mcp_enabled = b.mcp_client.mcp_enabled.unwrap_or(false);
    let mcp_tool_timeout_secs = b
        .mcp_client
        .mcp_tool_timeout_secs
        .unwrap_or(command_timeout_secs)
        .max(1);

    let web_search_provider = match b.web_search.web_search_provider_str.as_deref() {
        Some(s) => WebSearchProvider::parse(s)?,
        None => WebSearchProvider::default(),
    };
    let web_search_api_key = types::SecretString::new(
        b.web_search
            .web_search_api_key
            .clone()
            .unwrap_or_default()
            .into(),
    );
    // 默认 worbrow（本机浏览器）通常需更长墙钟；Brave/Tavily 可自行调低
    let web_search_timeout_secs = b.web_search.web_search_timeout_secs.unwrap_or(60).max(1);
    let web_search_max_results = b
        .web_search
        .web_search_max_results
        .unwrap_or(8)
        .clamp(1, 20) as u32;
    let web_search_proxy = b
        .web_search
        .web_search_proxy
        .clone()
        .unwrap_or_default()
        .trim()
        .to_string();

    let http_fetch_allowed_prefixes = b
        .http_fetch
        .http_fetch_allowed_prefixes
        .clone()
        .unwrap_or_default();
    let http_fetch_timeout_secs = b.http_fetch.http_fetch_timeout_secs.unwrap_or(30).max(1);
    let http_fetch_max_response_bytes = b
        .http_fetch
        .http_fetch_max_response_bytes
        .unwrap_or(524_288)
        .clamp(1024, 4_194_304) as usize;
    let http_fetch_user_agent = b
        .http_fetch
        .http_fetch_user_agent
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| format!("crabmate/{}", env!("CARGO_PKG_VERSION")));

    Ok(TailStorageInjectNetScalars {
        conversation_store_sqlite_path,
        conversation_store_ttl_secs,
        conversation_store_max_entries,
        context_archive_max_rows_per_conversation,
        context_archive_codec_min_bytes,
        agent_memory_file_enabled,
        agent_memory_file,
        agent_memory_file_max_chars,
        living_docs_inject_enabled,
        living_docs_relative_dir,
        living_docs_inject_max_chars,
        living_docs_file_max_each_chars,
        project_profile_inject_enabled,
        project_profile_inject_max_chars,
        project_dependency_brief_inject_enabled,
        project_dependency_brief_inject_max_chars,
        tool_call_explain_enabled,
        tool_call_explain_min_chars,
        tool_call_explain_max_chars,
        mcp_enabled,
        mcp_tool_timeout_secs,
        web_search_provider,
        web_search_api_key,
        web_search_timeout_secs,
        web_search_max_results,
        web_search_proxy,
        http_fetch_allowed_prefixes,
        http_fetch_timeout_secs,
        http_fetch_max_response_bytes,
        http_fetch_user_agent,
    })
}
