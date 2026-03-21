//! 在工作区内应用 **unified diff**（与 `git diff` / `diff -u` 同类）补丁。
//!
//! # CrabMate 统一补丁约定（给模型/人类）
//!
//! 1. **格式**：标准 unified diff，必须含 `---` / `+++` 文件头与 `@@ ... @@` hunk 头；行前缀 ` `（上下文）、`-`（删）、`+`（增）。
//! 2. **路径与 strip（二选一，须与 GNU patch 行为一致）**：
//!    - **写法 A（推荐，strip=0，默认）**：`--- src/foo.rs` / `+++ src/foo.rs`，路径为**相对工作区根**，与磁盘一致，**不要**加 `a/`/`b/`。
//!    - **写法 B（Git 风格，strip=1）**：`--- a/src/foo.rs` / `+++ b/src/foo.rs`，调用工具时传 **`strip: 1`**（剥掉 `a/` 后得到 `src/foo.rs`）；若误用 `strip=0` 仍带 `a/` 前缀，patch 会去找 `a/src/...` 而失败。
//! 3. **带上下文**：每个 hunk 在变更行上下保留 **至少 2～3 行** 未改动的上下文（` ` 开头），避免错位；**禁止**零上下文只贴一行。
//! 4. **小步**：一次补丁优先 **一个主题**（如一个函数、一处配置）；大改动拆成 **多次 `apply_patch`**，便于预检失败时定位。
//! 5. **可回滚**：成功应用后若需撤销，可用 `patch -R`（同补丁）、`git checkout -- 文件` 或再打一版 **反向 diff**；因此小步补丁更容易安全回退。
//!
//! 底层调用 GNU **`patch --batch --forward`**，先 **`--dry-run`** 再正式应用。
//!
//! # 安全策略
//!
//! - 仅允许相对路径（禁止绝对路径）
//! - 禁止 `..` 路径穿越
//! - 必须落在工作区根目录下
//! - 应用前先执行 `patch --dry-run` 预检查

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_OUTPUT_BYTES: usize = 12 * 1024;

pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let patch_text = match args.get("patch").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return "错误：缺少 patch 参数".to_string(),
    };
    let strip = args
        .get("strip")
        .and_then(|v| v.as_u64())
        .unwrap_or_default();

    let root = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {}", e),
    };

    if let Err(e) = validate_patch_paths(patch_text, &root) {
        return format!("补丁路径校验失败: {}", e);
    }

    let patch_file = match write_temp_patch_file(&root, patch_text) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let dry_run = run_patch_command(&root, &patch_file, strip, true);
    if !dry_run.success {
        let _ = std::fs::remove_file(&patch_file);
        return format!("apply_patch 预检查失败:\n{}", dry_run.output);
    }

    let apply = run_patch_command(&root, &patch_file, strip, false);
    let _ = std::fs::remove_file(&patch_file);
    if !apply.success {
        return format!("apply_patch 执行失败:\n{}", apply.output);
    }

    if apply.output.trim().is_empty() {
        "补丁应用成功".to_string()
    } else {
        format!("补丁应用成功:\n{}", apply.output)
    }
}

struct CmdResult {
    success: bool,
    output: String,
}

fn run_patch_command(root: &Path, patch_file: &Path, strip: u64, dry_run: bool) -> CmdResult {
    let mut cmd = Command::new("patch");
    cmd.arg("--batch")
        .arg("--forward")
        .arg("--reject-file=-")
        .arg(format!("--strip={}", strip))
        .arg(format!("--directory={}", root.display()))
        .arg(format!("--input={}", patch_file.display()));
    if dry_run {
        cmd.arg("--dry-run");
    }

    match cmd.output() {
        Ok(output) => {
            let status_ok = output.status.success();
            let mut text = String::new();
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.trim().is_empty() {
                text.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(stderr.trim_end());
            }
            if text.is_empty() {
                text = format!("patch 退出码: {}", output.status.code().unwrap_or(-1));
            }
            CmdResult {
                success: status_ok,
                output: truncate(&text, MAX_OUTPUT_BYTES),
            }
        }
        Err(e) => CmdResult {
            success: false,
            output: format!("无法启动 patch 命令: {}", e),
        },
    }
}

fn write_temp_patch_file(root: &Path, patch_text: &str) -> Result<PathBuf, String> {
    let cache_dir = root.join(".crabmate");
    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        return Err(format!("创建临时目录失败: {}", e));
    }
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let p = cache_dir.join(format!("tmp_patch_{}_{}.diff", std::process::id(), ts));
    std::fs::write(&p, patch_text.as_bytes())
        .map_err(|e| format!("写入临时补丁文件失败: {}", e))?;
    Ok(p)
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
    for c in p.components() {
        if matches!(c, Component::ParentDir) {
            return Err(format!("不允许使用 .. 路径: {}", raw_path));
        }
    }
    let joined = root.join(p);
    let normalized = normalize_path(&joined);
    if !normalized.starts_with(root) {
        return Err(format!("路径超出工作区: {}", raw_path));
    }
    ensure_existing_ancestor_within_workspace(root, &normalized)?;
    Ok(())
}

// 对“目标路径或其最近存在祖先”做 canonical 校验，防止借助工作区内 symlink 写到外部。
fn ensure_existing_ancestor_within_workspace(root: &Path, target: &Path) -> Result<(), String> {
    let mut ancestor = target;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| format!("路径无法解析: {}", target.display()))?;
    }
    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {} ({})", ancestor.display(), e))?;
    if !ancestor_canonical.starts_with(root) {
        return Err(format!("路径超出工作区: {}", target.display()));
    }
    Ok(())
}

fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn truncate(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut out = s[..max_bytes].to_string();
    out.push_str("\n\n... (patch 输出已截断)");
    out
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
        assert!(err.contains(".."));
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

    #[cfg(unix)]
    #[test]
    fn test_validate_single_path_rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

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
