//! 工作区内路径的**打开**语义：在 Linux 上优先使用 **`openat2(2)`** + **`RESOLVE_IN_ROOT`**，将路径解析约束在已校验的工作区根目录 fd 下，缓解「先 `canonicalize` 再按路径字符串 `open`」的 **TOCTOU**。
//!
//! - **Linux**：相对路径在**根目录 fd** 上解析；**工作区内的符号链接仍可被跟随**，但解析不得越过该根（含绝对 symlink 目标）。
//! - **其它 Unix**：回退为对已 `canonicalize` 路径的单次 `std::fs` 打开（与历史行为一致）。
//! - **非 Unix**：不包含 Linux 专用依赖；由调用方使用 `std::fs`。

use std::fs::{File, Metadata, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};

/// `resolve_for_read_open` 的成功结果：已打开的文件句柄、用于缓存键等指标的路径、以及 **`fstat`** 元数据（与打开同一 inode，避免对路径二次 `open`）。
pub(crate) struct OpenedWorkspaceFile {
    pub file: File,
    /// 尽量为 **`/proc/self/fd/N` 的 `canonicalize` 结果**（Linux），否则为逻辑路径，供 `read_file` 缓存键与展示。
    pub resolved_path: PathBuf,
    pub metadata: Metadata,
}

#[cfg(target_os = "linux")]
fn canonical_path_via_proc_fd(file: &File) -> Option<PathBuf> {
    let fd = file.as_raw_fd();
    let proc_link = format!("/proc/self/fd/{fd}");
    std::fs::canonicalize(proc_link).ok()
}

#[cfg(not(target_os = "linux"))]
fn canonical_path_via_proc_fd(_file: &File) -> Option<PathBuf> {
    None
}

fn rel_under_root(root_canonical: &Path, logical: &Path) -> io::Result<PathBuf> {
    logical
        .strip_prefix(root_canonical)
        .map(|p| p.to_path_buf())
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "path outside workspace root",
            )
        })
}

/// 在已 **canonical** 的工作区根下打开只读文件：`logical` 为根下的**词法**绝对路径（通常来自 `join` + `canonicalize`），须已存在且为文件。
#[cfg(target_os = "linux")]
pub(crate) fn open_existing_file_under_root(
    root_canonical: &Path,
    logical: &Path,
) -> io::Result<OpenedWorkspaceFile> {
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
    use nix::sys::stat::Mode;

    let rel = rel_under_root(root_canonical, logical)?;
    // `logical == root_canonical`（如工具 path `.`）时相对分量为空，直接打开根目录 fd。
    if rel.as_os_str().is_empty() {
        let file = OpenOptions::new()
            .read(true)
            .open(root_canonical)
            .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;
        let metadata = file.metadata()?;
        let resolved_path =
            canonical_path_via_proc_fd(&file).unwrap_or_else(|| logical.to_path_buf());
        return Ok(OpenedWorkspaceFile {
            file,
            resolved_path,
            metadata,
        });
    }

    let root = OpenOptions::new()
        .read(true)
        .open(root_canonical)
        .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;

    let how = OpenHow::new()
        .flags(OFlag::O_RDONLY | OFlag::O_CLOEXEC)
        .mode(Mode::empty())
        .resolve(ResolveFlag::RESOLVE_IN_ROOT);

    let owned = openat2(&root, rel.as_path(), how).map_err(io::Error::from)?;

    // SAFETY: `openat2` 成功返回的新建 fd，所有权移交给 `File`。
    let file = unsafe { File::from_raw_fd(owned.into_raw_fd()) };
    let metadata = file.metadata()?;
    let resolved_path = canonical_path_via_proc_fd(&file).unwrap_or_else(|| logical.to_path_buf());

    Ok(OpenedWorkspaceFile {
        file,
        resolved_path,
        metadata,
    })
}

/// 在已 **canonical** 的工作区根下打开目录（`O_DIRECTORY`），供 Web 列表等使用。
#[cfg(target_os = "linux")]
pub(crate) fn open_directory_under_root(
    root_canonical: &Path,
    logical: &Path,
) -> io::Result<(nix::dir::Dir, PathBuf)> {
    use nix::dir::Dir;
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
    use nix::sys::stat::Mode;

    if logical == root_canonical {
        let d = Dir::open(
            root_canonical,
            OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
            Mode::empty(),
        )
        .map_err(io::Error::from)?;
        return Ok((d, logical.to_path_buf()));
    }

    let rel = rel_under_root(root_canonical, logical)?;
    if rel.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty relative path under workspace root",
        ));
    }

    let root = OpenOptions::new()
        .read(true)
        .open(root_canonical)
        .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;

    let how = OpenHow::new()
        .flags(OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC)
        .mode(Mode::empty())
        .resolve(ResolveFlag::RESOLVE_IN_ROOT);

    let owned = openat2(&root, rel.as_path(), how).map_err(io::Error::from)?;
    let dir = Dir::from_fd(owned).map_err(io::Error::from)?;
    Ok((dir, logical.to_path_buf()))
}

#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn open_existing_file_under_root(
    _root_canonical: &Path,
    logical: &Path,
) -> io::Result<OpenedWorkspaceFile> {
    let file = File::open(logical)?;
    let metadata = file.metadata()?;
    let resolved_path = logical.to_path_buf();
    Ok(OpenedWorkspaceFile {
        file,
        resolved_path,
        metadata,
    })
}

/// 在工作区根下打开用于写入的文件：`create_only` → `O_CREAT|O_EXCL`；`update_only` → 仅打开已存在；否则 `O_CREAT|O_TRUNC`。
#[cfg(target_os = "linux")]
pub(crate) fn open_file_write_under_root(
    root_canonical: &Path,
    logical: &Path,
    create_only: bool,
    update_only: bool,
) -> io::Result<File> {
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
    use nix::sys::stat::Mode;

    let rel = rel_under_root(root_canonical, logical)?;
    if rel.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "empty relative path under workspace root",
        ));
    }

    let root = OpenOptions::new()
        .read(true)
        .open(root_canonical)
        .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;

    let mut oflag = OFlag::O_WRONLY | OFlag::O_CLOEXEC;
    if create_only {
        oflag |= OFlag::O_CREAT | OFlag::O_EXCL;
    } else if update_only {
        // 必须已存在；不跟随末级 symlink（与 `O_NOFOLLOW` 创建语义一致）。
        oflag |= OFlag::O_NOFOLLOW;
    } else {
        oflag |= OFlag::O_CREAT | OFlag::O_TRUNC;
    }

    let mode = Mode::S_IRUSR | Mode::S_IWUSR | Mode::S_IRGRP | Mode::S_IROTH;
    let how = OpenHow::new()
        .flags(oflag)
        .mode(mode)
        .resolve(ResolveFlag::RESOLVE_IN_ROOT);

    let owned = openat2(&root, rel.as_path(), how).map_err(io::Error::from)?;
    Ok(unsafe { File::from_raw_fd(owned.into_raw_fd()) })
}

#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn open_file_write_under_root(
    _root_canonical: &Path,
    logical: &Path,
    create_only: bool,
    update_only: bool,
) -> io::Result<File> {
    let p = logical;
    if create_only {
        OpenOptions::new().create_new(true).write(true).open(p)
    } else if update_only {
        OpenOptions::new().write(true).open(p)
    } else {
        OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(p)
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn open_directory_under_root(
    _root_canonical: &Path,
    logical: &Path,
) -> io::Result<(nix::dir::Dir, PathBuf)> {
    use nix::dir::Dir;
    use nix::fcntl::OFlag;
    use nix::sys::stat::Mode;
    let d = Dir::open(
        logical,
        OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC,
        Mode::empty(),
    )
    .map_err(io::Error::from)?;
    Ok((d, logical.to_path_buf()))
}

#[cfg(not(unix))]
pub(crate) fn open_existing_file_under_root(
    _root_canonical: &Path,
    logical: &Path,
) -> io::Result<OpenedWorkspaceFile> {
    let file = File::open(logical)?;
    let metadata = file.metadata()?;
    Ok(OpenedWorkspaceFile {
        file,
        resolved_path: logical.to_path_buf(),
        metadata,
    })
}

/// 在工作区根下删除常规文件（非目录）：Linux 在父目录 fd 上 `unlinkat`，缩短按路径 `unlink` 的窗口。
#[cfg(all(unix, target_os = "linux"))]
pub(crate) fn unlink_file_under_root(root_canonical: &Path, logical: &Path) -> io::Result<()> {
    use nix::unistd::{UnlinkatFlags, unlinkat};

    let parent = logical
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no parent"))?;
    let name = logical
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
    let (dir, _) = open_directory_under_root(root_canonical, parent)?;
    unlinkat(&dir, name, UnlinkatFlags::NoRemoveDir).map_err(io::Error::from)
}

#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn unlink_file_under_root(_root_canonical: &Path, logical: &Path) -> io::Result<()> {
    std::fs::remove_file(logical)
}

#[cfg(not(unix))]
pub(crate) fn unlink_file_under_root(_root_canonical: &Path, logical: &Path) -> io::Result<()> {
    std::fs::remove_file(logical)
}

#[cfg(not(unix))]
pub(crate) fn open_file_write_under_root(
    _root_canonical: &Path,
    logical: &Path,
    create_only: bool,
    update_only: bool,
) -> io::Result<File> {
    use std::io::Write;
    if create_only {
        return OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(logical);
    }
    if update_only {
        return OpenOptions::new().write(true).open(logical);
    }
    if logical.exists() {
        let mut f = OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(logical)?;
        f.set_len(0)?;
        Ok(f)
    } else {
        OpenOptions::new().create(true).write(true).open(logical)
    }
}
