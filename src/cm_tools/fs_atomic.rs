//! 原子替换写盘：唯一临时名 + 元数据继承 + 硬链接写回 + 父目录 fsync。
//!
//! 供 `modify_file`（replace_lines / insert_after_line）与用户数据 JSON 状态文件写盘共用：
//! 目标存在时继承其属主/权限/扩展属性，避免 rename 覆盖后元数据退化；`nlink > 1` 时改走
//! 「写回原 inode」以保留硬链接（POSIX 下与 rename 原子替换不可兼得，故该路径非原子）。

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 临时文件守卫：提交成功后调用 [`TmpFileGuard::keep`]；否则 Drop 时删除，避免异常路径残留。
pub(crate) struct TmpFileGuard {
    path: PathBuf,
    committed: bool,
}

impl TmpFileGuard {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn keep(&mut self) {
        self.committed = true;
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.path);
        }
    }
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// 与目标同目录的唯一临时名 `.{fname}.{label}.{pid}.{seq}`，避免并发写同一文件时临时名冲突。
pub(crate) fn unique_tmp_path(target: &Path, label: &str) -> PathBuf {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => PathBuf::from("."),
    };
    let name = target
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    parent.join(format!(".{name}.{label}.{}.{}", std::process::id(), seq))
}

/// 目标存在时把其属主（best-effort）→ 权限位 → 扩展属性继承到临时文件；
/// 目标不存在则不做改动（保留调用方创建的默认权限）。
pub(crate) fn inherit_metadata_from_target(
    target: &Path,
    tmp_path: &Path,
    tmp_file: &File,
) -> Result<(), String> {
    let Ok(meta) = fs::metadata(target) else {
        return Ok(());
    };
    copy_owner_best_effort(&meta, tmp_file);
    // chown 可能清掉 setuid/setgid，故 chmod 必须放在 chown 之后。
    tmp_file
        .set_permissions(meta.permissions())
        .map_err(|e| format!("设置临时文件权限失败: {}", e))?;
    copy_xattrs_best_effort(target, tmp_path);
    Ok(())
}

/// best-effort 复制属主：无 CAP_CHOWN 时忽略失败（属主通常已与当前进程一致）。
#[cfg(unix)]
fn copy_owner_best_effort(meta: &fs::Metadata, dst: &File) {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;
    unsafe {
        let _ = libc::fchown(
            dst.as_raw_fd(),
            meta.uid() as libc::uid_t,
            meta.gid() as libc::gid_t,
        );
    }
}

#[cfg(not(unix))]
fn copy_owner_best_effort(_meta: &fs::Metadata, _dst: &File) {}

/// 复制源文件扩展属性（含 POSIX ACL 的 `system.posix_acl_access`）到目标；逐项 best-effort。
#[cfg(target_os = "linux")]
fn copy_xattrs_best_effort(src_path: &Path, dst_path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let src = match CString::new(src_path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return,
    };
    let dst = match CString::new(dst_path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let size = unsafe { libc::listxattr(src.as_ptr(), std::ptr::null_mut(), 0) };
    if size <= 0 {
        return;
    }
    let mut names = vec![0u8; size as usize];
    let got = unsafe {
        libc::listxattr(
            src.as_ptr(),
            names.as_mut_ptr() as *mut libc::c_char,
            names.len(),
        )
    };
    if got <= 0 {
        return;
    }
    names.truncate(got as usize);

    for name in names.split(|b| *b == 0u8) {
        if name.is_empty() {
            continue;
        }
        let name_c = match CString::new(name.to_vec()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let vlen = unsafe { libc::getxattr(src.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
        if vlen < 0 {
            continue;
        }
        let mut val = vec![0u8; vlen as usize];
        let vgot = unsafe {
            libc::getxattr(
                src.as_ptr(),
                name_c.as_ptr(),
                val.as_mut_ptr() as *mut libc::c_void,
                val.len(),
            )
        };
        if vgot < 0 {
            continue;
        }
        val.truncate(vgot as usize);
        // `security.*`（如 SELinux label）等可能因权限失败，best-effort 忽略。
        unsafe {
            let _ = libc::setxattr(
                dst.as_ptr(),
                name_c.as_ptr(),
                val.as_ptr() as *const libc::c_void,
                val.len(),
                0,
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn copy_xattrs_best_effort(_src_path: &Path, _dst_path: &Path) {}

/// 目标是否为多硬链接（nlink > 1）。这类文件不能用 rename 换 inode，
/// 否则其他硬链接仍指向旧内容。
#[cfg(unix)]
pub(crate) fn target_has_multiple_links(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    fs::metadata(path).map(|m| m.nlink() > 1).unwrap_or(false)
}

#[cfg(not(unix))]
pub(crate) fn target_has_multiple_links(_path: &Path) -> bool {
    false
}

/// 硬链接场景：把临时文件内容写回原 inode（O_TRUNC 原地覆盖），保留 inode，
/// 使其他硬链接同步可见新内容。写回成功后清理临时文件（best-effort）。
fn write_back_into_target_inode(tmp_path: &Path, target: &Path) -> Result<(), String> {
    let mut src =
        File::open(tmp_path).map_err(|e| format!("读取临时文件失败: {}", e))?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(target)
        .map_err(|e| format!("写入原文件失败: {}", e))?;
    std::io::copy(&mut src, &mut dst).map_err(|e| format!("写回原文件失败: {}", e))?;
    dst.sync_all().map_err(|e| format!("落盘原文件失败: {}", e))?;
    let _ = fs::remove_file(tmp_path);
    Ok(())
}

/// 提交临时文件：`preserve_inode` 为真时写回原 inode（保留硬链接，非原子），
/// 否则同目录 `rename` 原子替换；成功后尽力 fsync 父目录确保目录项落盘。
pub(crate) fn commit_tmp_over_target(
    tmp_path: &Path,
    target: &Path,
    preserve_inode: bool,
) -> Result<(), String> {
    if preserve_inode {
        write_back_into_target_inode(tmp_path, target)?;
    } else {
        // 同目录下 rename(2) 会对已存在的目标做原子替换；不再先 remove_file，
        // 既避免目标短暂消失，也避免 rename 失败时原文件已被删除（数据丢失）。
        fs::rename(tmp_path, target).map_err(|e| format!("替换目标文件失败: {}", e))?;
    }
    if let Some(parent) = target.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}
