//! `run_command` / `terminal_session` 的**路径参数门**在 S7 `AllowedRoots` 档下的矩阵。
//!
//! 「不弹窗」一律用**不给审批通道**（`web_ctx = None`）来证明：L3 下若误判成「需要审批」，
//! 会直接返回 `ApprovalUnavailable`。比数弹窗次数稳，也不依赖 fixture 的 channel 消费时序。
//!
//! 拆独立文件的原因同 `execute_read_dir_gate_tests.rs`：`execute_run_command_gate_tests.rs`
//! 正在逼近文件行数棘轮。

use super::run_command::{
    ExecMode, ExternalPathGate, PolicyApproval, approve_external_run_command_paths_if_needed,
    evaluate_policy,
};
use super::tests::{parsed_run_command, test_policy_context_with_caps_at};
use crate::cm_agent::permission::project_caps;
use crate::cm_types::{PathScope, PermissionLevel};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `base/{session,sibling}` + `workspace_allowed_roots = [base]` 的最小现场。
///
/// 布局与 `execute_read_dir_gate_tests.rs` 一致：`../sibling` 落在允许根内，
/// `../..` 与 `/etc/...` 落在根外。
struct Fixture {
    base: PathBuf,
    ws: PathBuf,
    cfg: Arc<crate::cm_config::AgentConfig>,
}

impl Fixture {
    fn new(tag: &str, allow_external: bool) -> Self {
        let base = std::env::temp_dir().join(format!("crabmate_cmd_s7_{tag}_{}", std::process::id()));
        std::fs::remove_dir_all(&base).ok();
        std::fs::create_dir_all(base.join("session")).expect("session");
        std::fs::create_dir_all(base.join("sibling")).expect("sibling");
        let base = base.canonicalize().expect("canonicalize");
        let ws = base.join("session");
        let mut cfg = crate::cm_config::load_config(None).expect("embed default");
        cfg.workspace_roots.workspace_allowed_roots = vec![base.clone()];
        // 外部 shell 可能设了 `CM_ALLOWED_COMMANDS=*`：两处开关都显式钉住。
        cfg.command_exec.allowed_commands = vec!["ls".to_string(), "cargo".to_string(), "cd".to_string()].into();
        cfg.command_exec.allow_external_path_with_approval = allow_external;
        Self {
            base,
            ws,
            cfg: Arc::new(cfg),
        }
    }

    fn ws(&self) -> &Path {
        self.ws.as_path()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.base).ok();
    }
}

/// 允许根内**全部**参数 ⇒ 放行，且不需要审批通道（L2 / L3 同结论）。
#[tokio::test]
async fn all_args_within_allowed_roots_allow_without_prompting() {
    let f = Fixture::new("within", true);
    let allowed = ["cargo".to_string()];
    // `=` 形态与空格形态各一例；后者顺带覆盖「输出目标不存在也不误判越界」。
    let equal_form =
        parsed_run_command(r#"{"command":"cargo","args":["--manifest-path=../sibling/Cargo.toml"]}"#);
    let spaced_form = parsed_run_command(
        r#"{"command":"cargo","args":["--manifest-path","../sibling/Cargo.toml","-o","../sibling/target/not-built.o"]}"#,
    );

    for level in [PermissionLevel::Workspace, PermissionLevel::Approved] {
        let caps = project_caps(level, f.cfg.as_ref());
        assert_eq!(caps.path_scope, PathScope::AllowedRoots, "{level:?}");
        let ctx = test_policy_context_with_caps_at(&f.cfg, &caps, None, ExecMode::Sync, f.ws());
        for parsed in [&equal_form, &spaced_form] {
            assert_eq!(
                approve_external_run_command_paths_if_needed(parsed, &allowed, &ctx)
                    .await
                    .expect("允许根内必须放行（无审批通道也放行）"),
                ExternalPathGate::ApprovedWithinRoots,
                "{level:?} / {}",
                parsed.spec.args.join(" ")
            );
        }
    }
}

/// 混入一个根外参数 ⇒ 只把**那一个**列出来，根内的不列。
#[tokio::test]
async fn partial_escape_lists_only_the_escaping_arg() {
    let f = Fixture::new("partial", true);
    let allowed = ["cargo".to_string()];
    let parsed = parsed_run_command(
        r#"{"command":"cargo","args":["--manifest-path","../sibling/Cargo.toml","-o","/etc/passwd"]}"#,
    );

    // L2：硬拒，正文点出越界参数且不含根内参数。
    let l2 = project_caps(PermissionLevel::Workspace, f.cfg.as_ref());
    let ctx = test_policy_context_with_caps_at(&f.cfg, &l2, None, ExecMode::Sync, f.ws());
    let msg = approve_external_run_command_paths_if_needed(&parsed, &allowed, &ctx)
        .await
        .expect_err("L2 下越界必须硬拒")
        .into_message()
        .0;
    assert!(msg.contains("/etc/passwd"), "{msg}");
    assert!(!msg.contains("sibling"), "越界正文不应列出根内参数：{msg}");

    // L3 + 无审批通道：仍必须拒（证明它没被当成「全在根内」放行），文案同样只列越界参数。
    let l3 = project_caps(PermissionLevel::Approved, f.cfg.as_ref());
    let ctx = test_policy_context_with_caps_at(&f.cfg, &l3, None, ExecMode::Sync, f.ws());
    let msg = approve_external_run_command_paths_if_needed(&parsed, &allowed, &ctx)
        .await
        .expect_err("L3 下越界需要审批通道")
        .into_message()
        .0;
    assert!(msg.contains("需要审批通道"), "{msg}");
    assert!(msg.contains("/etc/passwd"), "{msg}");
    assert!(!msg.contains("sibling"), "{msg}");
}

/// 配置开关关闭时：**只能**新增放行，不能新增拒绝（issue 语义 1）。
#[tokio::test]
async fn disabled_switch_still_allows_in_roots_but_does_not_add_denials() {
    let f = Fixture::new("switch_off", false);
    let allowed = ["cargo".to_string()];
    let l2 = project_caps(PermissionLevel::Workspace, f.cfg.as_ref());
    let ctx = test_policy_context_with_caps_at(&f.cfg, &l2, None, ExecMode::Sync, f.ws());

    let in_roots =
        parsed_run_command(r#"{"command":"cargo","args":["--manifest-path=../sibling/Cargo.toml"]}"#);
    assert_eq!(
        approve_external_run_command_paths_if_needed(&in_roots, &allowed, &ctx)
            .await
            .expect("开关关闭也要放行根内参数"),
        ExternalPathGate::ApprovedWithinRoots
    );

    // 根外参数：回 `NotNeeded`（由 prepare 侧报 `UnsafeArg`），**不**在 gate 层硬拒。
    let escaping = parsed_run_command(r#"{"command":"cargo","args":["-o","/etc/passwd"]}"#);
    assert_eq!(
        approve_external_run_command_paths_if_needed(&escaping, &allowed, &ctx)
            .await
            .expect("开关关闭不得新增硬拒"),
        ExternalPathGate::NotNeeded
    );
}

/// `cwd` 与 `cd` 前缀按同一根集合判：根内的 `cwd` 放行，`cd` 逃出全部根仍拒。
#[tokio::test]
async fn cwd_and_cd_prefix_use_the_same_root_set() {
    let f = Fixture::new("cd", true);
    let allowed = ["ls".to_string(), "cd".to_string()];
    let l2 = project_caps(PermissionLevel::Workspace, f.cfg.as_ref());
    let ctx = test_policy_context_with_caps_at(&f.cfg, &l2, None, ExecMode::Sync, f.ws());

    let cwd_inside = parsed_run_command(r#"{"command":"ls","cwd":"../sibling","args":[]}"#);
    assert_eq!(
        approve_external_run_command_paths_if_needed(&cwd_inside, &allowed, &ctx)
            .await
            .expect("cwd 在允许根内不该触发外部路径审批"),
        ExternalPathGate::ApprovedWithinRoots
    );

    // `cd ../..` 从 `base/session` 跳到 temp 根，落在全部允许根之外。
    let cd_escape = parsed_run_command(r#"{"command":"cd","args":["../..","&&","ls"]}"#);
    let msg = approve_external_run_command_paths_if_needed(&cd_escape, &allowed, &ctx)
        .await
        .expect_err("cd 逃出全部允许根必须拒")
        .into_message()
        .0;
    assert!(msg.contains("不允许工作区外路径参数"), "{msg}");
}

/// `terminal_session` 与 `run_command` 共用 `evaluate_policy`：Pty 档同样受益。
#[tokio::test]
async fn pty_mode_shares_the_same_root_judgement() {
    let f = Fixture::new("pty", true);
    let l2 = project_caps(PermissionLevel::Workspace, f.cfg.as_ref());
    let ctx = test_policy_context_with_caps_at(&f.cfg, &l2, None, ExecMode::Pty, f.ws());

    // 根内：放行，且不需要审批通道（误判成审批就会 `ApprovalUnavailable`）。
    let inside = parsed_run_command(r#"{"command":"ls","args":["../sibling"]}"#);
    let mut caps_snapshot = ctx.command_caps();
    let decision = evaluate_policy(&inside, &ctx, &mut caps_snapshot)
        .await
        .expect("Pty 下允许根内应放行，且不需要审批通道");
    assert!(decision.skip_arg_safety);
    assert!(
        !decision.approvals.contains(&PolicyApproval::ExternalPath),
        "根内放行不是审批，不该记 ExternalPath：{:?}",
        decision.approvals
    );

    // 根外：Pty 档同样落进同一条 gate——L2 是硬边界，直接拒（而非「需要审批通道」）。
    let outside = parsed_run_command(r#"{"command":"ls","args":["/etc/passwd"]}"#);
    let mut caps_snapshot = ctx.command_caps();
    let msg = evaluate_policy(&outside, &ctx, &mut caps_snapshot)
        .await
        .expect_err("Pty 下越界必须拒")
        .into_message()
        .0;
    assert!(msg.contains("不允许工作区外路径参数"), "{msg}");
}
