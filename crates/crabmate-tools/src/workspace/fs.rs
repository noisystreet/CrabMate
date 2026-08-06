//! 工作区内路径的**打开**语义：在 Linux 上优先使用 **`openat2(2)`** + **`RESOLVE_IN_ROOT`**，将路径解析约束在已校验的工作区根目录 fd 下，缓解「先 `canonicalize` 再按路径字符串 `open`」的 **TOCTOU**。
//!
//! - **Linux**：相对路径在**根目录 fd** 上解析；**工作区内的符号链接仍可被跟随**，但解析不得越过该根（含绝对 symlink 目标）。
//! - **其它 Unix**：回退为对已 `canonicalize` 路径的单次 `std::fs` 打开（与历史行为一致）。
//! - **非 Unix**：不包含 Linux 专用依赖；由调用方使用 `std::fs`。

use std::fs::{File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

#[cfg(unix)]
use std::os::unix::io::{AsRawFd, FromRawFd, IntoRawFd};

/// `resolve_for_read_open` 的成功结果：已打开的文件句柄、用于缓存键等指标的路径、以及 **`fstat`** 元数据（与打开同一 inode，避免对路径二次 `open`）。
pub struct OpenedWorkspaceFile {
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
pub fn open_existing_file_under_root(
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
pub fn open_directory_under_root(
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
pub fn open_existing_file_under_root(
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
pub fn open_file_write_under_root(
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
pub fn open_file_write_under_root(
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
pub fn open_directory_under_root(
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
pub fn open_existing_file_under_root(
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
pub fn unlink_file_under_root(root_canonical: &Path, logical: &Path) -> io::Result<()> {
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
pub fn unlink_file_under_root(_root_canonical: &Path, logical: &Path) -> io::Result<()> {
    std::fs::remove_file(logical)
}

#[cfg(not(unix))]
pub fn unlink_file_under_root(_root_canonical: &Path, logical: &Path) -> io::Result<()> {
    std::fs::remove_file(logical)
}

#[cfg(not(unix))]
pub fn open_file_write_under_root(
    _root_canonical: &Path,
    logical: &Path,
    create_only: bool,
    update_only: bool,
) -> io::Result<File> {
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

/// 在工作区根下创建 `logical` 指向的目录链（`parents=true` 等价 `mkdir -p`）。
#[cfg(target_os = "linux")]
pub fn create_directory_under_root(
    root_canonical: &Path,
    logical: &Path,
    parents: bool,
) -> io::Result<()> {
    use nix::errno::Errno;
    use nix::sys::stat::{Mode, mkdirat};

    let rel = rel_under_root(root_canonical, logical)?;
    if rel.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cannot create workspace root as subdirectory",
        ));
    }
    let components: Vec<&std::ffi::OsStr> = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s),
            _ => None,
        })
        .collect();
    if components.is_empty() {
        return Ok(());
    }

    let root = OpenOptions::new()
        .read(true)
        .open(root_canonical)
        .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;
    let mode = Mode::S_IRWXU | Mode::S_IRGRP | Mode::S_IXGRP | Mode::S_IROTH | Mode::S_IXOTH;

    if !parents {
        if components.len() == 1 {
            match mkdirat(&root, Path::new(components[0]), mode) {
                Ok(()) => Ok(()),
                Err(Errno::EEXIST) => Ok(()),
                Err(e) => Err(io::Error::from(e)),
            }
        } else {
            let mut parent_rel = PathBuf::new();
            for c in &components[..components.len() - 1] {
                parent_rel.push(c);
            }
            let parent_logical = root_canonical.join(parent_rel);
            let (parent_dir, _) = open_directory_under_root(root_canonical, &parent_logical)?;
            let last = components[components.len() - 1];
            match mkdirat(&parent_dir, Path::new(last), mode) {
                Ok(()) => Ok(()),
                Err(Errno::EEXIST) => Ok(()),
                Err(e) => Err(io::Error::from(e)),
            }
        }
    } else {
        mkdirat_components_from_fd(&root, &components, mode)
    }
}

#[cfg(target_os = "linux")]
fn open_dir_fd_under_parent(parent: &File, name: &std::ffi::OsStr) -> io::Result<File> {
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
    use nix::sys::stat::Mode;

    let how = OpenHow::new()
        .flags(OFlag::O_RDONLY | OFlag::O_DIRECTORY | OFlag::O_CLOEXEC)
        .mode(Mode::empty())
        .resolve(ResolveFlag::RESOLVE_IN_ROOT);
    let owned = openat2(parent, name, how).map_err(io::Error::from)?;
    // SAFETY: `openat2` 成功返回的新建 fd。
    Ok(unsafe { File::from_raw_fd(owned.into_raw_fd()) })
}

#[cfg(target_os = "linux")]
fn mkdirat_components_from_fd(
    root: &File,
    components: &[&std::ffi::OsStr],
    mode: nix::sys::stat::Mode,
) -> io::Result<()> {
    use nix::errno::Errno;
    use nix::sys::stat::mkdirat;

    let mut current_fd = root.try_clone()?;
    for comp in components {
        match mkdirat(&current_fd, Path::new(comp), mode) {
            Ok(()) => {}
            Err(Errno::EEXIST) => {}
            Err(e) => return Err(io::Error::from(e)),
        }
        current_fd = open_dir_fd_under_parent(&current_fd, comp)?;
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn create_directory_under_root(
    _root_canonical: &Path,
    logical: &Path,
    parents: bool,
) -> io::Result<()> {
    if parents {
        std::fs::create_dir_all(logical)
    } else {
        std::fs::create_dir(logical)
    }
}

#[cfg(not(unix))]
pub fn create_directory_under_root(
    _root_canonical: &Path,
    logical: &Path,
    parents: bool,
) -> io::Result<()> {
    if parents {
        std::fs::create_dir_all(logical)
    } else {
        std::fs::create_dir(logical)
    }
}

/// 为 `logical` 文件路径在工作区根下创建父目录（`mkdirat` 链，Linux）。
pub fn ensure_parent_dirs_under_root(root_canonical: &Path, logical_file: &Path) -> io::Result<()> {
    let rel = rel_under_root(root_canonical, logical_file)?;
    let parent = match rel.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Ok(()),
    };
    let logical_dir = root_canonical.join(parent);
    create_directory_under_root(root_canonical, &logical_dir, true)
}

/// 在已校验根下写入字节：先 `ensure_parent_dirs`，再 `open_file_write_under_root`。
pub fn write_bytes_under_root(
    root_canonical: &Path,
    logical: &Path,
    content: &[u8],
    create_only: bool,
    update_only: bool,
) -> io::Result<()> {
    ensure_parent_dirs_under_root(root_canonical, logical)?;
    let mut f = open_file_write_under_root(root_canonical, logical, create_only, update_only)?;
    f.write_all(content)?;
    Ok(())
}

/// 追加写入：Linux 上经 `openat2`；`create_if_missing` 为真时允许 `O_CREAT`。
#[cfg(target_os = "linux")]
pub fn open_file_append_under_root(
    root_canonical: &Path,
    logical: &Path,
    create_if_missing: bool,
) -> io::Result<File> {
    use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
    use nix::sys::stat::Mode;

    ensure_parent_dirs_under_root(root_canonical, logical)?;
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

    let mut oflag = OFlag::O_WRONLY | OFlag::O_APPEND | OFlag::O_CLOEXEC;
    if create_if_missing {
        oflag |= OFlag::O_CREAT;
    } else {
        oflag |= OFlag::O_NOFOLLOW;
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
pub fn open_file_append_under_root(
    _root_canonical: &Path,
    logical: &Path,
    create_if_missing: bool,
) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.append(true);
    if create_if_missing {
        opts.create(true);
    }
    opts.open(logical)
}

#[cfg(not(unix))]
pub fn open_file_append_under_root(
    _root_canonical: &Path,
    logical: &Path,
    create_if_missing: bool,
) -> io::Result<File> {
    let mut opts = OpenOptions::new();
    opts.append(true);
    if create_if_missing {
        opts.create(true);
    }
    opts.open(logical)
}

/// 在工作区根内移动常规文件（`renameat`；跨设备时复制后删除源）。
#[cfg(all(unix, target_os = "linux"))]
fn rename_file_cross_device_copy(
    root_canonical: &Path,
    src_logical: &Path,
    dst_logical: &Path,
) -> io::Result<()> {
    let opened = open_existing_file_under_root(root_canonical, src_logical)?;
    let bytes = read_opened_file_bytes(&opened)?;
    write_bytes_under_root(root_canonical, dst_logical, &bytes, false, false)?;
    unlink_file_under_root(root_canonical, src_logical)?;
    Ok(())
}

/// 在工作区根内移动常规文件（`renameat`；跨设备时复制后删除源）。
#[cfg(all(unix, target_os = "linux"))]
pub fn rename_file_under_root(
    root_canonical: &Path,
    src_logical: &Path,
    dst_logical: &Path,
) -> io::Result<()> {
    use nix::errno::Errno;
    use nix::fcntl::renameat;

    ensure_parent_dirs_under_root(root_canonical, dst_logical)?;
    let src_parent = src_logical
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "src has no parent"))?;
    let src_name = src_logical
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "src has no file name"))?;
    let dst_parent = dst_logical
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "dst has no parent"))?;
    let dst_name = dst_logical
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "dst has no file name"))?;

    let (src_dir, _) = open_directory_under_root(root_canonical, src_parent)?;
    let (dst_dir, _) = open_directory_under_root(root_canonical, dst_parent)?;
    match renameat(&src_dir, src_name, &dst_dir, dst_name) {
        Ok(()) => Ok(()),
        Err(Errno::EXDEV) => {
            rename_file_cross_device_copy(root_canonical, src_logical, dst_logical)
        }
        Err(e) => Err(io::Error::from(e)),
    }
}

#[cfg(all(unix, not(target_os = "linux")))]
pub fn rename_file_under_root(
    _root_canonical: &Path,
    src_logical: &Path,
    dst_logical: &Path,
) -> io::Result<()> {
    std::fs::rename(src_logical, dst_logical)
}

#[cfg(not(unix))]
pub fn rename_file_under_root(
    _root_canonical: &Path,
    src_logical: &Path,
    dst_logical: &Path,
) -> io::Result<()> {
    std::fs::rename(src_logical, dst_logical)
}

fn read_opened_file_bytes(opened: &OpenedWorkspaceFile) -> io::Result<Vec<u8>> {
    let mut f = opened.file.try_clone()?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    Ok(buf)
}

/// 从已打开的源文件复制到目标逻辑路径（目标不存在或覆盖由 `overwrite` 控制）。
pub fn copy_opened_file_under_root(
    root_canonical: &Path,
    opened: &OpenedWorkspaceFile,
    dst_logical: &Path,
    overwrite: bool,
) -> io::Result<u64> {
    if !overwrite {
        #[cfg(target_os = "linux")]
        {
            let rel = rel_under_root(root_canonical, dst_logical)?;
            if !rel.as_os_str().is_empty() {
                let root = OpenOptions::new()
                    .read(true)
                    .open(root_canonical)
                    .map_err(|e| io::Error::new(e.kind(), format!("open workspace root: {e}")))?;
                use nix::fcntl::{OFlag, OpenHow, ResolveFlag, openat2};
                use nix::sys::stat::Mode;
                let how = OpenHow::new()
                    .flags(OFlag::O_RDONLY | OFlag::O_CLOEXEC | OFlag::O_NOFOLLOW)
                    .mode(Mode::empty())
                    .resolve(ResolveFlag::RESOLVE_IN_ROOT);
                if openat2(&root, rel.as_path(), how).is_ok() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        "destination exists",
                    ));
                }
            }
        }
        #[cfg(not(target_os = "linux"))]
        if dst_logical.exists() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "destination exists",
            ));
        }
    }
    let bytes = read_opened_file_bytes(opened)?;
    write_bytes_under_root(root_canonical, dst_logical, &bytes, false, false)?;
    Ok(bytes.len() as u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn create_directory_under_root_nested_linux() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path().canonicalize().expect("canon");
        let target = base.join("a/b/c");
        create_directory_under_root(&base, &target, true).expect("mkdir -p");
        assert!(target.is_dir());
    }

    #[test]
    fn write_bytes_under_root_creates_parents() {
        let dir = tempdir().expect("tempdir");
        let base = dir.path().canonicalize().expect("canon");
        let file = base.join("nested/new.txt");
        write_bytes_under_root(&base, &file, b"hi", false, false).expect("write");
        assert_eq!(std::fs::read_to_string(&file).expect("read"), "hi");
    }
}
