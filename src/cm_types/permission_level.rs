//! 工具权限等级（L0–L4）与服务端上限（ceiling）来源。
//!
//! 设计见 `docs/design/permission_levels.md`：§4 等级定义、§5.1 上限推导、§9 安全边界。
//!
//! 本模块只放**枚举、解析与纯值构造**（如 [`scoped_allowlist_key`]：审批键的等级前缀，
//! 无 IO、无配置依赖）。「等级 → 六轴能力」的投影在
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

/// persistent allowlist（`AllowAlways` 的会话级集合）的**等级作用域键**：`l{level}|{key}`。
///
/// 设计 §6.3：写入用当回合的**生效**等级；**命中要求等级严格相等**（用当前等级现算键去
/// 比对），不是 `>=`。用 `>=` 会让 L3 批过的 URL / 命令在 L2 会话继续命中，而 L2 的全部
/// 意义就是「人工例外通道关闭」。
///
/// `level = None` 表示该调用路径尚未接线等级（如无 `caps` 的旧测试路径），返回裸键，
/// 与引入等级前逐字一致。
#[must_use]
pub fn scoped_allowlist_key(level: Option<PermissionLevel>, key: &str) -> String {
    match level {
        Some(level) => format!("l{}|{}", level.index(), key),
        None => key.to_string(),
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

/// 越界处理策略（六轴投影的 `approval` 轴）。
///
/// 定义在本层而非 [`crate::cm_agent::permission`]：审计事件（[`crate::cm_types::permission_audit`]）
/// 与文件路径访问器（`cm_tools::tools::file::PathAccess`）都要判断它，而依赖方向是
/// `cm_types` → `cm_config` → `cm_tools` → `cm_agent`，反向引用会让叶子层抬升。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// L0–L2：越界即拒（不弹审批；无人值守可解释、可复现）。
    Deny,
    /// L3：SSE 人工审批（现状）。
    Gate,
    /// L4：放行 + 审计。
    AuditOnly,
}

impl ApprovalPolicy {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ApprovalPolicy::Deny => "deny",
            ApprovalPolicy::Gate => "gate",
            ApprovalPolicy::AuditOnly => "audit_only",
        }
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

/// 六轴投影的**对外序列化形态**（设计 §16.2 `GET /permission` 的 `caps`）。
///
/// 定义在本层而不是 [`crate::cm_agent::permission`]：`cm_tools` 的 `self_config_info` 也要输出
/// 同一份投影（§11.1「同源序列化，避免两处漂移」），而依赖方向是
/// `cm_types` → `cm_config` → `cm_tools` → `cm_agent`，反向引用会让 `cm_tools` 变成非叶子层。
/// 由 `PermissionCaps::view()` 唯一产生——白名单 / 前缀**不进**投影（内部数据、长度不可控），
/// 因此本结构可直接进 golden 与 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PermissionCapsView {
    pub tool_visibility: String,
    pub external_tools: String,
    pub path_scope: String,
    pub command_scope: String,
    pub net_scope: String,
    pub approval: String,
}

impl PermissionLevel {
    /// 本等级六轴投影的**轴名**（`GET /permission` / `self_config_info` 的对外形态）。
    ///
    /// 权威投影是 [`crate::cm_agent::permission::project_caps`]（它还要用 [`crate::cm_config::AgentConfig`]
    /// 派生白名单 / 前缀 / 只读正列表，故不能下沉到本层）；本函数只做「等级 → 轴名」的映射，
    /// 供叶子层（`cm_tools::tools::self_config_info`）在不反向依赖 `cm_agent` 的前提下输出同一形态。
    /// 两者**不允许漂移**：一致性由 `cm_agent::permission::tests::level_caps_view_matches_projection`
    /// 逐等级钉住。
    #[must_use]
    pub fn caps_view(self) -> PermissionCapsView {
        let (tool_visibility, external_tools, path_scope, command_scope, net_scope, approval) =
            match self {
                PermissionLevel::Chat => ("none", "denied", "session_root", "none", "none", "deny"),
                PermissionLevel::Read => (
                    "readonly",
                    "denied",
                    "session_root",
                    "none",
                    "prefixes",
                    "deny",
                ),
                PermissionLevel::Workspace => (
                    "full",
                    "denied",
                    "session_root",
                    "allowlist",
                    "prefixes",
                    "deny",
                ),
                PermissionLevel::Approved => (
                    "full",
                    "allowed",
                    "session_root",
                    "allowlist",
                    "prefixes",
                    "gate",
                ),
                PermissionLevel::Unrestricted => {
                    ("full", "allowed", "any", "any", "any", "audit_only")
                }
            };
        PermissionCapsView {
            tool_visibility: tool_visibility.to_string(),
            external_tools: external_tools.to_string(),
            path_scope: path_scope.to_string(),
            command_scope: command_scope.to_string(),
            net_scope: net_scope.to_string(),
            approval: approval.to_string(),
        }
    }
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

/// 文件类工具的路径范围（`PermissionCaps::path_scope` 的取值）。
///
/// 放在 `cm_types` 而不是 `cm_agent::permission`：`cm_tools` 的文件工具要按它决定
/// 「是否仍在工作区根内解析」，而依赖方向是 `cm_types` → `cm_config` → `cm_tools` → `cm_agent`；
/// 让 `cm_tools` 反向依赖策略层会破坏分层。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PathScope {
    /// 仅当前会话工作区根（L0–L3 的现状）。
    #[default]
    SessionRoot,
    /// 会话根 + `workspace_allowed_roots`（S6）。
    AllowedRoots,
    /// 任意路径（L4）：文件工具跳过越界校验，`run_command` 的参数路径安全检查也随之放宽。
    Any,
}

impl PathScope {
    /// 线格式串（与 `serde` 的 `snake_case` 一致）。
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PathScope::SessionRoot => "session_root",
            PathScope::AllowedRoots => "allowed_roots",
            PathScope::Any => "any",
        }
    }

    /// 是否为「不受工作区根约束」（L4）。文件工具与命令参数安全检查据此走放宽分支。
    #[must_use]
    pub const fn is_unrestricted(self) -> bool {
        matches!(self, PathScope::Any)
    }
}
