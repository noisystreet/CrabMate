//! 工作区文件/目录同步路径（Unix `openat2` 等）；从 `handlers.rs` 拆出以控制文件行数与 CCN。

#[cfg(unix)]
use crate::cm_api_contract::workspace::WorkspaceEntry;
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

/// 工作区文件读/写的**结构化**失败原因（#1478 B-β）。
///
/// 这些信息在底层本来是结构化的（`io::ErrorKind` / 元数据），此前被 `format!` 成中文串，
/// handler 再用 `contains("过大")` / `contains("已存在")` 猜回 HTTP 状态码。现在类型一路保留到
/// handler，`Display` 只负责用户可见文案（逐字保持原样）。
#[cfg(unix)]
#[derive(Debug)]
pub(crate) enum WorkspaceFileIoError {
    /// 目标是目录，不能当文件读。
    IsDirectory,
    /// 超过读取上限。
    TooLarge { len: u64, max_bytes: u64 },
    /// `create_only` 撞上已存在。
    AlreadyExists,
    /// `update_only` 撞上不存在。
    NotFound,
    /// 文本解码失败（`text_encoding` 尚未 typed 化，见 #1478 档 1）。
    Decode(String),
    /// 其它失败；`context` 是原有中文前缀，`detail` 只用于展示、**不参与判定**。
    Io {
        context: &'static str,
        detail: String,
    },
}

#[cfg(unix)]
impl std::fmt::Display for WorkspaceFileIoError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::IsDirectory => f.write_str("路径是目录，无法读取为文件"),
            Self::TooLarge { len, max_bytes } => {
                write!(f, "文件过大（{len} 字节），当前最多读取 {max_bytes} 字节")
            }
            Self::AlreadyExists => f.write_str("文件已存在，无法仅创建"),
            Self::NotFound => f.write_str("文件不存在，无法仅修改"),
            Self::Decode(msg) => f.write_str(msg),
            Self::Io { context, detail } => write!(f, "{context}: {detail}"),
        }
    }
}

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
) -> Result<String, WorkspaceFileIoError> {
    use std::io::Read;
    let opened = open_existing_file_under_root(&base, &can).map_err(|e| WorkspaceFileIoError::Io {
        context: "无法读取文件信息",
        detail: e.to_string(),
    })?;
    if opened.metadata.is_dir() {
        return Err(WorkspaceFileIoError::IsDirectory);
    }
    let len = opened.metadata.len();
    if len > max_b {
        return Err(WorkspaceFileIoError::TooLarge { len, max_bytes: max_b });
    }
    let mut f = opened.file;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw).map_err(|e| WorkspaceFileIoError::Io {
        context: "读取文件失败",
        detail: e.to_string(),
    })?;
    crate::text_encoding::decode_bytes_strict(&raw, enc_name)
        .map(|(s, _)| s)
        .map_err(WorkspaceFileIoError::Decode)
}

#[cfg(unix)]
pub(crate) fn workspace_read_file_bytes_sync_unix(
    base: std::path::PathBuf,
    can: std::path::PathBuf,
    max_b: u64,
) -> Result<Vec<u8>, WorkspaceFileIoError> {
    use std::io::Read;
    let opened = open_existing_file_under_root(&base, &can).map_err(|e| WorkspaceFileIoError::Io {
        context: "无法读取文件信息",
        detail: e.to_string(),
    })?;
    if opened.metadata.is_dir() {
        return Err(WorkspaceFileIoError::IsDirectory);
    }
    let len = opened.metadata.len();
    if len > max_b {
        return Err(WorkspaceFileIoError::TooLarge { len, max_bytes: max_b });
    }
    let mut f = opened.file;
    let mut raw = Vec::new();
    f.read_to_end(&mut raw).map_err(|e| WorkspaceFileIoError::Io {
        context: "读取文件失败",
        detail: e.to_string(),
    })?;
    Ok(raw)
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
    content: Vec<u8>,
    create_only: bool,
    update_only: bool,
) -> Result<(), WorkspaceFileIoError> {
    use std::io::{ErrorKind, Write};
    if let Some(parent) = normalized.parent()
        && !parent.as_os_str().is_empty()
    {
        crate::workspace::fs::ensure_parent_dirs_under_root(&base, &normalized).map_err(|e| {
            WorkspaceFileIoError::Io {
                context: "创建目录失败",
                detail: e.to_string(),
            }
        })?;
    }
    let mut f = match open_file_write_under_root(&base, &normalized, create_only, update_only) {
        Ok(f) => f,
        Err(e) if create_only && e.kind() == ErrorKind::AlreadyExists => {
            return Err(WorkspaceFileIoError::AlreadyExists);
        }
        Err(e) if update_only && e.kind() == ErrorKind::NotFound => {
            return Err(WorkspaceFileIoError::NotFound);
        }
        Err(e) => {
            return Err(WorkspaceFileIoError::Io {
                context: "打开文件失败",
                detail: e.to_string(),
            });
        }
    };
    f.write_all(&content).map_err(|e| WorkspaceFileIoError::Io {
        context: "写入文件失败",
        detail: e.to_string(),
    })
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
