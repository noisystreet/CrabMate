//! `session_workspace_changelist.*` 的 `CM_*` 覆盖（会话级工作区变更集）。
//!
//! 从 `chat_queue.rs` 拆出：它不是聊天队列，与并发/缓存限额无关，只是先前与
//! `chat_queues_cache` 同处一个分片。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
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
