//! `workspace_roots.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_csv_nonempty, apply_nonempty_opt};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_csv_nonempty(
        &mut b.workspace_roots.workspace_allowed_roots,
        "CM_WORKSPACE_ALLOWED_ROOTS",
    );
    apply_nonempty_opt(
        &mut b.workspace_roots.web_workspace_pool,
        "CM_WEB_WORKSPACE_POOL",
    );
}
