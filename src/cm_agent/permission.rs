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

use crate::cm_config::AgentConfig;
use crate::cm_names as tool_names;
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

/// 路径范围：定义在 [`crate::cm_types`]，此处重导出（`cm_tools` 的文件工具也要用它，
/// 不能让依赖方向反向）。
pub use crate::cm_types::PathScope;

/// 白名单「全放行」哨兵，与 [`crate::cm_tools::tools::allowlist_allows_all`] 同一约定。
///
/// `CommandScope::Any`（L4）靠它落成配置层能理解的值，不新增第二套「全放行」表示。
pub const COMMAND_ALLOW_ALL: &str = "*";

/// 命令范围。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandScope {
    /// L0 / L1：命令全禁（`run_command` / `terminal_session` / `python_snippet_run`
    /// 硬拒，S2 即生效）。
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

/// 越界处理策略（六轴投影的 `approval` 轴）：定义在 [`crate::cm_types`]，此处重导出。
///
/// 下沉到叶子层，是因为审计事件（[`crate::cm_types::permission_audit`]）与 `cm_tools` 的
/// `PathAccess` 都要判断它，而依赖方向不允许它们反向依赖本模块。
pub use crate::cm_types::ApprovalPolicy;

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

/// `caps` 的**对外序列化形态**：定义在 [`crate::cm_types`]（`cm_tools` 的 `self_config_info`
/// 也要输出同一份投影，叶子层不能反向依赖本模块），此处重导出以保持既有调用点不变。
pub use crate::cm_types::PermissionCapsView;

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

    /// 命令白名单（S3）：`None` = 本等级**全禁命令**（L0 / L1）；`Any` = `*` 哨兵。
    ///
    /// 返回 `Arc` 而不是 `&[String]`：`Any` 需要现造一个哨兵列表，与 `Allowlist` 分支
    /// 共用同一形状（[`crate::cm_tools::tools::allowlist_allows_all`] 认 `*`）。
    #[must_use]
    pub fn command_allowlist(&self) -> Option<Arc<[String]>> {
        match &self.command_scope {
            CommandScope::None => None,
            CommandScope::Allowlist(list) => Some(Arc::clone(list)),
            CommandScope::Any => Some(Arc::from([COMMAND_ALLOW_ALL.to_string()])),
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
pub const COMMAND_TOOLS: [&str; 3] = [
    tool_names::RUN_COMMAND,
    tool_names::TERMINAL_SESSION,
    tool_names::PYTHON_SNIPPET_RUN,
];

/// 是否为命令类工具（见 [`COMMAND_TOOLS`]）。
#[must_use]
pub fn is_command_tool(name: &str) -> bool {
    COMMAND_TOOLS.contains(&name)
}

/// `caps.external_tools = Denied` 时，非内置工具（`mcp__*` / `dyn__*`）一律不可执行。
///
/// `mcp__*` 与 `plugins/*.json` 动态工具的实际能力由**外部进程**决定，宿主侧的路径 /
/// 命令 / 网络三轴管不到它们；放进 L0–L2 就等于「硬边界」名不副实（设计 §7.1 / Q8）。
#[must_use]
pub fn external_tools_allowed_by_caps(caps: &PermissionCaps, name: &str) -> bool {
    caps.external_tools == ExternalTools::Allowed
        || crate::cm_tools::tool_naming::tool_origin(name)
            == crate::cm_tools::tool_naming::ToolOrigin::Builtin
}

/// 单工具是否允许在 `caps` 下**执行**：可见性过滤与 early-deny 共用这一条判据（§7.4「不要第二套」）。
///
/// 两条**先决**否决：外部工具轴（`Denied` 时非内置一律拒）与 `command_scope = None` 的命令类
/// 四工具。之后才按 `tool_visibility` 判定「进不进得来」；路径 / 命令白名单 / 网络的**越界**
/// 处理在各执行点按 `caps` 的相应轴做（S3）。
#[must_use]
pub fn tool_allowed_by_caps(caps: &PermissionCaps, name: &str) -> bool {
    if !external_tools_allowed_by_caps(caps, name) {
        return false;
    }
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

/// 工具执行面的**策略端口**（S3-b）。
///
/// 执行面（`DispatchToolPolicy`、串行 / 并行只读批、工作流节点）此前直接解构
/// [`PermissionCaps`] 的字段（`level` / `approval` / `path_scope` / `net_scope` /
/// `command_allowlist()`），宿主无法替换策略来源。本 trait 把执行面**实际消费**的那部分
/// 收成一个端口，执行面签名从此只认 `&dyn PolicyEngine`。
///
/// **判定规则没有第二套**：唯一的生产实现仍是 [`PermissionCaps`]（下方 `impl`），逐字委托
/// 本模块的既有自由函数。本 trait 不新增审批门，也不改 `permission_levels` 的语义。
///
/// 落点在 `cm_agent` 而不是叶子层：`NetScope` / `CommandScope` / `ToolVisibility` 这些轴
/// 本就定义在本模块，往 `cm_types` 复制一份会变成两套枚举。消费者（`agent` / `cm_workflow` /
/// `cm_internal`）都在本层之上，`cm_tools` 不消费本端口，因此不构成反向依赖。
///
/// `None` 语义由调用方保留（`Option<&dyn PolicyEngine>` 的 `None` = 未接线，等价于现状
/// 「只按 `cfg` 判」）；本 trait **不**提供「空策略」实现，避免把「没接线」变成
/// 「按某等级静默收窄」。
pub trait PolicyEngine: Send + Sync {
    /// 本回合权限等级（SSE 载荷、审计与拒因文案共用）。
    fn level(&self) -> PermissionLevel;

    /// 审批轴（`Deny` / `Gate` / `AuditOnly`）。
    fn approval(&self) -> crate::cm_types::ApprovalPolicy;

    /// 文件类工具的路径范围。
    fn path_scope(&self) -> crate::cm_types::PathScope;

    /// 出站网络是否**全禁**（`NetScope::None`，仅 L0）。
    fn outbound_denied(&self) -> bool;

    /// 出站网络是否**全放行**（`NetScope::Any`，仅 L4）。
    fn outbound_allows_any(&self) -> bool;

    /// 出站前缀白名单（`Prefixes` 的列表；全禁 / 全放行时返回空表）。
    ///
    /// 端口**不交出 `NetScope` 枚举本身**：调用点只需要「能不能出、放哪些前缀」；交出枚举
    /// 意味着将来新增一个网络档位时，每个调用点都要跟着改 match。
    fn outbound_prefixes(&self) -> &[String];

    /// 出站是否落在「查前缀」这一档。
    ///
    /// 由上面两个方法派生，保证「全禁 / 全放行 / 查前缀」三态不会各算一遍。
    fn outbound_uses_prefixes(&self) -> bool {
        !self.outbound_denied() && !self.outbound_allows_any()
    }

    /// 命令白名单；`None` = 本等级全禁命令，`["*"]` 哨兵 = 全放行。
    ///
    /// **实现契约**：`Some(vec![])` 表示**空白名单**（未列出的命令照常走审批），
    /// **不**表示「全禁」；全禁只能是 `None`。`run_command` 的 L0/L1 双保险就靠这个
    /// 区分——若实现方用空表表达全禁，那条闸门会静默失效。
    fn command_allowlist(&self) -> Option<Arc<[String]>>;

    /// 拒绝归因：`Some(axis)` = 该工具在本策略下不可执行，并按此审计轴留痕；`None` = 放行。
    ///
    /// **唯一判据**：实现方必须逐字复用既有判定，不要在端口层另写一份规则。归因顺序须与
    /// 判定的**先决否决顺序**一致（外部工具轴 → 命令类硬拒 → 可见性），否则拒绝理由与审计
    /// 事件会互相打脸。做成端口方法而不是让每个调用点自己判断，是为了让「谁被拒、按哪条轴」
    /// 只有一处实现。
    ///
    /// 返回值刻意复用审计轴（`PermissionAxis`）而不另立 `DenyReason`：决策与留痕分类
    /// 同源，调用点没有机会把「被拒」记到另一条轴上。
    fn denied_axis(&self, name: &str) -> Option<crate::cm_agent::permission_audit::PermissionAxis>;

    /// 单工具是否允许执行。
    ///
    /// 默认由 [`Self::denied_axis`] 派生，保证「放行判定」与「审计归因」不可能漂移；
    /// 实现方**不要**覆盖它去另算一遍。
    fn allows_tool(&self, name: &str) -> bool {
        self.denied_axis(name).is_none()
    }

    /// 被拒时的工具结果正文（含等级与一句话承诺）。
    fn deny_message(&self, name: &str) -> String;
}

/// 执行面策略端口的唯一生产实现：**逐字**委托本模块的自由函数，不做第二次判定。
///
/// 端口层唯一新增的是 `denied_axis` 的归因顺序（外部工具轴 → 命令类硬拒 → 可见性），
/// 它与 [`tool_allowed_by_caps`] 的先决否决顺序一一对应；`allows_tool` 由 `denied_axis`
/// 派生（见 trait 默认实现），所以「是否拒绝」与「按哪条轴留痕」不会各算一遍。
impl PolicyEngine for PermissionCaps {
    fn level(&self) -> PermissionLevel {
        self.level
    }

    fn approval(&self) -> crate::cm_types::ApprovalPolicy {
        self.approval
    }

    fn path_scope(&self) -> crate::cm_types::PathScope {
        self.path_scope
    }

    fn outbound_denied(&self) -> bool {
        matches!(self.net_scope, NetScope::None)
    }

    fn outbound_allows_any(&self) -> bool {
        matches!(self.net_scope, NetScope::Any)
    }

    fn outbound_prefixes(&self) -> &[String] {
        match &self.net_scope {
            NetScope::Prefixes(list) => list.as_slice(),
            NetScope::None | NetScope::Any => &[],
        }
    }

    fn command_allowlist(&self) -> Option<Arc<[String]>> {
        PermissionCaps::command_allowlist(self)
    }

    fn denied_axis(&self, name: &str) -> Option<crate::cm_agent::permission_audit::PermissionAxis> {
        use crate::cm_agent::permission_audit::PermissionAxis;
        if !external_tools_allowed_by_caps(self, name) {
            Some(PermissionAxis::External)
        } else if matches!(self.command_scope, CommandScope::None) && is_command_tool(name) {
            Some(PermissionAxis::Command)
        } else if tool_allowed_by_caps(self, name) {
            None
        } else {
            Some(PermissionAxis::Tool)
        }
    }

    fn deny_message(&self, name: &str) -> String {
        permission_denied_message(self, name)
    }
}

/// 回合权限 `min` 链的四档输入（设计 §6.1）。
///
/// 收成结构体而不是继续平铺形参：`RunAgentTurnAttach` 与三个 build args 都要把它穿到
/// `run_agent_turn`，平铺会让每个构造点多 4 行、并把 `run_agent_turn` 顶出 nloc ≤ 200 的棘轮。
#[derive(Debug, Clone, Copy)]
pub struct TurnPermissionStages {
    /// 角色级上限（`agent_roles[].max_permission_level`）；`None` = 不额外限制。
    pub role_max: Option<PermissionLevel>,
    /// 本回合会话模式（`ask` / `plan` → 再钳到 ≤ `read`）。
    pub session_mode: SessionMode,
    /// `/chat` 请求显式申请的等级；`None` = 未申请。
    pub requested: Option<PermissionLevel>,
    /// 会话持久化的意图；`None` = 该会话从未指定。
    pub persisted: Option<PermissionLevel>,
}

/// 回合级装配：解析等级 → 投影 `caps`，并按 L0 需要收窄本回合的终答计划要求。
///
/// 与投影同处一模块，是为了让「L0 没有工具可下发」这条规则只有一个落点（设计 §8.2）：
/// 调用方只需给出本回合输入，不必各自记得改 `per_plan_policy`。
pub fn plan_turn_permission(cfg: &mut AgentConfig, stages: TurnPermissionStages) -> PermissionCaps {
    let resolved = resolve_permission_level_for_turn(PermissionTurnInput {
        cfg: &*cfg,
        requested: stages.requested,
        persisted: stages.persisted,
        role_max: stages.role_max,
        session_mode: stages.session_mode,
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
    stages: TurnPermissionStages,
) -> (Arc<AgentConfig>, PermissionCaps) {
    let mut snap = cfg.clone();
    snap.chat_workspace_root = Some(workspace_root.to_path_buf());
    let caps = plan_turn_permission(&mut snap, stages);
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

/// `GET /permission` 的 `available` 上界（设计 §16.2）：`min(ceiling, role_max, session_mode)`。
///
/// 与 [`resolve_permission_level_for_turn`] 是**同一组三轴**，差别只在没看 `chosen`：
/// `available` 回答「这台机器 + 这个角色 + 这个会话模式下一共能选到哪一档」，
/// 而不是「这一轮最终落在哪一档」。上界一致性由
/// `permission_tests::selectable_cap_matches_resolve_chain` 钉住。
#[must_use]
pub fn selectable_max_level(
    cfg: &AgentConfig,
    role_max: Option<PermissionLevel>,
    session_mode: SessionMode,
) -> PermissionLevel {
    let ceiling = cfg.permission.effective_ceiling();
    let role_limit = role_max.unwrap_or(PermissionLevel::Unrestricted);
    // 会话模式是独立的一条轴：`ask` / `plan` 只读，能选的最高档就是 L1。
    let mode_limit = if session_mode.requires_readonly_tools() {
        PermissionLevel::Read
    } else {
        PermissionLevel::Unrestricted
    };
    ceiling.min(role_limit).min(mode_limit)
}

/// 选择器选项集（自低到高，全部 `≤` [`selectable_max_level`]）——`GET /permission` 的 `available`。
#[must_use]
pub fn selectable_levels(
    cfg: &AgentConfig,
    role_max: Option<PermissionLevel>,
    session_mode: SessionMode,
) -> Vec<PermissionLevel> {
    let cap = selectable_max_level(cfg, role_max, session_mode);
    PermissionLevel::ALL
        .iter()
        .copied()
        .filter(|level| *level <= cap)
        .collect()
}

#[cfg(test)]
#[path = "permission_tests.rs"]
mod tests;
