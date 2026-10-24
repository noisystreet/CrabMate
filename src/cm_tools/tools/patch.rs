//! 在工作区内应用 **unified diff**（与 `git diff` / `diff -u` 同类）补丁。
//!
//! 使用 `diffy` crate 纯 Rust 实现，不依赖系统 `patch` 命令。
//!
//! # 安全策略
//!
//! - 仅允许相对路径（禁止绝对路径）
//! - 禁止 `..` 路径穿越
//! - 必须落在工作区根目录下

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::file;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::WorkspaceChangelist;
use crate::cm_tools::workspace::path::{
    absolutize_relative_under_root, ensure_existing_ancestor_within_root,
};
use std::path::Path;
use std::sync::Arc;

/// 一次 `apply_patch` 调用的执行上下文（root / 工具上下文 / 是否预览 / 原始入参）。
///
/// 收成一个结构体是为了让每个 hunk 处理函数只接 2～3 个参数：原先 7 个平铺参数的写法
/// 在加 `dry_run` / `base_revision` 后会直接撞上仓库的形参上限。
struct PatchApply<'a> {
    root: &'a Path,
    ctx: &'a ToolContext<'a>,
    changelist: Option<&'a Arc<WorkspaceChangelist>>,
    dry_run: bool,
    /// 原始入参：`base_revision` 由 `file::verify_write_revision` 直接从这里读。
    args: &'a serde_json::Value,
}

#[cfg(test)]
pub fn run_with_changelist(args_json: &str, ctx: &ToolContext<'_>) -> String {
    run_with_changelist_try(args_json, ctx).unwrap_or_else(|e| e.message)
}

/// `run_with_changelist` 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn run_with_changelist_try(
    args_json: &str,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let args = crate::cm_tools::tools::parse_args_json(args_json)
        .map_err(ToolError::invalid_args)?;

    let patch_text = match args.get("patch").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return Err(ToolError::invalid_args("错误：缺少 patch 参数".to_string())),
    };
    let strip = args
        .get("strip")
        .and_then(|v| v.as_u64())
        .unwrap_or_default() as usize;
    let dry_run = args
        .get("dry_run")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let root = ctx.working_dir.canonicalize().map_err(|e| {
        ToolError::external_code(
            "apply_patch_failed",
            format!("工作区根目录无法解析: {}", e),
        )
    })?;

    if let Err(e) = validate_patch_paths(patch_text, &root) {
        return Err(ToolError::invalid_args(format!("补丁路径校验失败: {}", e)));
    }

    let apply = PatchApply {
        root: &root,
        ctx,
        changelist: ctx.workspace_changelist,
        dry_run,
        args: &args,
    };
    apply_unified_patch_try(&apply, patch_text, strip)
}

#[allow(clippy::result_large_err)]
fn apply_unified_patch_try(
    apply: &PatchApply<'_>,
    patch_text: &str,
    strip: usize,
) -> Result<String, ToolError> {
    let chunks = split_patch_by_file(patch_text);
    for chunk in &chunks {
        if let Err(e) = diffy::Patch::from_str(chunk) {
            return Err(ToolError::invalid_args(format!(
                "解析 unified diff 失败: {}",
                e
            )));
        }
    }

    // 两阶段：先把每个 hunk **试算**成「计划变更」，全部成功才落盘。
    // 原实现边算边写，多文件补丁在第 3 个文件对不上时会留下前 2 个已改的残局
    // （只靠「部分应用」文案告知），与「不可逆操作要先能整体确认」相悖。
    let mut plans: Vec<PlannedPatch> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    for hunk_patch in &chunks {
        match plan_single_hunk(apply, hunk_patch, strip) {
            Ok(Some(plan)) => plans.push(plan),
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    if !errors.is_empty() {
        // 失败正文同样走带显式头的格式化：`apply_patch` 的既有契约是「失败正文里只有
        // 一个 `crabmate_tool_output` 头」，别因为提前返回就绕过它。
        let output = format_tool_output_with_write_diff_preview(
            "apply_patch",
            false,
            unified_patch_format_outcome(&[], &errors),
            Vec::new(),
            WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
        );
        return Err(ToolError::external_code("apply_patch_failed", output));
    }
    ensure_single_file_base_revision(apply, &plans)?;

    let mut preview_files: Vec<WriteDiffFileState> = Vec::new();
    let mut applied_files = Vec::new();
    for plan in &plans {
        commit_planned(apply, plan, &mut preview_files, &mut applied_files)
            .map_err(|e| ToolError::external_code("apply_patch_failed", e))?;
    }

    let output = format_tool_output_with_write_diff_preview(
        "apply_patch",
        true,
        unified_patch_format_outcome(&applied_files, &[]),
        preview_files,
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    );
    Ok(output)
}

/// `base_revision` 只能表达**一个**文件的版本基线；多文件补丁无法用它做 CAS。
///
/// 与其静默忽略（调用方以为有保护），不如明确拒绝并给出改法。
#[allow(clippy::result_large_err)] // 与同文件其它入口一致：`ToolError` 是既有错误类型
fn ensure_single_file_base_revision(
    apply: &PatchApply<'_>,
    plans: &[PlannedPatch],
) -> Result<(), ToolError> {
    let has_base_revision = apply
        .args
        .get("base_revision")
        .and_then(|v| v.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if has_base_revision && plans.len() > 1 {
        return Err(ToolError::invalid_args(format!(
            "错误：base_revision 只能用于单文件补丁，当前补丁涉及 {} 个文件；请拆成多次 apply_patch，或去掉 base_revision",
            plans.len()
        )));
    }
    Ok(())
}

fn unified_patch_format_outcome(applied_files: &[String], errors: &[String]) -> String {
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

enum DevNullHunkKind {
    Create,
    Delete,
}

fn classify_dev_null_hunk(file_path: &str, original_path: &str) -> Option<DevNullHunkKind> {
    if original_path == "/dev/null" {
        Some(DevNullHunkKind::Create)
    } else if file_path == "/dev/null" {
        Some(DevNullHunkKind::Delete)
    } else {
        None
    }
}

/// 一个待提交的文件变更（先全部试算，再统一提交）。
enum PlannedPatch {
    Modify {
        rel: String,
        before: String,
        after: String,
    },
    Create {
        rel: String,
        after: String,
    },
    Delete {
        rel: String,
        before: Option<String>,
    },
}

/// 试算单个 hunk：只读 + 计算，**不落盘**。`Ok(None)` 表示无需变更（如删除的目标本就不存在）。
fn plan_single_hunk(
    apply: &PatchApply<'_>,
    hunk_patch: &str,
    strip: usize,
) -> Result<Option<PlannedPatch>, String> {
    let (file_path, original_path) =
        extract_target_path(hunk_patch, strip).ok_or_else(|| "无法提取文件路径".to_string())?;

    match classify_dev_null_hunk(&file_path, &original_path) {
        Some(DevNullHunkKind::Create) => {
            let single = diffy::Patch::from_str(hunk_patch)
                .map_err(|e| format!("{}: 解析失败: {}", file_path, e))?;
            let after = diffy::apply("", &single)
                .map_err(|e| format!("{}: 应用失败: {}", file_path, e))?;
            Ok(Some(PlannedPatch::Create {
                rel: file_path,
                after,
            }))
        }
        Some(DevNullHunkKind::Delete) => {
            let source = apply.root.join(&original_path);
            if !source.exists() {
                return Ok(None);
            }
            Ok(Some(PlannedPatch::Delete {
                before: std::fs::read_to_string(&source).ok(),
                rel: original_path,
            }))
        }
        None => {
            let target = apply.root.join(&file_path);
            let before = std::fs::read_to_string(&target)
                .map_err(|e| format!("{}: 读取失败: {}", file_path, e))?;
            let single = diffy::Patch::from_str(hunk_patch)
                .map_err(|e| format!("{}: 解析失败: {}", file_path, e))?;
            let after = diffy::apply(&before, &single)
                .map_err(|e| format!("{}: 应用失败: {}", file_path, e))?;
            Ok(Some(PlannedPatch::Modify {
                rel: file_path,
                before,
                after,
            }))
        }
    }
}

/// 提交单个计划变更：`dry_run` 只记账不落盘；否则走原子替换 + 可选 CAS。
fn commit_planned(
    apply: &PatchApply<'_>,
    plan: &PlannedPatch,
    preview_files: &mut Vec<WriteDiffFileState>,
    applied_files: &mut Vec<String>,
) -> Result<(), String> {
    match plan {
        PlannedPatch::Modify { rel, before, after } => {
            write_planned(apply, rel, after)?;
            record_mutation(apply, rel, Some(before.clone()), Some(after.clone()));
            preview_files.push(WriteDiffFileState {
                rel_path: rel.clone(),
                before: Some(before.clone()),
                after: Some(after.clone()),
            });
            applied_files.push(rel.clone());
        }
        PlannedPatch::Create { rel, after } => {
            ensure_parent_dir(apply, rel)?;
            write_planned(apply, rel, after)?;
            record_mutation(apply, rel, None, Some(after.clone()));
            preview_files.push(WriteDiffFileState {
                rel_path: rel.clone(),
                before: None,
                after: Some(after.clone()),
            });
            applied_files.push(format!("新建: {}", rel));
        }
        PlannedPatch::Delete { rel, before } => {
            if !apply.dry_run {
                let target = apply.root.join(rel);
                file::verify_write_revision(apply.args, &target, apply.ctx, false)?;
                std::fs::remove_file(&target)
                    .map_err(|e| format!("{}: 删除失败: {}", rel, e))?;
            }
            record_mutation(apply, rel, before.clone(), None);
            preview_files.push(WriteDiffFileState {
                rel_path: rel.clone(),
                before: before.clone(),
                after: None,
            });
            applied_files.push(format!("删除: {}", rel));
        }
    }
    Ok(())
}

/// 统一写盘入口：`dry_run` 直接返回；否则 `tmp → 继承元数据 → fsync → 校验 → rename`。
///
/// 走 `file::write_bytes_with_revision_guard` 而不是 `fs::write`：校验与写入之间的窗口
/// 更短，且并发读者不会看到写了一半的文件。
fn write_planned(apply: &PatchApply<'_>, rel: &str, content: &str) -> Result<(), String> {
    if apply.dry_run {
        return Ok(());
    }
    let target = apply.root.join(rel);
    file::write_bytes_with_revision_guard(&target, content.as_bytes(), apply.args, apply.ctx, false)
        .map_err(|e| format!("{}: {}", rel, e))
}

/// 新建文件前补父目录（与 `create_file` 一致：补丁不负责创建整条目录树的语义之外的东西）。
fn ensure_parent_dir(apply: &PatchApply<'_>, rel: &str) -> Result<(), String> {
    if apply.dry_run {
        return Ok(());
    }
    let target = apply.root.join(rel);
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("{}: 创建父目录失败: {}", rel, e))?;
    }
    Ok(())
}

/// 变更集记账：预览（`dry_run`）不得污染 changelist。
fn record_mutation(
    apply: &PatchApply<'_>,
    rel: &str,
    before: Option<String>,
    after: Option<String>,
) {
    if apply.dry_run {
        return;
    }
    if let Some(cl) = apply.changelist {
        cl.record_mutation(rel, before, after);
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
    let body = line
        .strip_prefix("--- ")
        .or_else(|| line.strip_prefix("+++ "))?;
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

#[cfg(test)]
mod tests_typed;
#[cfg(test)]
mod tests_write;
