//! `sync_tool_sandbox.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_nonempty_opt, apply_parse, apply_raw_opt};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_mode_str,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_MODE",
    );
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_image,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_IMAGE",
    );
    apply_raw_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_network,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_NETWORK",
    );
    apply_parse(
        &mut b
            .sync_tool_sandbox
            .sync_default_tool_sandbox_docker_timeout_secs,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_TIMEOUT_SECS",
    );
    // docker 用户余项（原独立分片 `env_overrides_part9`）并回本组：同属 `sync_tool_sandbox`。
    apply_nonempty_opt(
        &mut b.sync_tool_sandbox.sync_default_tool_sandbox_docker_user,
        "CM_SYNC_DEFAULT_TOOL_SANDBOX_DOCKER_USER",
    );
}
