//! L4 路径放行 × 不可逆操作确认门（设计 §9 MB-4、§13「L4 × MB-4 整链」）。
//!
//! **为什么单独一层**：`mutate_delete_tests` / `tests_typed_dir_perm` 用默认
//! `PathAccess::new`（`PathScope::SessionRoot`）测确认门。根外目标在那两档下
//! 「路径越界」与「缺 `confirm`」会**同时**成立，断言无法区分是哪一层拦下的——
//! 于是「有人把 `confirm` 并进 `approval = audit_only` 分支」这种回归不会被发现。
//!
//! 本文件改用 `PathScope::Any`（`unrestricted` 的路径轴；它只由
//! `project_caps(Unrestricted)` 产生，见 `cm_types::permission_audit::tests::
//! path_scope_any_exists_only_at_unrestricted`）先把路径轴放开，再断言确认门
//! **没有**跟着放宽。`l2_*` 用例是判别式对照：证明「L4 能删根外」确实来自 `Any`，
//! 而不是 `delete_dir` 本就不看根。
//!
//! 本层不引用 `cm_agent::permission::project_caps`：`cm_tools` 是叶子层，反向依赖
//! 会破坏分层（同 `path.rs` 的 `audit_unrestricted_path` 注释）。

use super::test_support::{make_test_dir, test_ctx, write};
use super::{PathAccess, delete_dir_try, delete_files_try};
use crate::cm_types::PathScope;
use std::path::Path;

/// 建 `<tmp>/ws`（工作区根）与返回值；调用方自行准备根外目标。
fn workspace(td: &Path) -> std::path::PathBuf {
    let ws = td.join("ws");
    std::fs::create_dir_all(&ws).expect("create workspace root");
    ws
}

/// L4 的路径载体（等价于 `project_caps(Unrestricted)` 的 `path_scope`）。
fn unrestricted_access(ws: &Path) -> PathAccess<'_> {
    PathAccess::with_scope(ws, PathScope::Any)
}

/// L4 下 `delete_dir` 仍要求 `confirm`；缺参时目标必须原样留存。
#[test]
fn l4_delete_dir_still_requires_confirm_outside_workspace() {
    let td = make_test_dir();
    let ws = workspace(&td);
    let victim = td.join("outside/victim");
    std::fs::create_dir_all(&victim).expect("create outside dir");

    let access = unrestricted_access(&ws);
    let err = delete_dir_try(r#"{"path":"../outside/victim"}"#, &access)
        .expect_err("L4 不得免除不可逆操作确认");
    assert!(err.message.contains("confirm=true"), "{}", err.message);
    assert!(victim.is_dir(), "缺 confirm 时不得删除目录");

    let _ = std::fs::remove_dir_all(&td);
}

/// 同一 L4 载体、补上 `confirm` 后根外目录真的被删——证明上一条的拒绝来自确认门，
/// 而不是「路径根本走不通」。
#[test]
fn l4_delete_dir_removes_outside_workspace_with_confirm() {
    let td = make_test_dir();
    let ws = workspace(&td);
    let victim = td.join("outside/victim");
    std::fs::create_dir_all(&victim).expect("create outside dir");

    let access = unrestricted_access(&ws);
    let out = delete_dir_try(r#"{"path":"../outside/victim","confirm":true}"#, &access)
        .expect("L4 应放行工作区外路径");
    assert!(out.contains("已删除目录"), "{out}");
    assert!(!victim.exists(), "已确认时必须真的删除");

    let _ = std::fs::remove_dir_all(&td);
}

/// 判别式对照：同一条输入在 L2（`SessionRoot`）下仍被硬拒，即便带了 `confirm`。
#[test]
fn l2_delete_dir_rejects_outside_workspace_even_with_confirm() {
    let td = make_test_dir();
    let ws = workspace(&td);
    let victim = td.join("outside/victim");
    std::fs::create_dir_all(&victim).expect("create outside dir");

    let access = PathAccess::new(&ws);
    let err = delete_dir_try(r#"{"path":"../outside/victim","confirm":true}"#, &access)
        .expect_err("L2 是工作区硬边界");
    assert!(!err.message.contains("confirm"), "{}", err.message);
    assert!(victim.is_dir(), "被拒时不得删除目录");

    let _ = std::fs::remove_dir_all(&td);
}

/// L4 下 `delete_files` 仍要求 `confirm`：先证路径轴已放行（`for_read` 成功），
/// 再证缺参被确认门拦下。
///
/// 这里**不**断言「补上 `confirm` 后删除成功」：`delete_files` 的删除阶段走
/// `unlink_file_under_root`，仍以工作区根为锚（`mutate::delete_file_remove`），
/// L4 下根外文件会在**删除阶段**失败。那是与本条正交的另一个缺口（未在
/// `docs/design/permission_levels.md` 记录），不作为期望行为钉进本测试。
#[test]
fn l4_delete_files_still_requires_confirm_outside_workspace() {
    let td = make_test_dir();
    let ws = workspace(&td);
    write(&td, "outside/a.txt", "A");

    let access = unrestricted_access(&ws);
    access
        .for_read("../outside/a.txt")
        .expect("L4 的路径轴应放行工作区外目标");

    let ctx = test_ctx(&ws);
    let err = delete_files_try(r#"{"paths":["../outside/a.txt"]}"#, &access, &ctx)
        .expect_err("L4 不得免除不可逆操作确认");
    assert!(err.message.contains("需要 confirm=true"), "{}", err.message);
    assert!(td.join("outside/a.txt").is_file(), "缺 confirm 时不得删除文件");

    let _ = std::fs::remove_dir_all(&td);
}
