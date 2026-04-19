//! `run_command` 工作区脚本/可执行文件自动审批辅助模块。

use crate::path_workspace::{
    WorkspacePathError, absolutize_relative_under_root, canonical_workspace_root,
    ensure_existing_ancestor_within_root,
};
use std::path::{Path, PathBuf};

/// `run_command` 的 `command` 若为「明显相对路径」（`./…` 或含 `/`），且解析到工作区内**已存在**的普通文件，
/// 且为可执行位（Unix）或常见脚本扩展名，则视为工作区脚本/可执行文件：**跳过**白名单外人工审批（与「本次允许」
/// 等同，仅扩展当次有效允许表，不写入永久允许集合）。
///
/// 不包含裸命令名（如 `gcc`），避免与工作区内同名文件或 PATH 解析混淆。
pub(crate) fn run_command_invocation_targets_workspace_script_or_executable(
    working_dir: &Path,
    command_raw: &str,
) -> bool {
    let t = command_raw.trim();
    if t.is_empty() || t.contains("..") {
        return false;
    }
    if !t.starts_with("./") && !t.contains('/') {
        return false;
    }
    let target = match resolve_executable_path(working_dir, t) {
        Ok(p) => p,
        Err(_) => return false,
    };
    if !target.is_file() {
        return false;
    }
    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(_) => return false,
    };
    if is_executable(&meta) {
        return true;
    }
    target
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            matches!(
                e.to_ascii_lowercase().as_str(),
                "sh" | "bash"
                    | "zsh"
                    | "py"
                    | "pl"
                    | "rb"
                    | "js"
                    | "mjs"
                    | "cjs"
                    | "ts"
                    | "ps1"
                    | "fish"
                    | "ksh"
                    | "dash"
            )
        })
}

/// 解析 `./xxx` 形式的工作区可执行文件路径。
/// 若命令是 `./xxx` 或含 `/` 的相对路径，且解析后为已存在的可执行文件或脚本，返回路径。
/// 否则返回 Err。
pub fn resolve_workspace_executable(
    working_dir: &Path,
    command_raw: &str,
) -> Result<PathBuf, WorkspacePathError> {
    let t = command_raw.trim();
    if t.is_empty() || t.contains("..") {
        return Err(WorkspacePathError::EmptyPath);
    }
    if !t.starts_with("./") && !t.contains('/') {
        return Err(WorkspacePathError::EmptyPath);
    }
    let target = resolve_executable_path(working_dir, t)?;
    if !target.is_file() {
        return Err(WorkspacePathError::EmptyPath);
    }
    let meta = std::fs::metadata(&target).map_err(|_| WorkspacePathError::EmptyPath)?;
    if !is_executable(&meta)
        && !target
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| {
                matches!(
                    e.to_ascii_lowercase().as_str(),
                    "sh" | "bash"
                        | "zsh"
                        | "py"
                        | "pl"
                        | "rb"
                        | "js"
                        | "mjs"
                        | "cjs"
                        | "ts"
                        | "ps1"
                        | "fish"
                        | "ksh"
                        | "dash"
                )
            })
    {
        return Err(WorkspacePathError::EmptyPath);
    }
    Ok(target)
}

/// 解析相对工作目录的路径，且不允许超出工作目录
fn resolve_executable_path(base: &Path, sub: &str) -> Result<PathBuf, WorkspacePathError> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err(WorkspacePathError::EmptyPath);
    }
    if Path::new(sub).is_absolute() {
        return Err(WorkspacePathError::AbsolutePathNotAllowed);
    }
    let base_canonical = canonical_workspace_root(base)?;
    let normalized = absolutize_relative_under_root(&base_canonical, sub)?;
    ensure_existing_ancestor_within_root(&base_canonical, &normalized)?;
    Ok(normalized)
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.is_file() && (meta.permissions().mode() & 0o111) != 0
}

#[cfg(not(unix))]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    meta.is_file()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn make_test_dir() -> PathBuf {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "crabmate_exec_tool_test_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[cfg(unix)]
    #[test]
    fn test_resolve_executable_path_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = make_test_dir();
        let outside = std::env::temp_dir().join(format!(
            "crabmate_exec_outside_{}_{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&outside).unwrap();
        let link = dir.join("bin");
        symlink(&outside, &link).unwrap();

        let res = resolve_executable_path(&dir, "bin/tool.sh");
        assert!(res.is_err(), "应拒绝 symlink 绕过执行路径");
        let msg = res.err().map(|e| e.to_string()).unwrap_or_default();
        assert!(
            msg.contains("路径不能超出工作目录"),
            "报错应提示越界: {msg}"
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }

    #[cfg(unix)]
    #[test]
    fn run_command_workspace_auto_approve_requires_path_shape() {
        let dir = make_test_dir();
        let script = dir.join("helper.sh");
        std::fs::write(&script, "#!/bin/sh\necho ok\n").unwrap();
        assert!(
            run_command_invocation_targets_workspace_script_or_executable(&dir, "./helper.sh"),
            ".sh under ./ should qualify"
        );
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let nested = dir.join("bin/wrap.sh");
        std::fs::write(&nested, "#!/bin/sh\n").unwrap();
        assert!(
            run_command_invocation_targets_workspace_script_or_executable(&dir, "bin/wrap.sh"),
            "subdir/ path should qualify"
        );
        assert!(
            !run_command_invocation_targets_workspace_script_or_executable(&dir, "ls"),
            "bare command name must not auto-approve"
        );
        assert!(
            !run_command_invocation_targets_workspace_script_or_executable(&dir, "./nope.sh"),
            "missing file must not auto-approve"
        );
        assert!(
            !run_command_invocation_targets_workspace_script_or_executable(&dir, "../x"),
            "path with .. rejected"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn run_command_workspace_auto_approve_executable_bit() {
        use std::os::unix::fs::PermissionsExt;

        let dir = make_test_dir();
        let bin = dir.join("mytool");
        std::fs::write(&bin, "#!/bin/sh\necho tool\n").unwrap();
        let mut perms = std::fs::metadata(&bin).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&bin, perms).unwrap();
        assert!(run_command_invocation_targets_workspace_script_or_executable(&dir, "./mytool"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
