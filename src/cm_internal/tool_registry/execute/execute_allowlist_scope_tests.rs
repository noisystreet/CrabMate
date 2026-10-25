//! 等级作用域（`persistent allowlist` 的 `l{n}|` 前缀键）回归测试。
//!
//! 独立成文件：`execute_tests.rs` 已贴近仓库 `fn-nloc-ratchet` 的文件行数上限（≤ 920）。
//!
//! 覆盖设计 §6.3 的两条规则：
//! 1. `AllowAlways` 写入用**当回合等级**做前缀；
//! 2. 命中要求等级**严格相等** —— L3 批过的键在 L2 不得继续放行。

use super::*;
use crate::cm_agent::permission::project_caps;
use crate::cm_types::PermissionLevel;
use super::tests::{
    approval_fixture, evaluate_for_test, parsed_run_command, test_policy_context,
    test_policy_context_with_caps,
};

/// 命令轴：L3 批过的命令，在 L2 必须落到硬拒（而不是先被旧键放行）。
#[tokio::test]
async fn allow_always_key_does_not_cross_permission_levels() {
    use CommandApprovalDecision::AllowAlways;

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let cmd = "crabmate_level_scoped_cmd";
    let parsed = parsed_run_command(&format!(r#"{{"command":"{cmd}","args":[]}}"#));

    let fixture = approval_fixture(AllowAlways);
    let l3 = project_caps(PermissionLevel::Approved, &cfg);
    let ctx = test_policy_context_with_caps(&cfg, &l3, Some(&fixture.web), ExecMode::Sync);
    evaluate_for_test(&parsed, &ctx).await.expect("L3 批准");

    let stored = fixture
        .web
        .persistent_allowlist_shared
        .lock()
        .await
        .clone();
    assert_eq!(
        stored,
        std::collections::HashSet::from([crate::cm_types::scoped_allowlist_key(
            Some(PermissionLevel::Approved),
            cmd
        )]),
        "键必须带 l3| 前缀，且不得留裸键"
    );

    // L2：同一集合、同一条命令 → 旧键不命中，落到硬拒（不弹审批、也不挂起）。
    let l2 = project_caps(PermissionLevel::Workspace, &cfg);
    let ctx = test_policy_context_with_caps(&cfg, &l2, Some(&fixture.web), ExecMode::Sync);
    let (msg, _) = evaluate_for_test(&parsed, &ctx)
        .await
        .expect_err("L2 不得继承 L3 的 allowlist 键")
        .into_message();
    assert!(msg.contains("不在当前权限等级"), "{msg}");
}

/// 未接线等级的路径（`permission = None`）仍写裸键：与引入等级前逐字一致。
#[tokio::test]
async fn allow_always_key_stays_bare_without_caps() {
    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.command_exec.allowed_commands = vec!["cargo".to_string()].into();
    let cfg = Arc::new(cfg);
    let cmd = "crabmate_bare_key_cmd";
    let parsed = parsed_run_command(&format!(r#"{{"command":"{cmd}","args":[]}}"#));

    let fixture = approval_fixture(CommandApprovalDecision::AllowAlways);
    let ctx = test_policy_context(&cfg, Some(&fixture.web), ExecMode::Sync);
    evaluate_for_test(&parsed, &ctx).await.expect("批准");

    assert!(
        fixture
            .web
            .persistent_allowlist_shared
            .lock()
            .await
            .contains(cmd),
        "无 caps 时必须写裸键"
    );
}
