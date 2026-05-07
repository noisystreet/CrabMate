//! 聊天队列、只读并行缓存、会话变更集等 `CM_*` 覆盖。

use crate::config::builder::ConfigBuilder;
use crate::config::source::parse_bool_like;

pub(super) fn env_override_chat_queue_parallel_and_caches(b: &mut ConfigBuilder) {
    chat_queue_override_sizes(b);
    parallel_readonly_and_test_result_caches(b);
    session_workspace_changelist_env(b);
}

fn chat_queue_override_sizes(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_CHAT_QUEUE_MAX_CONCURRENT")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.chat_queue_max_concurrent = Some(n);
    }
    if let Ok(v) = std::env::var("CM_CHAT_QUEUE_MAX_PENDING")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.chat_queue_max_pending = Some(n);
    }
    if let Ok(v) = std::env::var("CM_PARALLEL_READONLY_TOOLS_MAX")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.parallel_readonly_tools_max = Some(n);
    }
}

fn parallel_readonly_and_test_result_caches(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_READ_FILE_TURN_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.read_file_turn_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_READONLY_TOOL_TTL_CACHE_SECS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.readonly_tool_ttl_cache_secs = Some(n);
    }
    if let Ok(v) = std::env::var("CM_READONLY_TOOL_TTL_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.readonly_tool_ttl_cache_max_entries = Some(n);
    }
    if let Ok(v) = std::env::var("CM_TEST_RESULT_CACHE_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.chat_queues_cache.test_result_cache_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_TEST_RESULT_CACHE_MAX_ENTRIES")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.chat_queues_cache.test_result_cache_max_entries = Some(n);
    }
}

fn session_workspace_changelist_env(b: &mut ConfigBuilder) {
    if let Ok(v) = std::env::var("CM_SESSION_WORKSPACE_CHANGELIST_ENABLED")
        && let Some(val) = parse_bool_like(&v)
    {
        b.session_workspace_changelist
            .session_workspace_changelist_enabled = Some(val);
    }
    if let Ok(v) = std::env::var("CM_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS")
        && let Ok(n) = v.trim().parse::<u64>()
    {
        b.session_workspace_changelist
            .session_workspace_changelist_max_chars = Some(n);
    }
}
