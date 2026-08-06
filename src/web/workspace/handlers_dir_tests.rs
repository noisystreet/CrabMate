//! 工作区目录创建同步路径单元测试。

#[cfg(test)]
mod workspace_dir_create_tests {
    use super::super::handlers_sync::workspace_dir_create_sync;
    use std::path::PathBuf;

    fn workspace_base(root: &tempfile::TempDir) -> PathBuf {
        root.path().canonicalize().expect("canonical root")
    }

    #[test]
    fn create_dir_at_root_succeeds() {
        let root = tempfile::tempdir().expect("tempdir");
        let base = workspace_base(&root);
        let target = base.join("new_dir");
        workspace_dir_create_sync(base, target.clone(), false).expect("create");
        assert!(target.is_dir());
    }

    #[test]
    fn create_dir_with_parents_creates_nested() {
        let root = tempfile::tempdir().expect("tempdir");
        let base = workspace_base(&root);
        let target = base.join("a/b/c");
        workspace_dir_create_sync(base, target.clone(), true).expect("create nested");
        assert!(target.is_dir());
    }

    #[test]
    fn create_dir_without_parents_fails_when_parent_missing() {
        let root = tempfile::tempdir().expect("tempdir");
        let base = workspace_base(&root);
        let target = base.join("missing_parent/child");
        let err = workspace_dir_create_sync(base, target, false).expect_err("should fail");
        assert!(err.contains("创建目录失败"), "{err}");
    }

    #[test]
    fn create_dir_rejects_existing_file() {
        let root = tempfile::tempdir().expect("tempdir");
        let base = workspace_base(&root);
        let target = base.join("blocker");
        std::fs::write(&target, b"x").expect("write file");
        let err = workspace_dir_create_sync(base, PathBuf::from(&target), false)
            .expect_err("file blocks dir");
        assert!(err.contains("文件"), "{err}");
    }
}
