//! Web 工作区「项目池」：在固定根目录下用短名称标识子工作区（远程浏览器无需手输绝对路径）。

use std::path::{Path, PathBuf};

use thiserror::Error;

/// 项目名最大长度（不含路径分隔符）。
pub const WORKSPACE_PROJECT_NAME_MAX_LEN: usize = 64;

const RESERVED_PROJECT_NAMES: &[&str] = &[".", "..", ".crabmate"];

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WorkspaceProjectNameError {
    #[error("项目名不能为空")]
    Empty,
    #[error("项目名过长（最多 {max} 个字符）")]
    TooLong { max: usize },
    #[error("项目名 \"{name}\" 为保留名称")]
    Reserved { name: String },
    #[error("项目名须以字母或数字开头，且仅可含字母、数字、\".\"、\"_\"、\"-\"")]
    InvalidChars,
}

/// 校验 Web 项目池中的项目名（不含 `/` 等路径分量）。
pub fn validate_workspace_project_name(raw: &str) -> Result<&str, WorkspaceProjectNameError> {
    let name = raw.trim();
    if name.is_empty() {
        return Err(WorkspaceProjectNameError::Empty);
    }
    if name.len() > WORKSPACE_PROJECT_NAME_MAX_LEN {
        return Err(WorkspaceProjectNameError::TooLong {
            max: WORKSPACE_PROJECT_NAME_MAX_LEN,
        });
    }
    if RESERVED_PROJECT_NAMES.contains(&name) {
        return Err(WorkspaceProjectNameError::Reserved {
            name: name.to_string(),
        });
    }
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err(WorkspaceProjectNameError::Empty);
    };
    if !first.is_ascii_alphanumeric() {
        return Err(WorkspaceProjectNameError::InvalidChars);
    }
    for c in chars {
        if !c.is_ascii_alphanumeric() && c != '.' && c != '_' && c != '-' {
            return Err(WorkspaceProjectNameError::InvalidChars);
        }
    }
    Ok(name)
}

/// 将合法项目名解析为池根下的目录路径（**不**要求目录已存在）。
pub fn workspace_project_dir(
    pool: &Path,
    raw_name: &str,
) -> Result<PathBuf, WorkspaceProjectNameError> {
    let name = validate_workspace_project_name(raw_name)?;
    Ok(pool.join(name))
}

/// 列举项目池下符合命名规则的一级子目录名（按字典序）。
pub fn list_workspace_projects(pool: &Path) -> Result<Vec<String>, String> {
    if !pool.is_dir() {
        return Ok(vec![]);
    }
    let mut names = Vec::new();
    for entry in std::fs::read_dir(pool).map_err(|e| format!("读取项目池失败: {e}"))? {
        let entry = entry.map_err(|e| format!("读取项目池项失败: {e}"))?;
        let ft = entry
            .file_type()
            .map_err(|e| format!("读取项目池项类型失败: {e}"))?;
        if !ft.is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if validate_workspace_project_name(&name).is_ok() {
            names.push(name);
        }
    }
    names.sort();
    names.dedup();
    Ok(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_simple_names() {
        assert_eq!(validate_workspace_project_name("my-app").unwrap(), "my-app");
        assert_eq!(
            validate_workspace_project_name("A1_b2.c3").unwrap(),
            "A1_b2.c3"
        );
    }

    #[test]
    fn rejects_empty_and_reserved() {
        assert_eq!(
            validate_workspace_project_name("  "),
            Err(WorkspaceProjectNameError::Empty)
        );
        assert!(matches!(
            validate_workspace_project_name(".crabmate"),
            Err(WorkspaceProjectNameError::Reserved { .. })
        ));
    }

    #[test]
    fn rejects_path_like_names() {
        assert_eq!(
            validate_workspace_project_name("../x"),
            Err(WorkspaceProjectNameError::InvalidChars)
        );
        assert_eq!(
            validate_workspace_project_name("foo/bar"),
            Err(WorkspaceProjectNameError::InvalidChars)
        );
    }

    #[test]
    fn list_skips_invalid_entries() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::create_dir(root.path().join("good")).expect("mkdir");
        std::fs::write(root.path().join("file.txt"), b"x").expect("write");
        std::fs::create_dir(root.path().join(".crabmate")).expect("mkdir hidden");
        let list = list_workspace_projects(root.path()).expect("list");
        assert_eq!(list, vec!["good".to_string()]);
    }
}
