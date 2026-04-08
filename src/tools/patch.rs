//! 在工作区内应用 **unified diff**（与 `git diff` / `diff -u` 同类）补丁。
//!
//! 使用 `diffy` crate 纯 Rust 实现，不依赖系统 `patch` 命令。
//!
//! # 安全策略
//!
//! - 仅允许相对路径（禁止绝对路径）
//! - 禁止 `..` 路径穿越
//! - 必须落在工作区根目录下

use crate::path_workspace::{absolutize_relative_under_root, ensure_existing_ancestor_within_root};
use crate::workspace_changelist::WorkspaceChangelist;
use std::path::Path;
use std::sync::Arc;

pub(crate) fn run_with_changelist(
    args_json: &str,
    workspace_root: &Path,
    changelist: Option<&Arc<WorkspaceChangelist>>,
) -> String {
    let args = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let patch_text = match args.get("patch").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return "错误：缺少 patch 参数".to_string(),
    };
    let strip = args
        .get("strip")
        .and_then(|v| v.as_u64())
        .unwrap_or_default() as usize;

    let root = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    if let Err(e) = validate_patch_paths(patch_text, &root) {
        return format!("补丁路径校验失败: {}", e);
    }

    apply_unified_patch(patch_text, &root, strip, changelist)
}

fn apply_unified_patch(
    patch_text: &str,
    root: &Path,
    strip: usize,
    changelist: Option<&Arc<WorkspaceChangelist>>,
) -> String {
    let patch = match diffy::Patch::from_str(patch_text) {
        Ok(p) => p,
        Err(e) => return format!("解析 unified diff 失败: {}", e),
    };

    let mut applied_files = Vec::new();
    let mut errors = Vec::new();

    for hunk_patch in split_patch_by_file(patch_text) {
        let (file_path, original_path) = match extract_target_path(&hunk_patch, strip) {
            Some(p) => p,
            None => {
                errors.push("无法提取文件路径".to_string());
                continue;
            }
        };

        if file_path == "/dev/null" || original_path == "/dev/null" {
            if original_path == "/dev/null" {
                let target = root.join(&file_path);
                let single = match diffy::Patch::from_str(&hunk_patch) {
                    Ok(p) => p,
                    Err(e) => {
                        errors.push(format!("{}: 解析失败: {}", file_path, e));
                        continue;
                    }
                };
                let new_content = diffy::apply("", &single);
                match new_content {
                    Ok(content) => {
                        if let Some(parent) = target.parent() {
                            let _ = std::fs::create_dir_all(parent);
                        }
                        if let Err(e) = std::fs::write(&target, content.as_bytes()) {
                            errors.push(format!("{}: 创建文件失败: {}", file_path, e));
                        } else {
                            if let Some(cl) = changelist {
                                cl.record_mutation(&file_path, None, Some(content));
                            }
                            applied_files.push(format!("新建: {}", file_path));
                        }
                    }
                    Err(e) => errors.push(format!("{}: 应用失败: {}", file_path, e)),
                }
                continue;
            }
            if file_path == "/dev/null" {
                let source = root.join(&original_path);
                if source.exists() {
                    let before = std::fs::read_to_string(&source).ok();
                    if let Err(e) = std::fs::remove_file(&source) {
                        errors.push(format!("{}: 删除失败: {}", original_path, e));
                    } else {
                        if let Some(cl) = changelist {
                            cl.record_mutation(&original_path, before, None);
                        }
                        applied_files.push(format!("删除: {}", original_path));
                    }
                }
                continue;
            }
        }

        let target = root.join(&file_path);
        let original = match std::fs::read_to_string(&target) {
            Ok(s) => s,
            Err(e) => {
                errors.push(format!("{}: 读取失败: {}", file_path, e));
                continue;
            }
        };

        let single = match diffy::Patch::from_str(&hunk_patch) {
            Ok(p) => p,
            Err(e) => {
                errors.push(format!("{}: 解析失败: {}", file_path, e));
                continue;
            }
        };

        match diffy::apply(&original, &single) {
            Ok(patched) => {
                if let Err(e) = std::fs::write(&target, patched.as_bytes()) {
                    errors.push(format!("{}: 写入失败: {}", file_path, e));
                } else {
                    if let Some(cl) = changelist {
                        cl.record_mutation(&file_path, Some(original.clone()), Some(patched));
                    }
                    applied_files.push(file_path);
                }
            }
            Err(e) => errors.push(format!("{}: 应用失败: {}", file_path, e)),
        }
    }

    let _ = patch;

    if errors.is_empty() {
        if applied_files.is_empty() {
            "补丁应用成功（无文件变更）".to_string()
        } else {
            format!("补丁应用成功：\n{}", applied_files.join("\n"))
        }
    } else if applied_files.is_empty() {
        format!("补丁应用失败：\n{}", errors.join("\n"))
    } else {
        format!(
            "补丁部分应用：\n成功：\n{}\n失败：\n{}",
            applied_files.join("\n"),
            errors.join("\n")
        )
    }
}

fn split_patch_by_file(patch_text: &str) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut in_file = false;

    for line in patch_text.lines() {
        if line.starts_with("--- ") {
            if in_file && !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            in_file = true;
        }
        if in_file {
            current.push_str(line);
            current.push('\n');
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    if chunks.is_empty() {
        chunks.push(patch_text.to_string());
    }
    chunks
}

fn extract_target_path(patch_chunk: &str, strip: usize) -> Option<(String, String)> {
    let mut original = None;
    let mut target = None;
    for line in patch_chunk.lines() {
        if let Some(rest) = line.strip_prefix("--- ") {
            let path = rest.split_whitespace().next()?;
            original = Some(strip_components(path, strip));
        } else if let Some(rest) = line.strip_prefix("+++ ") {
            let path = rest.split_whitespace().next()?;
            target = Some(strip_components(path, strip));
        }
        if original.is_some() && target.is_some() {
            break;
        }
    }
    Some((target?, original?))
}

fn strip_components(path: &str, n: usize) -> String {
    if path == "/dev/null" {
        return path.to_string();
    }
    let parts: Vec<&str> = path.split('/').collect();
    if n >= parts.len() {
        parts.last().unwrap_or(&"").to_string()
    } else {
        parts[n..].join("/")
    }
}

fn validate_patch_paths(patch_text: &str, root: &Path) -> Result<(), String> {
    let mut seen_header = false;
    for line in patch_text.lines() {
        let Some(raw_path) = parse_header_path(line) else {
            continue;
        };
        seen_header = true;
        if raw_path == "/dev/null" {
            continue;
        }
        validate_single_path(raw_path, root)?;
    }
    if !seen_header {
        return Err("未检测到 unified diff 文件头（--- / +++）".to_string());
    }
    Ok(())
}

fn parse_header_path(line: &str) -> Option<&str> {
    let body = if let Some(rest) = line.strip_prefix("--- ") {
        rest
    } else if let Some(rest) = line.strip_prefix("+++ ") {
        rest
    } else {
        return None;
    };
    body.split_whitespace().next()
}

fn validate_single_path(raw_path: &str, root: &Path) -> Result<(), String> {
    let path_no_prefix = raw_path
        .strip_prefix("a/")
        .or_else(|| raw_path.strip_prefix("b/"))
        .unwrap_or(raw_path);
    let p = Path::new(path_no_prefix);
    if p.is_absolute() {
        return Err(format!("不允许绝对路径: {}", raw_path));
    }
    let normalized = absolutize_relative_under_root(root, path_no_prefix)
        .map_err(|e| format!("路径超出工作区或无效: {} ({})", raw_path, e.user_message()))?;
    ensure_existing_ancestor_within_root(root, &normalized)
        .map_err(|e| format!("路径超出工作区或无效: {} ({})", raw_path, e.user_message()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_header_path() {
        assert_eq!(
            parse_header_path("--- a/src/main.rs"),
            Some("a/src/main.rs")
        );
        assert_eq!(
            parse_header_path("+++ b/src/main.rs\t2026-01-01"),
            Some("b/src/main.rs")
        );
        assert_eq!(parse_header_path("@@ -1,2 +1,2 @@"), None);
    }

    #[test]
    fn test_validate_single_path_rejects_parent() {
        let root = std::env::current_dir().unwrap();
        let err = validate_single_path("../etc/passwd", &root).unwrap_err();
        assert!(
            err.contains("超出") || err.contains("工作目录"),
            "应拒绝越出工作区: {}",
            err
        );
    }

    #[test]
    fn test_validate_patch_paths_ok() {
        let root = std::env::current_dir().unwrap();
        let patch = "\
--- a/src/main.rs
+++ b/src/main.rs
@@ -1 +1 @@
-old
+new
";
        assert!(validate_patch_paths(patch, &root).is_ok());
    }

    #[test]
    fn test_strip_components() {
        assert_eq!(strip_components("a/src/main.rs", 1), "src/main.rs");
        assert_eq!(strip_components("src/main.rs", 0), "src/main.rs");
        assert_eq!(strip_components("/dev/null", 1), "/dev/null");
    }

    #[test]
    fn test_split_patch_by_file() {
        let patch = "\
--- a/foo.rs
+++ b/foo.rs
@@ -1 +1 @@
-old
+new
--- a/bar.rs
+++ b/bar.rs
@@ -1 +1 @@
-x
+y
";
        let chunks = split_patch_by_file(patch);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].contains("foo.rs"));
        assert!(chunks[1].contains("bar.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn test_validate_single_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;
        use std::time::{SystemTime, UNIX_EPOCH};

        let root = std::env::temp_dir().join(format!(
            "crabmate_patch_tool_test_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        let outside = std::env::temp_dir().join(format!(
            "crabmate_patch_outside_{}_{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("escape");
        symlink(&outside, &link).unwrap();

        let err = validate_single_path("escape/pwned.txt", &root).unwrap_err();
        assert!(
            err.contains("路径超出工作区"),
            "应拒绝 symlink 绕过: {}",
            err
        );

        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
