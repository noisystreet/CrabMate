//! `command_exec.*` 的 `CM_*` 覆盖。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_csv_nonempty, apply_nonempty_opt, apply_parse};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_parse(
        &mut b.command_exec.command_timeout_secs,
        "CM_COMMAND_TIMEOUT_SECS",
    );
    apply_parse(
        &mut b.command_exec.command_max_output_len,
        "CM_COMMAND_MAX_OUTPUT_LEN",
    );
    apply_csv_nonempty(&mut b.command_exec.allowed_commands, "CM_ALLOWED_COMMANDS");
    apply_nonempty_opt(
        &mut b.command_exec.run_command_working_dir,
        "CM_RUN_COMMAND_WORKING_DIR",
    );
}
