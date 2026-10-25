//! `CM_PERMISSION_*` 环境变量覆盖权限等级相关键（优先级高于磁盘 TOML）。
//!
//! 键与生效顺序见 `docs/配置说明.md`「权限等级」，设计见 `docs/design/permission_levels.md` §10。

use crate::cm_config::builder::ConfigBuilder;
use crate::cm_config::env_override_apply::{apply_bool, apply_csv_allow_empty, apply_nonempty_opt};

/// 等级串 / 上限串 / 三个列表 / 非 loopback 放宽开关。
///
/// 列表键用 `apply_csv_allow_empty`：空串语义是**显式清空**（如某形态下不要 L0 控制面工具），
/// 与 `permission_read_tools` 的「显式空数组 = 配置错误」由 `finalize` 的校验兜住。
pub(super) fn apply_env_permission_overrides(b: &mut ConfigBuilder) {
    let perm = &mut b.permission;
    apply_nonempty_opt(&mut perm.permission_level_str, "CM_PERMISSION_LEVEL");
    apply_nonempty_opt(
        &mut perm.permission_level_max_str,
        "CM_PERMISSION_LEVEL_MAX",
    );
    apply_csv_allow_empty(
        &mut perm.permission_chat_control_tools,
        "CM_PERMISSION_CHAT_CONTROL_TOOLS",
    );
    apply_csv_allow_empty(
        &mut perm.permission_read_tools,
        "CM_PERMISSION_READ_TOOLS",
    );
    apply_csv_allow_empty(
        &mut perm.permission_deny_system_paths,
        "CM_PERMISSION_DENY_SYSTEM_PATHS",
    );
    apply_csv_allow_empty(
        &mut perm.permission_confirm_tools,
        "CM_PERMISSION_CONFIRM_TOOLS",
    );
    apply_bool(
        &mut perm.allow_unrestricted_permission_for_non_loopback,
        "CM_ALLOW_UNRESTRICTED_PERMISSION_FOR_NON_LOOPBACK",
    );
}
