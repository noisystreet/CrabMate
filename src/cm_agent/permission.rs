//! 权限等级的**回合解析**（min 链）与**六轴投影**（`PermissionCaps`）。
//!
//! 设计见 `docs/design/permission_levels.md`：§6 解析与组合规则、§7 `PermissionCaps` 投影。
//!
//! 两条硬约束（§7.3）：
//!
//! 1. **只有本模块允许 `match level`**（唯一投影点）。各执行点只读 `caps.*` 的小枚举，
//!    不读 `cfg.permission_level`，否则新增能力轴要改 N 处，且 CCN 立刻顶破仓库门禁。
//! 2. 纯值结构、无 IO：可以整体 golden 比对，也可以被后续 `GET /permission` 直接序列化。
//!
//! S1 只提供类型与纯函数；执行点接线见 §8（S2–S4）。

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::cm_config::AgentConfig;
use crate::cm_types::{PermissionLevel, SessionMode};

/// 工具可见性（§7.1）：本回合向模型下发哪些 `tools` 定义。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolVisibility {
    /// L0：不下发任何工具定义。
    None,
    /// L1：只下发**独立正列表** `permission_read_tools` 里的工具。
    ///
    /// **不读** [`crate::cm_tools::registry_policy::is_readonly_tool`]：后者是并行只读批的
    /// 性能判据，且其写集合可被 `tool_registry_write_effect_tools` 整体替换——拿它当安全边界，
    /// 运维为调并行漏配 `run_command` 就会让 L1 直接放行子进程。
    Readonly,
    /// L2–L4：全量内置工具（再与角色 `allowed_tools` / `tool_overrides.json` 取交）。
    Full,
}

/// 不受 `caps` 约束的**外部工具**（MCP 代理 `mcp__*` 与工作区动态工具 `plugins/*.json`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternalTools {
    /// L0–L2：不进 `tools` 列表，且在 early-deny 处直接拒（§8.1）。
    Denied,
    /// L3 / L4：由 MCP server 自身与审批策略约束。
    Allowed,
}

/// 路径范围。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathScope {
    /// 仅当前会话工作区根（L0–L3 的现状）。
    SessionRoot,
    /// 会话根 + `workspace_allowed_roots`（S6）。
    AllowedRoots,
    /// 任意路径（L4）。
    Any,
}

/// 命令范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandScope {
    /// L0 / L1：命令全禁（`run_command` / `terminal_session` / `python_snippet_run` /
    /// `run_executable` 硬拒，S2 即生效）。
    None,
    /// L2 / L3：沿用 `allowed_commands`（直接复用配置里的 `Arc`，不复制）。
    Allowlist(Arc<[String]>),
    /// L4：等价于白名单哨兵 `["*"]`。
    Any,
}

/// 出站网络范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetScope {
    /// L0：出站全禁。
    None,
    /// L1–L3：沿用 `http_fetch_allowed_prefixes`。
    Prefixes(Arc<Vec<String>>),
    /// L4：等价于 `["*"]`。
    Any,
}

/// 越界处理策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApprovalPolicy {
    /// L0–L2：越界即拒（不弹审批；无人值守可解释、可复现）。
    Deny,
    /// L3：SSE 人工审批（现状）。
    Gate,
    /// L4：放行 + 审计。
    AuditOnly,
}

impl ToolVisibility {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ToolVisibility::None => "none",
            ToolVisibility::Readonly => "readonly",
            ToolVisibility::Full => "full",
        }
    }
}

impl ExternalTools {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            ExternalTools::Denied => "denied",
            ExternalTools::Allowed => "allowed",
        }
    }
}

impl PathScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PathScope::SessionRoot => "session_root",
            PathScope::AllowedRoots => "allowed_roots",
            PathScope::Any => "any",
        }
    }
}

impl CommandScope {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            CommandScope::None => "none",
            CommandScope::Allowlist(_) => "allowlist",
            CommandScope::Any => "any",
        }
    }
}

impl NetScope {
    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            NetScope::None => "none",
            NetScope::Prefixes(_) => "prefixes",
            NetScope::Any => "any",
        }
    }
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

/// 单回合的唯一能力快照：各执行点只消费本结构（§7.4「不要第二套」）。
#[derive(Debug, Clone)]
pub struct PermissionCaps {
    pub level: PermissionLevel,
    pub tool_visibility: ToolVisibility,
    pub external_tools: ExternalTools,
    pub path_scope: PathScope,
    pub command_scope: CommandScope,
    pub net_scope: NetScope,
    pub approval: ApprovalPolicy,
}

/// `caps` 的**对外序列化形态**：六轴各自的小写字符串（§16.2 `GET /permission` 的 `caps`）。
///
/// 由 [`PermissionCaps::view`] 唯一产生——白名单 / 前缀**不进**投影（它们是内部数据，且长度
/// 不可控），因此本结构可直接进 golden 与 HTTP 响应。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionCapsView {
    pub tool_visibility: String,
    pub external_tools: String,
    pub path_scope: String,
    pub command_scope: String,
    pub net_scope: String,
    pub approval: String,
}

impl PermissionCaps {
    /// 投影的对外形态（不含工具清单本身）。
    #[must_use]
    pub fn view(&self) -> PermissionCapsView {
        PermissionCapsView {
            tool_visibility: self.tool_visibility.as_str().to_string(),
            external_tools: self.external_tools.as_str().to_string(),
            path_scope: self.path_scope.as_str().to_string(),
            command_scope: self.command_scope.as_str().to_string(),
            net_scope: self.net_scope.as_str().to_string(),
            approval: self.approval.as_str().to_string(),
        }
    }
}

/// 等级 → 六轴投影（**唯**一的 `match level` 处，§7.3）。
///
/// L4 的三处「放行」全部复用既有开关，不需要新机制：`command_scope = Any` 等价于白名单哨兵
/// `["*"]`；`path_scope = Any` 等价于 `skip_arg_safety = true` + 文件工具走 `resolve_any`；
/// `net_scope = Any` 等价于 `http_fetch_allowed_prefixes = ["*"]`。
#[must_use]
pub fn project_caps(level: PermissionLevel, cfg: &AgentConfig) -> PermissionCaps {
    let full = || PermissionCaps {
        level,
        tool_visibility: ToolVisibility::Full,
        external_tools: ExternalTools::Allowed,
        path_scope: PathScope::SessionRoot,
        command_scope: allowlist_from_cfg(cfg),
        net_scope: prefixes_from_cfg(cfg),
        approval: ApprovalPolicy::Gate,
    };
    match level {
        PermissionLevel::Chat => PermissionCaps {
            level,
            tool_visibility: ToolVisibility::None,
            external_tools: ExternalTools::Denied,
            // L0 下路径轴不可达（没有工具）；取最严值，避免"枚举里有个看起来更宽的洞"。
            path_scope: PathScope::SessionRoot,
            command_scope: CommandScope::None,
            net_scope: NetScope::None,
            approval: ApprovalPolicy::Deny,
        },
        PermissionLevel::Read => PermissionCaps {
            level,
            tool_visibility: ToolVisibility::Readonly,
            external_tools: ExternalTools::Denied,
            path_scope: PathScope::SessionRoot,
            command_scope: CommandScope::None,
            net_scope: prefixes_from_cfg(cfg),
            approval: ApprovalPolicy::Deny,
        },
        PermissionLevel::Workspace => PermissionCaps {
            tool_visibility: ToolVisibility::Full,
            external_tools: ExternalTools::Denied,
            approval: ApprovalPolicy::Deny,
            ..full()
        },
        PermissionLevel::Approved => full(),
        PermissionLevel::Unrestricted => PermissionCaps {
            level,
            tool_visibility: ToolVisibility::Full,
            external_tools: ExternalTools::Allowed,
            path_scope: PathScope::Any,
            command_scope: CommandScope::Any,
            net_scope: NetScope::Any,
            approval: ApprovalPolicy::AuditOnly,
        },
    }
}

fn allowlist_from_cfg(cfg: &AgentConfig) -> CommandScope {
    CommandScope::Allowlist(Arc::clone(&cfg.command_exec.allowed_commands))
}

fn prefixes_from_cfg(cfg: &AgentConfig) -> NetScope {
    NetScope::Prefixes(Arc::new(cfg.http_fetch.http_fetch_allowed_prefixes.clone()))
}

/// 等级解析的输入（§6.1 的 min 链）。
///
/// 用结构体而非位置参数：字段会随 S5（请求体 / 持久化）继续增长，而仓库门禁限制形参 ≤ 9。
#[derive(Debug, Clone, Copy)]
pub struct PermissionTurnInput<'a> {
    /// 配置快照（读默认等级与生效上限）。
    pub cfg: &'a AgentConfig,
    /// `/chat` 请求显式申请的等级（`None` = 未申请）。
    pub requested: Option<PermissionLevel>,
    /// 会话持久化的「请求意图」（`None` = 该会话从未指定）。
    pub persisted: Option<PermissionLevel>,
    /// 角色上限 `agent_roles[].max_permission_level`。
    pub role_max: Option<PermissionLevel>,
    /// 本回合会话工作模式（`Ask` / `Plan` → 再钳到 ≤ `read`）。
    pub session_mode: SessionMode,
}

/// 收窄的轴（`clamped_by` 的取值，§16.2）。空数组表示未被钳制。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionClamp {
    /// 超过了服务端上限 `permission_level_max`。
    Ceiling,
    /// 超过了角色上限 `max_permission_level`。
    Role,
    /// `Ask` / `Plan` 模式把等级钳到 `read`。
    SessionMode,
}

impl PermissionClamp {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            PermissionClamp::Ceiling => "ceiling",
            PermissionClamp::Role => "role",
            PermissionClamp::SessionMode => "session_mode",
        }
    }
}

/// 一次解析的结果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPermission {
    /// 本回合生效等级（所有钳制之后）。
    pub effective: PermissionLevel,
    /// 请求原样回显：只回显一个值时，「我明明切了 L4，怎么还是只读」无从排查（§16.1）。
    pub requested: Option<PermissionLevel>,
    /// 实际收窄的轴，顺序固定 `ceiling` → `role` → `session_mode`。
    pub clamped_by: Vec<PermissionClamp>,
}

/// §6.1 的 min 链：
///
/// ```text
/// fallback  = min(default_level, ceiling, role_max)
/// chosen    = requested ?? persisted ?? fallback
/// effective = min(chosen, ceiling, role_max)，再按 session_mode 钳到 ≤ read
/// ```
///
/// 任何一步都只**收窄**，本设计里不存在提权路径（MB-1）。
#[must_use]
pub fn resolve_permission_level_for_turn(input: PermissionTurnInput<'_>) -> ResolvedPermission {
    let ceiling = input.cfg.permission.effective_ceiling();
    let role_limit = input.role_max.unwrap_or(PermissionLevel::Unrestricted);
    let fallback = input
        .cfg
        .permission
        .permission_level
        .min(ceiling)
        .min(role_limit);
    let chosen = input.requested.or(input.persisted).unwrap_or(fallback);

    let mut clamped_by = Vec::new();
    let mut effective = chosen;
    if effective > ceiling {
        effective = ceiling;
        clamped_by.push(PermissionClamp::Ceiling);
    }
    if effective > role_limit {
        effective = role_limit;
        clamped_by.push(PermissionClamp::Role);
    }
    if input.session_mode.requires_readonly_tools() && effective > PermissionLevel::Read {
        effective = PermissionLevel::Read;
        clamped_by.push(PermissionClamp::SessionMode);
    }

    ResolvedPermission {
        effective,
        requested: input.requested,
        clamped_by,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    fn test_cfg() -> AgentConfig {
        let mut cfg = crate::cm_config::load_config(None).expect("embed default");
        // 固定白名单 / 前缀，让投影 golden 不受嵌入默认值变动影响。
        cfg.command_exec.allowed_commands = vec!["cargo".to_string(), "git".to_string()].into();
        cfg.http_fetch.http_fetch_allowed_prefixes = vec!["https://example.com".to_string()];
        cfg
    }

    #[derive(Debug, Deserialize)]
    struct GoldenCaps {
        tool_visibility: String,
        external_tools: String,
        path_scope: String,
        command_scope: String,
        net_scope: String,
        approval: String,
    }

    #[derive(Debug, Deserialize)]
    struct GoldenLine {
        level: String,
        caps: GoldenCaps,
    }

    /// 五级 × 六轴 golden（设计 §13「`project_caps` 五级 golden」）。
    #[test]
    fn project_caps_matches_golden() {
        let raw = include_str!("fixtures/permission_caps_golden.jsonl");
        let cfg = test_cfg();
        let mut seen = 0usize;
        for line in raw.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let golden: GoldenLine = serde_json::from_str(line).expect("golden line is valid JSON");
            let level = crate::cm_types::parse_permission_level(&golden.level)
                .expect("golden level is a known permission level");
            let view = project_caps(level, &cfg).view();
            assert_eq!(
                view,
                PermissionCapsView {
                    tool_visibility: golden.caps.tool_visibility,
                    external_tools: golden.caps.external_tools,
                    path_scope: golden.caps.path_scope,
                    command_scope: golden.caps.command_scope,
                    net_scope: golden.caps.net_scope,
                    approval: golden.caps.approval,
                },
                "level {level} 的六轴投影与 golden 不一致"
            );
            seen += 1;
        }
        assert_eq!(seen, PermissionLevel::ALL.len(), "golden 必须覆盖五级");
    }

    #[test]
    fn caps_view_is_stable_json() {
        let cfg = test_cfg();
        let json = serde_json::to_string(&project_caps(PermissionLevel::Approved, &cfg).view())
            .expect("serialize");
        assert_eq!(
            json,
            r#"{"tool_visibility":"full","external_tools":"allowed","path_scope":"session_root","command_scope":"allowlist","net_scope":"prefixes","approval":"gate"}"#
        );
    }

    #[test]
    fn command_scope_shares_the_configured_arc() {
        let cfg = test_cfg();
        let caps = project_caps(PermissionLevel::Workspace, &cfg);
        let CommandScope::Allowlist(list) = &caps.command_scope else {
            panic!("L2 必须是白名单档");
        };
        assert!(Arc::ptr_eq(list, &cfg.command_exec.allowed_commands));
    }

    #[test]
    fn unrestricted_bypasses_all_three_axes() {
        let cfg = test_cfg();
        let caps = project_caps(PermissionLevel::Unrestricted, &cfg);
        assert_eq!(caps.path_scope, PathScope::Any);
        assert_eq!(caps.command_scope, CommandScope::Any);
        assert_eq!(caps.net_scope, NetScope::Any);
        assert_eq!(caps.approval, ApprovalPolicy::AuditOnly);
    }

    fn input<'a>(
        cfg: &'a AgentConfig,
        requested: Option<PermissionLevel>,
        persisted: Option<PermissionLevel>,
        role_max: Option<PermissionLevel>,
        session_mode: SessionMode,
    ) -> PermissionTurnInput<'a> {
        PermissionTurnInput {
            cfg,
            requested,
            persisted,
            role_max,
            session_mode,
        }
    }

    #[test]
    fn request_above_ceiling_is_clamped() {
        let mut cfg = test_cfg();
        cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            Some(PermissionLevel::Unrestricted),
            None,
            None,
            SessionMode::Act,
        ));
        assert_eq!(got.effective, PermissionLevel::Workspace);
        assert_eq!(got.requested, Some(PermissionLevel::Unrestricted));
        assert_eq!(got.clamped_by, vec![PermissionClamp::Ceiling]);
    }

    #[test]
    fn request_above_role_cap_is_clamped() {
        let cfg = test_cfg();
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            Some(PermissionLevel::Workspace),
            None,
            Some(PermissionLevel::Read),
            SessionMode::Act,
        ));
        assert_eq!(got.effective, PermissionLevel::Read);
        assert_eq!(got.clamped_by, vec![PermissionClamp::Role]);
    }

    #[test]
    fn persisted_intent_is_reclamped_against_new_ceiling() {
        let mut cfg = test_cfg();
        // 运维把上限调低后，旧会话的 `unrestricted` 意图不得续跑（§6.2 fail-safe）。
        cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            None,
            Some(PermissionLevel::Unrestricted),
            None,
            SessionMode::Act,
        ));
        assert_eq!(got.effective, PermissionLevel::Workspace);
        assert_eq!(got.clamped_by, vec![PermissionClamp::Ceiling]);
    }

    #[test]
    fn fallback_uses_configured_default_when_nothing_requested() {
        let mut cfg = test_cfg();
        cfg.permission.permission_level = PermissionLevel::Read;
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            None,
            None,
            None,
            SessionMode::Act,
        ));
        assert_eq!(got.effective, PermissionLevel::Read);
        assert!(got.clamped_by.is_empty());
    }

    #[test]
    fn ask_and_plan_modes_clamp_to_read() {
        let cfg = test_cfg();
        for mode in [SessionMode::Ask, SessionMode::Plan] {
            let got = resolve_permission_level_for_turn(input(
                &cfg,
                Some(PermissionLevel::Approved),
                None,
                None,
                mode,
            ));
            assert_eq!(got.effective, PermissionLevel::Read, "{mode}");
            assert_eq!(got.clamped_by, vec![PermissionClamp::SessionMode]);
        }
    }

    /// `clamped_by` 按 `ceiling` → `role` → `session_mode` 的固定顺序记录**实际发生**的收窄。
    ///
    /// 只写「真的降了」的轴：`role_max` 恰好等于钳制后的值时不算收窄（否则等于把
    /// 「配置写了个够严的上限」误报成「被收了权限」）。三轴同时收窄在五档枚举下不可能
    /// ——`session_mode` 的目标档 `read` 已是下限——故这里验证前两轴，`session_mode` 由
    /// `ask_and_plan_modes_clamp_to_read` 单独覆盖。
    #[test]
    fn clamps_are_reported_in_axis_order() {
        let mut cfg = test_cfg();
        cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            Some(PermissionLevel::Unrestricted),
            None,
            Some(PermissionLevel::Read),
            SessionMode::Act,
        ));
        assert_eq!(got.effective, PermissionLevel::Read);
        assert_eq!(
            got.clamped_by,
            vec![PermissionClamp::Ceiling, PermissionClamp::Role]
        );
        assert_eq!(PermissionClamp::SessionMode.as_str(), "session_mode");

        // 上限不变、角色不紧时：只有会话模式收窄（顺序仍从 ceiling 开始）。
        let got = resolve_permission_level_for_turn(input(
            &cfg,
            Some(PermissionLevel::Approved),
            None,
            None,
            SessionMode::Plan,
        ));
        assert_eq!(got.effective, PermissionLevel::Read);
        assert_eq!(
            got.clamped_by,
            vec![PermissionClamp::Ceiling, PermissionClamp::SessionMode]
        );
    }
}
