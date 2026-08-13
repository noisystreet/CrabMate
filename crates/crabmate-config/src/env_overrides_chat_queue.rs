//! 聊天队列、只读并行缓存、会话变更集等 `CM_*` 覆盖。

use crate::builder::ConfigBuilder;
use crate::env_override_apply::{apply_bool, apply_parse};

pub(super) fn env_override_chat_queue_parallel_and_caches(b: &mut ConfigBuilder) {
    chat_queue_override_sizes(b);
    parallel_readonly_and_test_result_caches(b);
    session_workspace_changelist_env(b);
}

fn chat_queue_override_sizes(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.chat_queues_cache.chat_queue_max_concurrent,
        "CM_CHAT_QUEUE_MAX_CONCURRENT",
    );
    apply_parse(
        &mut b.chat_queues_cache.chat_queue_max_pending,
        "CM_CHAT_QUEUE_MAX_PENDING",
    );
    apply_parse(
        &mut b.chat_queues_cache.parallel_readonly_tools_max,
        "CM_PARALLEL_READONLY_TOOLS_MAX",
    );
}

fn parallel_readonly_and_test_result_caches(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.chat_queues_cache.read_file_turn_cache_max_entries,
        "CM_READ_FILE_TURN_CACHE_MAX_ENTRIES",
    );
    apply_parse(
        &mut b.chat_queues_cache.readonly_tool_ttl_cache_secs,
        "CM_READONLY_TOOL_TTL_CACHE_SECS",
    );
    apply_parse(
        &mut b.chat_queues_cache.readonly_tool_ttl_cache_max_entries,
        "CM_READONLY_TOOL_TTL_CACHE_MAX_ENTRIES",
    );
    apply_bool(
        &mut b.chat_queues_cache.test_result_cache_enabled,
        "CM_TEST_RESULT_CACHE_ENABLED",
    );
    apply_parse(
        &mut b.chat_queues_cache.test_result_cache_max_entries,
        "CM_TEST_RESULT_CACHE_MAX_ENTRIES",
    );
}

fn session_workspace_changelist_env(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b
            .session_workspace_changelist
            .session_workspace_changelist_enabled,
        "CM_SESSION_WORKSPACE_CHANGELIST_ENABLED",
    );
    apply_parse(
        &mut b
            .session_workspace_changelist
            .session_workspace_changelist_max_chars,
        "CM_SESSION_WORKSPACE_CHANGELIST_MAX_CHARS",
    );
}
