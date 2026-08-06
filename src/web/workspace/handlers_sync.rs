//! 工作区文件/目录同步写路径（Unix `openat2`）；从 `handlers.rs` 拆出以控制文件行数。

#[cfg(unix)]
use crate::workspace::fs::open_file_write_under_root;

#[cfg(unix)]
pub(crate) fn workspace_file_write_sync_unix(
    base: std::path::PathBuf,
    normalized: std::path::PathBuf,
    content: String,
    create_only: bool,
    update_only: bool,
) -> Result<(), String> {
    use std::io::{ErrorKind, Write};
    if let Some(parent) = normalized.parent()
        && !parent.as_os_str().is_empty()
    {
        crate::workspace::fs::ensure_parent_dirs_under_root(&base, &normalized)
            .map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let mut f = match open_file_write_under_root(&base, &normalized, create_only, update_only) {
        Ok(f) => f,
        Err(e) if create_only && e.kind() == ErrorKind::AlreadyExists => {
            return Err("文件已存在，无法仅创建".to_string());
        }
        Err(e) if update_only && e.kind() == ErrorKind::NotFound => {
            return Err("文件不存在，无法仅修改".to_string());
        }
        Err(e) => {
            return Err(format!("打开文件失败: {e}"));
        }
    };
    f.write_all(content.as_bytes())
        .map_err(|e| format!("写入文件失败: {e}"))
}

pub(crate) fn workspace_dir_create_sync(
    base_canonical: std::path::PathBuf,
    canonical: std::path::PathBuf,
    parents: bool,
) -> Result<(), String> {
    if canonical.exists() {
        if canonical.is_dir() {
            return Err("目录已存在".to_string());
        }
        return Err("路径已存在且为文件".to_string());
    }
    crate::workspace::fs::create_directory_under_root(&base_canonical, &canonical, parents)
        .map_err(|e| format!("创建目录失败: {e}"))
}
