//! `roles_prompts.*` 的 `CM_*` 覆盖（系统提示、默认角色 id、编码工作台、默认会话模式）。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_bool, apply_nonempty_opt, apply_nonempty_string_clearing_opt,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    // `CM_SYSTEM_PROMPT` 必须先于 `CM_SYSTEM_PROMPT_FILE`：
    // 前者会清空 `system_prompt_file`，后者是「后处理覆盖」，同时设置时文件优先于内联。
    apply_nonempty_string_clearing_opt(
        &mut b.roles_prompts.system_prompt,
        &mut b.roles_prompts.system_prompt_file,
        "CM_SYSTEM_PROMPT",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.system_prompt_file,
        "CM_SYSTEM_PROMPT_FILE",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.default_agent_role_id,
        "CM_DEFAULT_CM_ROLE",
    );
    apply_bool(
        &mut b.roles_prompts.coding_workbench_enabled,
        "CM_CODING_WORKBENCH_ENABLED",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.coding_workbench_increment_file,
        "CM_CODING_WORKBENCH_INCREMENT_FILE",
    );
    apply_nonempty_opt(
        &mut b.roles_prompts.default_session_mode,
        "CM_DEFAULT_SESSION_MODE",
    );
}
