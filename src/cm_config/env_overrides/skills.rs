//! `skills.*` 的 `CM_*` 覆盖。
//!
//! `CM_SKILLS_DISABLE_HOST_LAYERS` 必须排在两个单独目录变量之前：它把用户层 / 系统层
//! 置为空串（一次关掉跨工作区可见的技能），随后 `CM_SKILLS_USER_DIR` /
//! `CM_SKILLS_SYSTEM_DIR` 仍可显式覆盖回来。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{
    apply_bool, apply_nonempty_opt, apply_parse, apply_trimmed_opt, env_flag_true,
};

pub(super) fn apply(b: &mut ConfigBuilder) {
    apply_bool(&mut b.skills.skills_enabled, "CM_SKILLS_ENABLED");
    apply_nonempty_opt(&mut b.skills.skills_dir, "CM_SKILLS_DIR");
    // `CM_SKILLS_DISABLE_HOST_LAYERS=1`：测试/CI 一次关掉用户+系统层（仍可用下面两个变量单独覆盖）。
    if env_flag_true("CM_SKILLS_DISABLE_HOST_LAYERS") {
        b.skills.skills_user_dir = Some(String::new());
        b.skills.skills_system_dir = Some(String::new());
    }
    apply_trimmed_opt(&mut b.skills.skills_user_dir, "CM_SKILLS_USER_DIR");
    apply_trimmed_opt(&mut b.skills.skills_system_dir, "CM_SKILLS_SYSTEM_DIR");
    apply_parse(&mut b.skills.skills_max_chars, "CM_SKILLS_MAX_CHARS");
}
