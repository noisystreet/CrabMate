//! S7 [`PathScope::AllowedRoots`] 的路径解析单元测试。
//!
//! 独立成文件而不是塞进 `path.rs` 的 `mod tests`：`path.rs` 已接近文件行数棘轮，
//! 且这一批用例共用同一套「父目录 / 工作区 / 兄弟仓 / 根外」脚手架。

use super::path::PathAccess;
use crate::cm_types::PathScope;

/// 临时目录脚手架：`base/{workspace,sibling,outside}`。
///
/// 布局刻意还原真实用法——工作区是 `base/workspace`，允许根写 `base`，
/// 于是 `../sibling/...` 既「含 `..`」又「落在允许根内」，正好是本 issue 的主用例。
struct Fixture {
    base: std::path::PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let base = std::env::temp_dir().join(format!(
            "crabmate_s7_allowed_roots_{tag}_{}",
            std::process::id()
        ));
        std::fs::remove_dir_all(&base).ok();
        for dir in ["workspace", "sibling", "outside"] {
            std::fs::create_dir_all(base.join(dir)).expect("create fixture dir");
        }
        let base = base.canonicalize().expect("canonicalize fixture base");
        Self { base }
    }

    fn workspace(&self) -> std::path::PathBuf {
        self.base.join("workspace")
    }

    fn sibling(&self) -> std::path::PathBuf {
        self.base.join("sibling")
    }

    fn write(&self, rel: &str, body: &[u8]) {
        std::fs::write(self.base.join(rel), body).expect("write fixture file");
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.base).ok();
    }
}

/// 主用例：`../sibling/x.txt` 含 `..`，但落在允许根内 ⇒ 放行。
#[test]
fn reads_sibling_repo_under_allowed_root() {
    let f = Fixture::new("read");
    f.write("sibling/x.txt", b"hi");
    let ws = f.workspace();
    let roots = vec![f.base.clone()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let got = access.for_read("../sibling/x.txt").expect("root 内应放行");
    assert_eq!(got, f.sibling().join("x.txt"));

    // 同一个 `PathAccess` 上，会话根内的相对路径结论不变。
    f.write("workspace/a.txt", b"x");
    assert_eq!(
        access.for_read("a.txt").expect("会话根内"),
        ws.join("a.txt")
    );
}

/// 绝对路径落在允许根内 ⇒ 读放行。
#[test]
fn reads_absolute_path_under_allowed_root() {
    let f = Fixture::new("abs");
    f.write("sibling/y.txt", b"hi");
    let ws = f.workspace();
    let roots = vec![f.base.clone()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let abs = f.sibling().join("y.txt");
    let got = access
        .for_read(abs.to_str().expect("utf8"))
        .expect("根内绝对路径应放行");
    assert_eq!(got, abs);
}

/// 越出**所有**根 ⇒ 策略拒绝（不是 `invalid_args`）。
#[test]
fn rejects_path_outside_every_root() {
    let f = Fixture::new("reject");
    f.write("outside/secret.txt", b"no");
    let ws = f.workspace();
    // 白名单只到兄弟仓：`outside/` 不在其中。
    let roots = vec![f.sibling()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let err = access
        .for_read("../outside/secret.txt")
        .expect_err("越界必须拒");
    assert!(err.is_policy_denied(), "应归策略拒绝: {err}");
    assert_eq!(err.kind(), "outside_allowed_roots");
}

/// 写路径：目标可不存在，父级在允许根内即放行。
#[test]
fn writes_missing_target_under_allowed_root() {
    let f = Fixture::new("write");
    let ws = f.workspace();
    let roots = vec![f.base.clone()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let target = access
        .for_write("../sibling/new/deep.txt")
        .expect("根内新目标应放行");
    assert!(target.ends_with("sibling/new/deep.txt"), "{target:?}");
    assert!(!target.exists(), "写解析不应真的创建文件");
}

/// 允许根**内**的 symlink 指向根外 ⇒ 仍拒（越界按 canonical 落点判，不按字面前缀）。
#[test]
fn symlink_escape_inside_allowed_root_still_rejected() {
    let f = Fixture::new("symlink");
    f.write("outside/secret.txt", b"no");
    std::os::unix::fs::symlink(f.base.join("outside"), f.sibling().join("escape"))
        .expect("create symlink");
    let ws = f.workspace();
    let roots = vec![f.sibling()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let err = access
        .for_read("../sibling/escape/secret.txt")
        .expect_err("symlink 逃逸必须拒");
    assert!(err.is_policy_denied(), "应归策略拒绝: {err}");
}

/// 空根 ⇒ fail-closed 退化为 `SessionRoot`（与 Web 选址侧「空表 = 任意」故意不同）。
#[test]
fn empty_allowed_roots_degrades_to_session_root() {
    let f = Fixture::new("empty");
    f.write("workspace/a.txt", b"x");
    f.write("sibling/b.txt", b"x");
    let ws = f.workspace();
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &[]);
    let session = PathAccess::new(&ws);

    for rel in ["a.txt", "../sibling/b.txt"] {
        assert_eq!(
            access.for_read(rel).is_ok(),
            session.for_read(rel).is_ok(),
            "空根下 `{rel}` 的结论必须与 SessionRoot 一致"
        );
    }
    assert!(access.for_read("a.txt").is_ok(), "会话根内仍可读");
    assert!(
        access.for_read("../sibling/b.txt").is_err(),
        "空根不得放宽到兄弟仓"
    );
}

/// `AllowedRoots` **不**写 `allowed` 审计：`allowed` 只由 L4 的 `AuditOnly` 记（设计 §6.3）。
#[test]
fn allowed_roots_emits_no_allowed_audit() {
    use crate::cm_types::permission_audit::test_support::captured_with;

    let f = Fixture::new("audit");
    f.write("sibling/x.txt", b"hi");
    let ws = f.workspace();
    let roots = vec![f.base.clone()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    let captured = captured_with(|| {
        let _ = access.for_read("../sibling/x.txt");
    });
    assert!(
        captured.is_empty(),
        "AllowedRoots 不该产生 allowed 审计: {captured}"
    );
}

/// `is_within_scope`（`read_dir` 外部路径审批的判据）与根集合同源。
#[test]
fn is_within_scope_matches_root_set() {
    let f = Fixture::new("scope");
    f.write("workspace/a.txt", b"x");
    f.write("sibling/x.txt", b"hi");
    let ws = f.workspace();
    let roots = vec![f.sibling()];
    let access = PathAccess::with_scope_and_roots(&ws, PathScope::AllowedRoots, &roots);

    assert!(access.is_within_scope("a.txt"), "会话根内");
    assert!(access.is_within_scope("../sibling/x.txt"), "允许根内");
    assert!(!access.is_within_scope("../outside"), "根外");
    assert!(
        !access.is_within_scope("../sibling/missing.txt"),
        "目标不存在 ⇒ 不认"
    );

    // L4 恒真（`Any` 不再判落点）。
    let any = PathAccess::with_scope(&ws, PathScope::Any);
    assert!(any.is_within_scope("/etc"));
}
