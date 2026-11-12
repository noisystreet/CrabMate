//! `agents_md.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_bool(
        &mut b.agents_md.agents_md_enabled,
        "CM_AGENTS_MD_ENABLED",
    );
    apply_parse(
        &mut b.agents_md.agents_md_max_chars,
        "CM_AGENTS_MD_MAX_CHARS",
    );
}
