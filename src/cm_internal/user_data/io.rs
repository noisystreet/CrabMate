//! 原子写盘与目录权限。

use std::fs;
use std::io::Write;
use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::cm_tools::fs_atomic;

pub fn ensure_tree(root: &Path) -> Result<(), String> {
    for sub in [root, &root.join("global"), &root.join("workspaces")] {
        if sub.exists() {
            restrict_dir(sub)?;
        } else {
            fs::create_dir_all(sub).map_err(|e| format!("创建目录 {}: {e}", sub.display()))?;
            restrict_dir(sub)?;
        }
    }
    Ok(())
}

pub fn restrict_dir(p: &Path) -> Result<(), String> {
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

pub fn read_json_file<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    if !path.is_file() {
        return Err(format!("文件不存在: {}", path.display()));
    }
    let raw = fs::read_to_string(path).map_err(|e| format!("读取 {}: {e}", path.display()))?;
    serde_json::from_str(&raw).map_err(|e| format!("解析 {}: {e}", path.display()))
}

pub fn read_json_file_or_default<T: DeserializeOwned + Default>(path: &Path) -> T {
    if !path.is_file() {
        return T::default();
    }
    read_json_file(path).unwrap_or_default()
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| format!("创建目录 {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| format!("序列化 {}: {e}", path.display()))?;

    // 多硬链接（nlink > 1）时不能用 rename 换 inode，否则其他硬链接仍指向旧内容。
    let preserve_inode = fs_atomic::target_has_multiple_links(path);

    let tmp = fs_atomic::unique_tmp_path(path, "crabmate_tmp");
    let mut guard = fs_atomic::TmpFileGuard::new(tmp.clone());
    {
        // 以 0600 创建再按目标元数据放宽：目录已 0700，内容可能含敏感信息，避免出现宽权限窗口。
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        opts.mode(0o600);
        let mut f = opts
            .open(&tmp)
            .map_err(|e| format!("创建临时文件 {}: {e}", tmp.display()))?;
        if !preserve_inode {
            // 临时文件随后会 rename 覆盖目标，需继承原文件属主/权限/扩展属性，否则会退化为默认值。
            fs_atomic::inherit_metadata_from_target(path, &tmp, &f)?;
        }
        f.write_all(json.as_bytes())
            .map_err(|e| format!("写入 {}: {e}", tmp.display()))?;
        f.sync_all()
            .map_err(|e| format!("sync {}: {e}", tmp.display()))?;
    }

    fs_atomic::commit_tmp_over_target(&tmp, path, preserve_inode)?;
    guard.keep();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    fn json_paths(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn test_write_json_atomic_preserves_target_mode() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("prefs.json");
        fs::write(&path, "{}").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();

        write_json_atomic(&path, &serde_json::json!({"a": 1})).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "覆盖写状态文件应保留原权限位");
        assert_eq!(json_paths(dir.path()), vec!["prefs.json"]);
    }

    #[cfg(unix)]
    #[test]
    fn test_write_json_atomic_new_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.json");

        write_json_atomic(&path, &serde_json::json!({"a": 1})).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "新建状态文件应为仅属主可读写");
    }

    #[cfg(unix)]
    #[test]
    fn test_write_json_atomic_preserves_hard_link_inode() {
        use std::os::unix::fs::MetadataExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let link = dir.path().join("alias.json");
        fs::write(&path, "{}").unwrap();
        fs::hard_link(&path, &link).unwrap();
        let inode_before = fs::metadata(&path).unwrap().ino();

        write_json_atomic(&path, &serde_json::json!({"a": 1})).unwrap();

        let inode_after = fs::metadata(&path).unwrap().ino();
        assert_eq!(inode_after, inode_before, "多硬链接文件应保留原 inode");
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&fs::read_to_string(&link).unwrap()).unwrap(),
            serde_json::json!({"a": 1}),
            "通过硬链接应能看到更新后的内容"
        );
        assert!(
            json_paths(dir.path()).iter().all(|n| !n.contains("crabmate_tmp")),
            "成功提交后不应残留临时文件: {:?}",
            json_paths(dir.path())
        );
    }
}
