//! `GET /permission` 的响应契约（设计 `docs/design/permission_levels.md` §16.2）。
//!
//! 只读**策略投影**：回答「当前落在哪一档、为什么、还能选到哪一档」。
//! 不含回合内 `AllowAlways` 已批准的键，也不含 `run_command` 的 `skip_arg_safety` 等派生项
//! （那些只存在于 `PolicyDecision` 内部，进 HTTP 响应会让投影随回合漂移）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cm_types::PermissionCapsView;

/// [`PermissionView::level`] 的取值来源（设计 §16.2 的 `level_source`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevelSource {
    /// 无会话级意图：回落配置默认 `permission_level`（已按生效上限钳制）。
    Config,
    /// 会话持久化的意图（`?conversation_id=` 带来的 `active_permission_level`）。
    Persisted,
}

impl PermissionLevelSource {
    /// 线格式串（`snake_case`，与 `serde` 一致）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionLevelSource::Config => "config",
            PermissionLevelSource::Persisted => "persisted",
        }
    }
}

/// `GET /permission` 响应体。
///
/// 客户端据此渲染等级选择器：选项集直接取 [`Self::available`]，
/// 「为什么不能选 L4」由 [`Self::clamped_by`] + [`Self::ceiling_source`] 回答。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PermissionView {
    /// 当前生效等级（min 链之后；与工具拒绝文案里的等级是同一个值）。
    pub level: String,
    /// `level` 的来源：`config` / `persisted`。
    pub level_source: PermissionLevelSource,
    /// 服务端生效上限（启动期冻结；热重载只降不升）。
    pub ceiling: String,
    /// 上限来源：`config` / `derived_loopback` / `derived_non_loopback`。
    pub ceiling_source: String,
    /// 选择器选项集：全部 `≤` 上限，并已扣除角色上限与会话模式钳制；自低到高。
    pub available: Vec<String>,
    /// 实际收窄的轴，顺序固定 `ceiling` → `role` → `session_mode`；空数组 = 未被钳制。
    pub clamped_by: Vec<String>,
    /// 六轴投影，与 `self_config_info` 的 `permission` 小节同源（同一个 [`PermissionCapsView`]），
    /// 避免两处漂移。
    pub caps: PermissionCapsView,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_types::PermissionLevel;

    #[test]
    fn level_source_strings_match_serde() {
        assert_eq!(PermissionLevelSource::Config.as_str(), "config");
        assert_eq!(PermissionLevelSource::Persisted.as_str(), "persisted");
        assert_eq!(
            serde_json::to_string(&PermissionLevelSource::Persisted).expect("serialize"),
            "\"persisted\""
        );
    }

    #[test]
    fn view_serializes_flat_by_design() {
        let view = PermissionView {
            level: PermissionLevel::Approved.as_str().to_string(),
            level_source: PermissionLevelSource::Config,
            ceiling: PermissionLevel::Unrestricted.as_str().to_string(),
            ceiling_source: "derived_loopback".to_string(),
            available: vec!["chat".to_string(), "read".to_string()],
            clamped_by: Vec::new(),
            caps: PermissionLevel::Approved.caps_view(),
        };
        let json: serde_json::Value =
            serde_json::to_value(&view).expect("PermissionView 必须可序列化");
        assert_eq!(json["level"], "approved");
        assert_eq!(json["level_source"], "config");
        assert_eq!(json["ceiling_source"], "derived_loopback");
        assert_eq!(json["available"][1], "read");
        // 空 `clamped_by` 仍要出现在响应里（客户端按数组判断，不能用缺键推断）。
        assert!(json["clamped_by"].is_array());
        assert_eq!(json["caps"]["approval"], "gate");
    }
}
