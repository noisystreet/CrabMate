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

use std::collections::HashSet;
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

/// L1 只读工具的**正列表**来源（§4.2 / §7.2）。
///
/// 两条分支都**不读** `tool_registry_write_effect_tools`：那是并行只读批的性能开关，可被
/// 运维整体替换，拿它当安全边界会在漏配时直接放行写工具。
#[derive(Clone)]
pub enum ReadonlyAllow {
    /// 配置显式写出的正列表（`permission_read_tools`）。
    Explicit(Arc<HashSet<String>>),
    /// 省略配置：由「写副作用工具清单的补集」推导。
    Derived,
}

impl ReadonlyAllow {
    fn from_cfg(cfg: &AgentConfig) -> Self {
        match &cfg.permission.permission_read_tools {
            Some(list) => ReadonlyAllow::Explicit(Arc::new(
                list.iter()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            )),
            None => ReadonlyAllow::Derived,
        }
    }

    /// 该名字是否在 L1 允许执行的只读集合里。
    #[must_use]
    pub fn allows(&self, name: &str) -> bool {
        match self {
            ReadonlyAllow::Explicit(set) => set.contains(name),
            ReadonlyAllow::Derived => {
                crate::cm_tools::registry_policy::is_readonly_tool_for_permission(name)
            }
        }
    }
}

impl std::fmt::Debug for ReadonlyAllow {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ReadonlyAllow::Explicit(set) => write!(f, "Explicit({})", set.len()),
            ReadonlyAllow::Derived => f.write_str("Derived"),
        }
    }
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
    /// L1 的只读正列表；仅 `tool_visibility = Readonly` 时有值（其余等级为 `None`）。
    pub readonly_allow: Option<ReadonlyAllow>,
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
        readonly_allow: None,
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
            readonly_allow: None,
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
            // 正列表在投影期算一次（显式配置或由写副作用清单推导），执行期只查表。
            readonly_allow: Some(ReadonlyAllow::from_cfg(cfg)),
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
            readonly_allow: None,
            external_tools: ExternalTools::Allowed,
            path_scope: PathScope::Any,
            command_scope: CommandScope::Any,
            net_scope: NetScope::Any,
            approval: ApprovalPolicy::AuditOnly,
        },
    }
}

/// 命令类工具（§8.1 的四个入口）：`command_scope = None`（L0 / L1）时必须硬拒。
///
/// 这是**双保险**：可见性已经不下发这些工具，但模型仍可能凭历史发出调用，故在 early-deny
/// 处再拒一次，不依赖 `is_readonly_tool` 是否漏配。
pub const COMMAND_TOOLS: [&str; 4] = [
    "run_command",
    "terminal_session",
    "python_snippet_run",
    "run_executable",
];

/// 是否为命令类工具（见 [`COMMAND_TOOLS`]）。
#[must_use]
pub fn is_command_tool(name: &str) -> bool {
    COMMAND_TOOLS.contains(&name)
}

/// 单工具是否允许在 `caps` 下**执行**：可见性过滤与 early-deny 共用这一条判据（§7.4「不要第二套」）。
///
/// L2–L4（`tool_visibility = Full`）恒为 `true`：路径 / 命令 / 网络三轴的越界处理在 S3 接线，
/// 此处只做「进不进得来」的判定，避免 S2 就改变现状档位的行为。
#[must_use]
pub fn tool_allowed_by_caps(caps: &PermissionCaps, name: &str) -> bool {
    if matches!(caps.command_scope, CommandScope::None) && is_command_tool(name) {
        return false;
    }
    match caps.tool_visibility {
        ToolVisibility::None => false,
        ToolVisibility::Full => true,
        // 只读正列表**只**对内置工具有效：外部工具（`mcp__*` / `plugins/*.json`）的可用性
        // 由 `external_tools` 轴决定。否则运维把 `mcp__x` 写进 `permission_read_tools`
        // 就绕过了 L1 的「外部工具 Denied」（设计 §8.1）。
        ToolVisibility::Readonly => {
            crate::cm_tools::registry_policy::is_builtin_tool(name)
                && caps
                    .readonly_allow
                    .as_ref()
                    .is_some_and(|allow| allow.allows(name))
        }
    }
}

/// 被权限等级拒绝时的工具结果正文（日志与模型共用，含等级与一句话承诺）。
#[must_use]
pub fn permission_denied_message(caps: &PermissionCaps, name: &str) -> String {
    format!(
        "工具 `{name}` 在当前权限等级（{}）下不可用：{}。请改用只读方式完成，或请用户调整 permission_level。",
        caps.level,
        caps.level.promise_zh()
    )
}

/// 回合级装配：解析等级 → 投影 `caps`，并按 L0 需要收窄本回合的终答计划要求。
///
/// 与投影同处一模块，是为了让「L0 没有工具可下发」这条规则只有一个落点（设计 §8.2）：
/// 调用方只需给出本回合输入，不必各自记得改 `per_plan_policy`。
pub fn plan_turn_permission(
    cfg: &mut AgentConfig,
    role_max: Option<PermissionLevel>,
    session_mode: SessionMode,
) -> PermissionCaps {
    let resolved = resolve_permission_level_for_turn(PermissionTurnInput {
        cfg: &*cfg,
        requested: None,
        persisted: None,
        role_max,
        session_mode,
    });
    // 投影取 `projection_level` 而非 `effective`：`ask` / `plan` 的只读由既有
    // `ReviewReadonly` 执行，不能回灌成 L1 的正列表（见 [`ResolvedPermission`]）。
    let caps = project_caps(resolved.projection_level, cfg);
    if caps.tool_visibility == ToolVisibility::None {
        cfg.per_plan_policy.final_plan_requirement =
            crate::cm_config::FinalPlanRequirementMode::Never;
    }
    caps
}

fn allowlist_from_cfg(cfg: &AgentConfig) -> CommandScope {
    CommandScope::Allowlist(Arc::clone(&cfg.command_exec.allowed_commands))
}

/// 回合快照 + 权限 `caps` 一次算好：`run_agent_turn` 的入口装配。
///
/// 快照要带上本回合的 `chat_workspace_root`（出站 `@` 图跟本轮 `work_dir`），而 caps 的
/// 命令白名单 / 出站前缀与它无关，故两者可以合并成一次调用，避免入口函数继续膨胀
/// （仓库门禁：fn nloc ≤ 200）。
pub fn prepare_turn_cfg_and_permission(
    cfg: &AgentConfig,
    workspace_root: &std::path::Path,
    role_max: Option<PermissionLevel>,
    session_mode: SessionMode,
) -> (Arc<AgentConfig>, PermissionCaps) {
    let mut snap = cfg.clone();
    snap.chat_workspace_root = Some(workspace_root.to_path_buf());
    let caps = plan_turn_permission(&mut snap, role_max, session_mode);
    (Arc::new(snap), caps)
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
    /// 投影 [`PermissionCaps`] 时该用的等级。
    ///
    /// 与 `effective` 只在一处分叉：**仅**由 `session_mode` 造成的收窄不回灌到投影。
    /// `ask` / `plan` 的只读由既有 `ReviewReadonly` 通道执行（`step_executor_constraint`，
    /// 语义含「MCP 代理放行 + `review_readonly_deny_tools`」）；若此处也按 L1 投影，
    /// 默认档（`approved`）的 Plan 会话会连带丢掉 MCP 工具——等于让会话模式悄悄改变默认行为，
    /// 那正是设计 §7.4「不要第二套判据」要避免的。
    pub projection_level: PermissionLevel,
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
/// projection_level = 上面那条 effective，**会话模式之前**的值（见 [`ResolvedPermission::projection_level`]）
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
    // 等级轴到此为止：投影（工具可见性 / 命令 / 网络）只认这一档。
    let projection_level = effective;
    // 会话模式是**另一条轴**：它只缩小 `effective` 的可报告值，回溯到 `caps` 会让
    // `ask` / `plan` 顶着 L1 的正列表去跑，从而丢掉 ReviewReadonly 本来的 MCP 放行。
    if input.session_mode.requires_readonly_tools() && effective > PermissionLevel::Read {
        effective = PermissionLevel::Read;
        clamped_by.push(PermissionClamp::SessionMode);
    }

    ResolvedPermission {
        effective,
        projection_level,
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
            assert_eq!(got.projection_level, PermissionLevel::Approved, "{mode}");
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

    /// L3（默认档）必须与改造前逐字等价：任何内置工具都被允许，命令类工具也不例外。
    /// 这是 S2 的验收标准（设计 §12「S3 是唯一可能悄悄改变现有行为的切片」的前置约束）。
    #[test]
    fn approved_level_allows_every_tool_like_before() {
        let cfg = test_cfg();
        let caps = project_caps(PermissionLevel::Approved, &cfg);
        for name in [
            "read_file",
            "run_command",
            "terminal_session",
            "python_snippet_run",
            "run_executable",
            "modify_file",
            "delete_dir",
            "mcp__demo__tool",
        ] {
            assert!(tool_allowed_by_caps(&caps, name), "L3 不应拒绝 {name}");
        }
    }

    /// L0：不下发也不执行任何工具（连只读都不给）。
    #[test]
    fn chat_level_denies_every_tool() {
        let cfg = test_cfg();
        let caps = project_caps(PermissionLevel::Chat, &cfg);
        for name in ["read_file", "glob_files", "present_clarification_questionnaire"] {
            assert!(!tool_allowed_by_caps(&caps, name), "L0 不应允许 {name}");
        }
    }

    /// L1：推导正列表 = 「内置且非写副作用」；写工具、命令与外部工具一律拒。
    #[test]
    fn read_level_allows_only_derived_readonly_tools() {
        let cfg = test_cfg();
        assert!(
            cfg.permission.permission_read_tools.is_none(),
            "本用例针对推导路径，需要未显式配置正列表"
        );
        let caps = project_caps(PermissionLevel::Read, &cfg);
        assert!(tool_allowed_by_caps(&caps, "read_file"));
        assert!(tool_allowed_by_caps(&caps, "glob_files"));
        for name in [
            "run_command",
            "terminal_session",
            "modify_file",
            "delete_dir",
            "mcp__demo__tool",
        ] {
            assert!(!tool_allowed_by_caps(&caps, name), "L1 不应允许 {name}");
        }
    }

    /// L1 显式正列表：**只**放行名单内名字，覆盖推导结果。
    #[test]
    fn read_level_explicit_positive_list_overrides_derivation() {
        let mut cfg = test_cfg();
        cfg.permission.permission_read_tools = Some(vec!["read_file".to_string()]);
        let caps = project_caps(PermissionLevel::Read, &cfg);
        assert!(tool_allowed_by_caps(&caps, "read_file"));
        // `glob_files` 本身是只读的（推导路径会放行），但显式名单没写它 → 拒。
        assert!(!tool_allowed_by_caps(&caps, "glob_files"));
    }

    /// 推导路径**不读** `tool_registry_write_effect_tools`：运维为调并行把 `run_command`
    /// 移出写集合后，L1 仍必须拒它（把性能开关当安全边界会静默放行子进程）。
    #[test]
    fn read_level_ignores_write_effect_perf_override() {
        let mut cfg = test_cfg();
        cfg.tool_registry_policy.tool_registry_write_effect_tools =
            Some(Arc::new(HashSet::from(["create_file".to_string()])));
        let caps = project_caps(PermissionLevel::Read, &cfg);
        assert!(!tool_allowed_by_caps(&caps, "run_command"));
        assert!(!tool_allowed_by_caps(&caps, "modify_file"));
    }

    /// 拒绝正文带上等级与该级的一句话承诺，便于模型与用户对齐。
    #[test]
    fn denial_message_mentions_level_and_promise() {
        let cfg = test_cfg();
        let caps = project_caps(PermissionLevel::Read, &cfg);
        let msg = permission_denied_message(&caps, "run_command");
        assert!(msg.contains("run_command"), "{msg}");
        assert!(msg.contains(PermissionLevel::Read.as_str()), "{msg}");
        assert!(msg.contains(PermissionLevel::Read.promise_zh()), "{msg}");
    }

    /// `ask` / `plan` 的只读由 `ReviewReadonly` 执行，投影不得回灌成 L1：否则默认档下
    /// Plan 会话会丢掉 MCP 代理工具（`step_executor_policy` 明确放行，见
    /// `review_readonly_allows_mcp_proxy`），等于让会话模式悄悄改了默认行为。
    #[test]
    fn session_mode_does_not_shrink_projected_caps() {
        let cfg = test_cfg();
        for mode in [SessionMode::Ask, SessionMode::Plan] {
            let resolved = resolve_permission_level_for_turn(input(
                &cfg,
                Some(PermissionLevel::Approved),
                None,
                None,
                mode,
            ));
            assert_eq!(resolved.effective, PermissionLevel::Read, "{mode}");
            let caps = project_caps(resolved.projection_level, &cfg);
            assert_eq!(
                caps.tool_visibility,
                ToolVisibility::Full,
                "{mode} 不应把投影缩成只读"
            );
            assert!(
                tool_allowed_by_caps(&caps, "mcp__demo__tool"),
                "{mode} 下 MCP 代理仍应由 ReviewReadonly 放行"
            );
        }
    }

    /// 显式正列表也放不出外部工具：L1 下 `mcp__*` / 动态工具只受 `external_tools` 轴管，
    /// 否则把 `mcp__x` 写进 `permission_read_tools` 就绕过了 L1 的外部工具禁用。
    #[test]
    fn read_level_explicit_list_cannot_whitelist_external_tools() {
        let mut cfg = test_cfg();
        cfg.permission.permission_read_tools = Some(vec![
            "read_file".to_string(),
            "mcp__demo__tool".to_string(),
        ]);
        let caps = project_caps(PermissionLevel::Read, &cfg);
        assert!(tool_allowed_by_caps(&caps, "read_file"));
        assert!(!tool_allowed_by_caps(&caps, "mcp__demo__tool"));
    }

    /// L0 装配：`chat` 档把本回合的终答计划要求收敛到 `never`（设计 §8.2），并保留空工具面。
    #[test]
    fn prepare_turn_relaxes_final_plan_requirement_for_chat_level() {
        let mut cfg = test_cfg();
        cfg.permission.permission_level = PermissionLevel::Chat;
        cfg.permission.permission_level_max_effective = PermissionLevel::Unrestricted;
        let (snap, caps) =
            prepare_turn_cfg_and_permission(&cfg, std::path::Path::new("ws"), None, SessionMode::Act);
        assert_eq!(caps.tool_visibility, ToolVisibility::None);
        assert_eq!(
            snap.per_plan_policy.final_plan_requirement,
            crate::cm_config::FinalPlanRequirementMode::Never
        );
        assert_eq!(
            snap.chat_workspace_root.as_deref(),
            Some(std::path::Path::new("ws"))
        );
    }

    /// 非 L0 档不得动 `final_plan_requirement`：默认档的编排行为必须逐字不变。
    #[test]
    fn prepare_turn_keeps_final_plan_requirement_for_approved_level() {
        let cfg = test_cfg();
        let before = cfg.per_plan_policy.final_plan_requirement;
        let (snap, caps) =
            prepare_turn_cfg_and_permission(&cfg, std::path::Path::new("ws"), None, SessionMode::Act);
        assert_eq!(caps.tool_visibility, ToolVisibility::Full);
        assert_eq!(snap.per_plan_policy.final_plan_requirement, before);
    }
}
