//! 工作区内 **`cargo metadata`** 子进程构造的单一真源。
//!
//! 供 `cargo_metadata` 工具、首轮项目画像 / 依赖摘要、`license_notice` 等共用，避免各处参数分叉。

use std::path::Path;
use std::process::Command;

/// 构建在 `workspace_root` 下执行的 `cargo metadata` 命令（已设置 `current_dir`，尚未 `spawn` / `output`）。
///
/// - `no_deps`：为 true 时附加 **`--no-deps`**（与工具默认一致）。
/// - `format_version`：写入 **`--format-version=<n>`**（与工具 JSON 参数默认 **1** 一致）。
pub fn cargo_metadata_command(
    workspace_root: &Path,
    no_deps: bool,
    format_version: u64,
) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.arg("metadata")
        .arg(format!("--format-version={format_version}"));
    if no_deps {
        cmd.arg("--no-deps");
    }
    cmd.current_dir(workspace_root);
    cmd
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn cargo_metadata_command_argv_matches_tool_defaults() {
        let root = Path::new("/tmp/workspace");
        let cmd = cargo_metadata_command(root, true, 1);
        assert_eq!(cmd.get_program(), OsStr::new("cargo"));
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![
                OsStr::new("metadata"),
                OsStr::new("--format-version=1"),
                OsStr::new("--no-deps"),
            ]
        );
        assert_eq!(cmd.get_current_dir(), Some(root));
    }

    #[test]
    fn cargo_metadata_command_without_no_deps() {
        let root = Path::new("/ws");
        let cmd = cargo_metadata_command(root, false, 1);
        let args: Vec<_> = cmd.get_args().collect();
        assert_eq!(
            args,
            vec![OsStr::new("metadata"), OsStr::new("--format-version=1"),]
        );
    }
}
