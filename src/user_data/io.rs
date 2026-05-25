//! 原子写盘与目录权限。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde::de::DeserializeOwned;

pub(crate) fn ensure_tree(root: &Path) -> Result<(), String> {
    for sub in [
        root,
        &root.join("global"),
        &root.join("workspaces"),
        &root.join("secrets"),
    ] {
        if sub.exists() {
            restrict_dir(sub)?;
        } else {
            fs::create_dir_all(sub).map_err(|e| format!("创建目录 {}: {e}", sub.display()))?;
            restrict_dir(sub)?;
        }
    }
    Ok(())
}

pub(crate) fn restrict_dir(p: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let meta = fs::metadata(p).map_err(|e| format!("metadata {}: {e}", p.display()))?;
        let mut perm = meta.permissions();
        perm.set_mode(0o700);
        fs::set_permissions(p, perm).map_err(|e| format!("chmod {}: {e}", p.display()))?;
    }
    Ok(())
}

pub(crate) fn restrict_secret_file(p: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if !p.exists() {
            return Ok(());
        }
        let meta = fs::metadata(p).map_err(|e| format!("metadata {}: {e}", p.display()))?;
        let mut perm = meta.permissions();
        perm.set_mode(0o600);
        fs::set_permissions(p, perm).map_err(|e| format!("chmod {}: {e}", p.display()))?;
    }
    Ok(())
}

pub(crate) fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("读取 {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 {}: {e}", path.display()))
}

pub(crate) fn read_json_file_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    if !path.is_file() {
        return T::default();
    }
    read_json_file(path).unwrap_or_default()
}

pub(crate) fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录 {}: {e}", parent.display()))?;
    }
    let tmp: PathBuf = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("序列化 {}: {e}", path.display()))?;
    {
        let mut f =
            fs::File::create(&tmp).map_err(|e| format!("创建临时文件 {}: {e}", tmp.display()))?;
        f.write_all(json.as_bytes())
            .map_err(|e| format!("写入 {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
    }
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    Ok(())
}

pub(crate) fn write_secret_line(path: &Path, content: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        ensure_tree(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content.trim()).map_err(|e| format!("写入 {}: {e}", tmp.display()))?;
    fs::rename(&tmp, path).map_err(|e| format!("rename {}: {e}", path.display()))?;
    restrict_secret_file(path)?;
    Ok(())
}

pub(crate) fn read_secret_line(path: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let t = raw.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}
