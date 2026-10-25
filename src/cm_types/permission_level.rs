//! 工具权限等级（L0–L4）与服务端上限（ceiling）来源。
//!
//! 设计见 `docs/design/permission_levels.md`：§4 等级定义、§5.1 上限推导、§9 安全边界。
//!
//! 本模块只放**枚举与解析**。「等级 → 六轴能力」的投影在
//! [`crate::cm_agent::permission`]（依赖方向 `cm_types` → `cm_config` → `cm_tools` → `cm_agent`，
//! 投影要用 [`crate::cm_config::AgentConfig`]，因此不能下沉到本层）。

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// 单一旋钮：本回合的允许面，以及是否保留人工例外通道。
///
/// 声明顺序即严格程度，`Ord` 只用于 `min()` 式收窄比较（设计 §6.1）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default,
)]
#[serde(rename_all = "lowercase")]
pub enum PermissionLevel {
    /// L0：只能对话；不下发工具定义。
    Chat,
    /// L1：只读探查（工作区内读 + 出站只读），无写盘、无子进程。
    Read,
    /// L2：工作区硬边界；越界直接拒，不弹审批（无人值守档）。
    Workspace,
    /// L3：现状——基线同 L2，越界走人工审批。
    #[default]
    Approved,
    /// L4：不受工作区路径与命令白名单限制；放行 + 审计。
    Unrestricted,
}

impl PermissionLevel {
    /// 自低到高（= 声明顺序）。
    pub const ALL: [PermissionLevel; 5] = [
        PermissionLevel::Chat,
        PermissionLevel::Read,
        PermissionLevel::Workspace,
        PermissionLevel::Approved,
        PermissionLevel::Unrestricted,
    ];

    /// 配置 / 线格式串（与 `serde` 的 `lowercase` 一致）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionLevel::Chat => "chat",
            PermissionLevel::Read => "read",
            PermissionLevel::Workspace => "workspace",
            PermissionLevel::Approved => "approved",
            PermissionLevel::Unrestricted => "unrestricted",
        }
    }

    /// 数字序（L0 = 0 … L4 = 4）；供审批键 `l{n}|` 前缀与结构化日志使用（设计 §6.3）。
    #[must_use]
    pub const fn index(self) -> u8 {
        match self {
            PermissionLevel::Chat => 0,
            PermissionLevel::Read => 1,
            PermissionLevel::Workspace => 2,
            PermissionLevel::Approved => 3,
            PermissionLevel::Unrestricted => 4,
        }
    }

    /// 用户可见的一句话承诺（设计 §4.2）；用于启动日志与后续 `GET /permission` 文案。
    #[must_use]
    pub const fn promise_zh(self) -> &'static str {
        match self {
            PermissionLevel::Chat => "只能对话：不下发工具定义",
            PermissionLevel::Read => "只读探查：工作区内读 + 出站只读",
            PermissionLevel::Workspace => "工作区硬边界：越界直接拒，不弹审批",
            PermissionLevel::Approved => "现状：越界走人工审批",
            PermissionLevel::Unrestricted => "不受工作区路径与命令白名单限制：放行 + 审计",
        }
    }
}

impl fmt::Display for PermissionLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for PermissionLevel {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        parse_permission_level(s)
    }
}

/// 解析 `chat` / `read` / `workspace` / `approved` / `unrestricted`
/// （`trim` + 大小写不敏感；空串或未知 → `Err`，与 [`crate::cm_types::parse_session_mode`] 同规）。
pub fn parse_permission_level(raw: &str) -> Result<PermissionLevel, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok(PermissionLevel::Chat),
        "read" => Ok(PermissionLevel::Read),
        "workspace" => Ok(PermissionLevel::Workspace),
        "approved" => Ok(PermissionLevel::Approved),
        "unrestricted" => Ok(PermissionLevel::Unrestricted),
        "" => Err(
            "permission_level 不能为空（须为 chat / read / workspace / approved / unrestricted）"
                .into(),
        ),
        other => Err(format!(
            "未知 permission_level \"{other}\"（须为 chat / read / workspace / approved / unrestricted）"
        )),
    }
}

/// 可选字段：空 / 缺省 → `None`；非空须合法。
pub fn parse_optional_permission_level(
    raw: Option<&str>,
) -> Result<Option<PermissionLevel>, String> {
    match raw.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some(s) => parse_permission_level(s).map(Some),
    }
}

/// 服务端上限（ceiling）的来源，用于 `GET /permission` 的 `ceiling_source`（设计 §5.1）。
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default, PartialOrd, Ord,
)]
#[serde(rename_all = "snake_case")]
pub enum CeilingSource {
    /// 显式配置（TOML `permission_level_max` / `CM_PERMISSION_LEVEL_MAX`）。
    Config,
    /// 未配置且绑定 loopback：推导为 `unrestricted`（本机单用户）。
    DerivedLoopback,
    /// 未配置且绑定非 loopback：推导为 `approved`（信任域不同）。
    #[default]
    DerivedNonLoopback,
}

impl CeilingSource {
    /// 线格式串（`snake_case`，与 `serde` 一致）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            CeilingSource::Config => "config",
            CeilingSource::DerivedLoopback => "derived_loopback",
            CeilingSource::DerivedNonLoopback => "derived_non_loopback",
        }
    }

    /// 是否来自启动期推导（而非显式配置）。
    #[must_use]
    pub const fn is_derived(self) -> bool {
        matches!(
            self,
            CeilingSource::DerivedLoopback | CeilingSource::DerivedNonLoopback
        )
    }
}

impl fmt::Display for CeilingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_case_insensitive_and_trims() {
        assert_eq!(
            parse_permission_level(" Approved ").unwrap(),
            PermissionLevel::Approved
        );
        assert_eq!(
            parse_permission_level("Unrestricted").unwrap(),
            PermissionLevel::Unrestricted
        );
        assert_eq!(
            parse_permission_level("workspace").unwrap(),
            PermissionLevel::Workspace
        );
    }

    #[test]
    fn rejects_empty_and_unknown() {
        assert!(parse_permission_level("").is_err());
        assert!(parse_permission_level("   ").is_err());
        assert!(parse_permission_level("root").is_err());
        assert!(
            parse_optional_permission_level(Some("  "))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            parse_optional_permission_level(Some(" read ")).unwrap(),
            Some(PermissionLevel::Read)
        );
    }

    #[test]
    fn ordering_follows_declaration() {
        let ordered: Vec<PermissionLevel> = PermissionLevel::ALL.to_vec();
        let mut sorted = ordered.clone();
        sorted.sort();
        assert_eq!(sorted, ordered);
        assert!(PermissionLevel::Chat < PermissionLevel::Unrestricted);
        assert_eq!(
            PermissionLevel::Workspace.min(PermissionLevel::Read),
            PermissionLevel::Read
        );
    }

    #[test]
    fn default_is_approved_and_index_matches_order() {
        assert_eq!(PermissionLevel::default(), PermissionLevel::Approved);
        for (i, level) in PermissionLevel::ALL.iter().enumerate() {
            assert_eq!(usize::from(level.index()), i);
            assert_eq!(level.as_str().parse::<PermissionLevel>().unwrap(), *level);
            assert_eq!(level.to_string(), level.as_str());
        }
    }

    #[test]
    fn ceiling_source_roundtrip_strings() {
        assert_eq!(CeilingSource::Config.as_str(), "config");
        assert_eq!(CeilingSource::DerivedLoopback.as_str(), "derived_loopback");
        assert_eq!(
            CeilingSource::DerivedNonLoopback.as_str(),
            "derived_non_loopback"
        );
        assert!(CeilingSource::DerivedLoopback.is_derived());
        assert!(!CeilingSource::Config.is_derived());
        assert_eq!(
            serde_json::to_string(&CeilingSource::DerivedLoopback).unwrap(),
            "\"derived_loopback\""
        );
        assert_eq!(
            serde_json::from_str::<PermissionLevel>("\"workspace\"").unwrap(),
            PermissionLevel::Workspace
        );
    }
}
