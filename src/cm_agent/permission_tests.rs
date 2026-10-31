//! [`permission`] 的单元测试。
//!
//! 拆出成独立文件：主文件行数受仓库 `fn-nloc-ratchet` 门禁限制（文件 ≤ 920 行），
//! 与本仓 `text_sanitize/tests.rs` 同模式。

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

/// S3 观测出口：`cm_types::PermissionLevel::caps_view()`（叶子层 `self_config_info` 用）必须
/// 与权威投影 [`project_caps`] 的 `view()` 逐等级一致——两处轴名映射**不允许漂移**。
#[test]
fn level_caps_view_matches_projection() {
    let cfg = test_cfg();
    for level in PermissionLevel::ALL {
        assert_eq!(
            level.caps_view(),
            project_caps(level, &cfg).view(),
            "{level:?} 的轴名映射与 project_caps 不一致"
        );
    }
}

/// S3：`command_scope` 三态到白名单的映射（`Any` 必须落成 `*` 哨兵）。
#[test]
fn command_allowlist_maps_three_states() {
    let cfg = test_cfg();
    assert!(
        project_caps(PermissionLevel::Read, &cfg)
            .command_allowlist()
            .is_none(),
        "L1 命令全禁"
    );
    let l2 = project_caps(PermissionLevel::Workspace, &cfg)
        .command_allowlist()
        .expect("L2 有白名单");
    assert_eq!(l2.as_ref(), cfg.command_exec.allowed_commands.as_ref());
    let l4 = project_caps(PermissionLevel::Unrestricted, &cfg)
        .command_allowlist()
        .expect("L4 全放行");
    assert_eq!(l4.as_ref(), ["*".to_string()].as_slice());
}

/// S3：`external_tools = Denied` 时 `mcp__*` / `dyn__*` 在 early-deny 就被拒，
/// 而不是靠「没进 tools 列表」兜着。
#[test]
fn external_tools_denied_below_l3() {
    let cfg = test_cfg();
    for level in [
        PermissionLevel::Chat,
        PermissionLevel::Read,
        PermissionLevel::Workspace,
    ] {
        let caps = project_caps(level, &cfg);
        assert!(
            !tool_allowed_by_caps(&caps, "mcp__fanalyzer__list"),
            "{level:?} 不应放行 MCP 代理工具"
        );
        assert!(
            !tool_allowed_by_caps(&caps, "dyn__deploy"),
            "{level:?} 不应放行动态工具"
        );
    }
    for level in [PermissionLevel::Approved, PermissionLevel::Unrestricted] {
        let caps = project_caps(level, &cfg);
        assert!(tool_allowed_by_caps(&caps, "mcp__fanalyzer__list"), "{level:?}");
        assert!(tool_allowed_by_caps(&caps, "dyn__deploy"), "{level:?}");
    }
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
        prepare_turn_cfg_and_permission(
            &cfg,
            std::path::Path::new("ws"),
            TurnPermissionStages {
                role_max: None,
                session_mode: SessionMode::Act,
                requested: None,
                persisted: None,
            },
        );
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
        prepare_turn_cfg_and_permission(
            &cfg,
            std::path::Path::new("ws"),
            TurnPermissionStages {
                role_max: None,
                session_mode: SessionMode::Act,
                requested: None,
                persisted: None,
            },
        );
    assert_eq!(caps.tool_visibility, ToolVisibility::Full);
    assert_eq!(snap.per_plan_policy.final_plan_requirement, before);
}

/// S5b 接线回归：`requested` 档必须真的进 min 链。
///
/// 若把请求值合并进 `persisted` 再传（那样 `PermissionTurnInput.requested` 恒为 `None`），
/// 这里会红——而「解析 → 选档 → 落盘」各段单测仍然全绿。这正是本轮修正要防的形态：
/// 客户端在 `POST /chat` 里切到 L1 不能是空操作。
#[test]
fn requested_stage_reaches_caps_and_wins_over_persisted() {
    let mut cfg = test_cfg();
    cfg.permission.permission_level = PermissionLevel::Approved;
    cfg.permission.permission_level_max_effective = PermissionLevel::Unrestricted;

    let (_, caps) = prepare_turn_cfg_and_permission(
        &cfg,
        std::path::Path::new("ws"),
        TurnPermissionStages {
            role_max: None,
            session_mode: SessionMode::Act,
            requested: Some(PermissionLevel::Read),
            persisted: Some(PermissionLevel::Approved),
        },
    );
    assert_eq!(
        caps.tool_visibility,
        ToolVisibility::Readonly,
        "requested 必须优先于 persisted"
    );

    // 只有 persisted 时仍走它：两档各自保留，互不遮蔽。
    let (_, caps) = prepare_turn_cfg_and_permission(
        &cfg,
        std::path::Path::new("ws"),
        TurnPermissionStages {
            role_max: None,
            session_mode: SessionMode::Act,
            requested: None,
            persisted: Some(PermissionLevel::Read),
        },
    );
    assert_eq!(caps.tool_visibility, ToolVisibility::Readonly);
}

/// §16.2 的 `available`：上界必须与 `resolve_permission_level_for_turn` 的 min 链一致。
///
/// 取上界作为请求时 `effective` 正好落在上界，取更高档（L4）时也只到上界——两处若各写一套
/// 三轴逻辑，这个测试会立刻红（防「第二套判据」）。
#[test]
fn selectable_cap_matches_resolve_chain() {
    let cfg = test_cfg();
    let cases = [
        (None, SessionMode::Act),
        (Some(PermissionLevel::Read), SessionMode::Act),
        (Some(PermissionLevel::Workspace), SessionMode::Plan),
        (None, SessionMode::Ask),
    ];
    for (role_max, session_mode) in cases {
        let cap = selectable_max_level(&cfg, role_max, session_mode);
        let levels = selectable_levels(&cfg, role_max, session_mode);
        assert_eq!(
            levels.last().copied(),
            Some(cap),
            "available 的最高项必须等于上界（role_max={role_max:?} mode={session_mode:?}）"
        );
        assert!(
            levels.windows(2).all(|pair| pair[0] < pair[1]),
            "available 必须自低到高且不重复：{levels:?}"
        );
        let at_cap = resolve_permission_level_for_turn(input(
            &cfg,
            Some(cap),
            None,
            role_max,
            session_mode,
        ));
        assert_eq!(at_cap.effective, cap, "申请上界应正好落在上界");
        let top_requested = resolve_permission_level_for_turn(input(
            &cfg,
            Some(PermissionLevel::Unrestricted),
            None,
            role_max,
            session_mode,
        ));
        assert_eq!(
            top_requested.effective, cap,
            "申请 L4 也只能到上界（等价于「选项集里没有更高档」）"
        );
    }
}

/// 三条轴各自都能单独收紧 `available`，而不是只有取 min 后的合成效果。
#[test]
fn each_axis_narrows_selectable_levels() {
    let mut cfg = test_cfg();
    cfg.permission.permission_level_max_effective = PermissionLevel::Workspace;
    assert_eq!(
        selectable_levels(&cfg, None, SessionMode::Act),
        vec![
            PermissionLevel::Chat,
            PermissionLevel::Read,
            PermissionLevel::Workspace
        ],
        "ceiling 收到 L2"
    );
    assert_eq!(
        selectable_levels(&cfg, Some(PermissionLevel::Read), SessionMode::Act),
        vec![PermissionLevel::Chat, PermissionLevel::Read],
        "角色上限收到 L1"
    );
    assert_eq!(
        selectable_levels(&cfg, None, SessionMode::Ask),
        vec![PermissionLevel::Chat, PermissionLevel::Read],
        "ask / plan 收到 L1"
    );
}
