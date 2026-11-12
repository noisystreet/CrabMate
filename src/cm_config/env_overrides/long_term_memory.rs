//! `long_term_memory.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_nonempty_opt, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    core(b);
    indexing(b);
}

fn core(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.long_term_memory.long_term_memory_enabled,
        "CM_LONG_TERM_MEMORY_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.long_term_memory.long_term_memory_vector_backend_str,
        "CM_LONG_TERM_MEMORY_VECTOR_BACKEND",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_max_entries,
        "CM_LONG_TERM_MEMORY_MAX_ENTRIES",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_inject_max_chars,
        "CM_LONG_TERM_MEMORY_INJECT_MAX_CHARS",
    );
    apply_nonempty_opt(
        &mut b.long_term_memory.long_term_memory_store_sqlite_path,
        "CM_LONG_TERM_MEMORY_STORE_SQLITE_PATH",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_top_k,
        "CM_LONG_TERM_MEMORY_TOP_K",
    );
}

fn indexing(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.long_term_memory.long_term_memory_max_chars_per_chunk,
        "CM_LONG_TERM_MEMORY_MAX_CHARS_PER_CHUNK",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_min_chars_to_index,
        "CM_LONG_TERM_MEMORY_MIN_CHARS_TO_INDEX",
    );
    apply_bool(
        &mut b.long_term_memory.long_term_memory_async_index,
        "CM_LONG_TERM_MEMORY_ASYNC_INDEX",
    );
    apply_bool(
        &mut b.long_term_memory.long_term_memory_auto_index_turns,
        "CM_LONG_TERM_MEMORY_AUTO_INDEX_TURNS",
    );
    apply_bool(
        &mut b
            .long_term_memory
            .long_term_memory_auto_summarize_experience,
        "CM_LONG_TERM_MEMORY_AUTO_SUMMARIZE_EXPERIENCE",
    );
    apply_bool(
        &mut b
            .long_term_memory
            .long_term_memory_prioritize_experience_recall,
        "CM_LONG_TERM_MEMORY_PRIORITIZE_EXPERIENCE_RECALL",
    );
    apply_parse(
        &mut b.long_term_memory.long_term_memory_default_ttl_secs,
        "CM_LONG_TERM_MEMORY_DEFAULT_TTL_SECS",
    );
}
