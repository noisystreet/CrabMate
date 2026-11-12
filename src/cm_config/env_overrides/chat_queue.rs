//! `chat_queues_cache.*` 的 `CM_*` 覆盖（聊天队列并发 / 排队上限与只读缓存）。
//!
//! 会话级工作区变更集属 `session_workspace_changelist` 段，见 `session_changelist.rs`。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    queue_sizes(b);
    readonly_and_test_result_caches(b);
}

fn queue_sizes(b: &mut ConfigBuilder) {
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

fn readonly_and_test_result_caches(b: &mut ConfigBuilder) {
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
