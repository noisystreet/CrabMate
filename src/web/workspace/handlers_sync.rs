//! 工作区文件/目录同步路径（Unix `openat2` 等）；从 `handlers.rs` 拆出以控制文件行数与 CCN。

#[cfg(unix)]
use crate::web::http_types::workspace::WorkspaceEntry;
#[cfg(unix)]
use crate::workspace::fs::{
    open_directory_under_root, open_existing_file_under_root, open_file_write_under_root,
    unlink_file_under_root,
};
#[cfg(unix)]
use libc;
#[cfg(unix)]
use nix::dir::Type;
#[cfg(unix)]
use nix::fcntl::AtFlags;
#[cfg(unix)]
use nix::sys::stat::fstatat;

#[cfg(unix)]
pub(crate) fn workspace_list_entries_sync(
    base: std::path::PathBuf,
    can: std::path::PathBuf,
) -> Result<Vec<WorkspaceEntry>, String> {
    let (mut dir, _) =
        open_directory_under_root(&base, &can).map_err(|e| format!("无法读取工作目录: {e}"))?;
    let mut names: Vec<String> = Vec::new();
    let mut types_hint: Vec<Option<Type>> = Vec::new();
    for ent in dir.iter() {
        let ent = ent.map_err(|e| format!("读取目录项失败: {e}"))?;
        let name_c = ent.file_name();
        let nb = name_c.to_bytes();
        if nb == b"." || nb == b".." {
            continue;
        }
        names.push(String::from_utf8_lossy(nb).to_string());
        types_hint.push(ent.file_type());
    }
    let mut entries = Vec::new();
    for (name, hint) in names.into_iter().zip(types_hint) {
        let is_dir = match hint {
            Some(Type::Directory) => true,
            Some(Type::Symlink) | None => {
                let st = fstatat(&dir, name.as_str(), AtFlags::AT_SYMLINK_NOFOLLOW)
                    .map_err(|e| format!("读取目录项失败: {e}"))?;
                (st.st_mode & libc::S_IFMT) == libc::S_IFDIR
            }
            _ => false,
        };
        entries.push(WorkspaceEntry { name, is_dir });
    }
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
    Ok(entries)
}

#[cfg(unix)]
pub(crate) fn workspace_read_file_sync_unix(
    base: std::path::PathBuf,
    can: std::path::PathBuf,
    enc_name: crate::text_encoding::TextEncodingName,
    max_b: u64,
) -> Result<String, String> {
    use std::io::Read;
    let opened =
        open_existing_file_under_root(&base, &can).map_err(|e| format!("无法读取文件信息: {e}"))?;
    if opened.metadata.is_dir() {
        return Err("路径是目录，无法读取为文件".to_string());
    }
    let len = opened.metadata.len();
    if len > max_b {
        return Err(format!(
            "文件过大（{} 字节），当前最多读取 {} 字节",
            len, max_b
        ));
    }
    let mut f = opened.file;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw)
        .map_err(|e| format!("读取文件失败: {e}"))?;
    crate::text_encoding::decode_bytes_strict(&raw, enc_name).map(|(s, _)| s)
}

#[cfg(unix)]
pub(crate) fn workspace_delete_file_sync_unix(
    base: std::path::PathBuf,
    can: std::path::PathBuf,
) -> Result<(), String> {
    let opened =
        open_existing_file_under_root(&base, &can).map_err(|e| format!("无法读取文件信息: {e}"))?;
    if opened.metadata.is_dir() {
        return Err("不支持删除目录".to_string());
    }
    unlink_file_under_root(&base, &can).map_err(|e| format!("删除文件失败: {e}"))
}

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
