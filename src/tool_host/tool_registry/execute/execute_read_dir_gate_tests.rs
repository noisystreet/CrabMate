//! `read_dir` 外部路径审批在 S7 `AllowedRoots` 档下的直接测试。
//!
//! 拆出成独立文件：`execute_tests.rs` 已顶到文件行数棘轮（≤ 920 行），
//! 与 `execute_run_command_gate_tests.rs` 同模式。

use super::dispatch::approve_external_read_dir_if_needed;
use crate::cm_agent::permission::{PolicyEngine, project_caps};
use crate::cm_types::{PathScope, PermissionLevel};
use std::sync::Arc;

/// 五态矩阵：L2 根内放行 / L2 根外硬拒 / 预审批无上下文不定论 / L3 根内放行 / L3 根外要通道。
///
/// 钉住两条最容易回归的语义：
/// 1. 根内目标**放行且不弹窗**（L2 硬边界档与 L3 审批档都一样，且不记 `allowed`）；
/// 2. **拿不到根集合的调用点（并行批预审批）不定论**——不能把「本来在根内」的目标在 L2 下
///    误判成硬拒。这是回归点：`scope_ctx = None` 必须 `Ok`，不能落到 `Deny` 分支。
#[tokio::test]
async fn read_dir_allowed_roots_matrix() {
    let base = std::env::temp_dir().join(format!("crabmate_read_dir_s7_{}", std::process::id()));
    std::fs::remove_dir_all(&base).ok();
    std::fs::create_dir_all(base.join("workspace")).expect("workspace");
    std::fs::create_dir_all(base.join("sibling")).expect("sibling");
    let base = base.canonicalize().expect("canonicalize");
    let ws = base.join("workspace");
    let roots = vec![base.clone()];

    let mut cfg = crate::cm_config::load_config(None).expect("embed default");
    cfg.workspace_roots.workspace_allowed_roots = roots.clone();
    let cfg = Arc::new(cfg);

    let inside = r#"{"path":"../sibling"}"#;
    let outside = format!(r#"{{"path":"{}"}}"#, base.join("nowhere").display());

    // L2（workspace ⇒ approval = Deny）
    let l2_caps = project_caps(PermissionLevel::Workspace, cfg.as_ref());
    assert_eq!(
        l2_caps.path_scope,
        PathScope::AllowedRoots,
        "非空根下 L2 应是多根档"
    );
    let l2: &dyn PolicyEngine = &l2_caps;

    approve_external_read_dir_if_needed(
        inside,
        None,
        Some(l2),
        Some((ws.as_path(), roots.as_slice())),
    )
    .await
    .expect("L2 下允许根内应放行（不弹窗）");

    let msg = approve_external_read_dir_if_needed(
        &outside,
        None,
        Some(l2),
        Some((ws.as_path(), roots.as_slice())),
    )
    .await
    .expect_err("L2 下越界必须硬拒")
    .to_string();
    assert!(msg.contains("不允许 read_dir"), "{msg}");

    // 预审批层拿不到根集合：不定论、不阻断（否则 L2 会把根内目标误判硬拒）。
    approve_external_read_dir_if_needed(inside, None, Some(l2), None)
        .await
        .expect("无上下文时不得在 L2 下误判硬拒");

    // L3（approved ⇒ approval = Gate）：根内仍放行，且不需要审批通道。
    let l3_caps = project_caps(PermissionLevel::Approved, cfg.as_ref());
    let l3: &dyn PolicyEngine = &l3_caps;
    approve_external_read_dir_if_needed(
        inside,
        None,
        Some(l3),
        Some((ws.as_path(), roots.as_slice())),
    )
    .await
    .expect("L3 下允许根内应放行，不需要审批通道");

    let msg = approve_external_read_dir_if_needed(
        &outside,
        None,
        Some(l3),
        Some((ws.as_path(), roots.as_slice())),
    )
    .await
    .expect_err("L3 下越界需要审批通道")
    .to_string();
    assert!(msg.contains("需要审批通道"), "{msg}");

    std::fs::remove_dir_all(&base).ok();
}
