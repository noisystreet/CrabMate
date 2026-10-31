//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::io::Write;
use std::path::{Path, PathBuf};

use super::path::{
    PathAccess, canonical_workspace_root, path_for_tool_display,
    tool_user_error_from_workspace_path,
};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::tool_param_types::{
    AppendFileArgs, CreateDirArgs, DeleteDirArgs, DeleteFilesArgs,
};
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::fs::{
    create_directory_under_root, open_file_append_under_root, unlink_file_under_root,
};

use crate::cm_names as tool_names;

fn parse_delete_dir_args(args_json: &str) -> Result<DeleteDirArgs, String> {
    crate::cm_tools::tools::parse_args_typed(args_json)
}

fn parse_append_file_args(args_json: &str) -> Result<AppendFileArgs, String> {
    crate::cm_tools::tools::parse_args_typed(args_json)
}

#[allow(clippy::result_large_err)]
fn delete_dir_validate_target(
    access: &PathAccess<'_>,
    path: &str,
) -> Result<PathBuf, ToolError> {
    let working_dir = access.root();
    let target = access.for_read(path).map_err(ToolError::path_resolve_failed)?;
    if !target.is_dir() {
        return Err(ToolError::invalid_args(format!(
            "错误：{} 不是目录",
            path_for_tool_display(working_dir, &target, Some(path))
        )));
    }
    let base_canonical =
        canonical_workspace_root(working_dir).map_err(ToolError::path_resolve_failed)?;
    if target == base_canonical {
        return Err(ToolError::invalid_args(
            "错误：不能删除工作区根目录".to_string(),
        ));
    }
    Ok(target)
}

fn delete_dir_success_message(
    recursive: bool,
    working_dir: &Path,
    target: &Path,
    path: &str,
) -> String {
    format!(
        "已删除目录{}：{}",
        if recursive { "（递归）" } else { "" },
        path_for_tool_display(working_dir, target, Some(path))
    )
}

fn delete_dir_failure_message(recursive: bool, e: &std::io::Error) -> String {
    if !recursive && e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
        "删除失败：目录非空，需要 recursive=true 才能删除非空目录".to_string()
    } else {
        format!("删除目录失败：{}", e)
    }
}

#[allow(clippy::result_large_err)]
fn append_resolve_target_path(
    access: &PathAccess<'_>,
    path: &str,
    create_if_missing: bool,
) -> Result<PathBuf, ToolError> {
    if create_if_missing {
        access.for_write(path).map_err(ToolError::path_resolve_failed)
    } else {
        match access.for_read(path) {
            Ok(p) => Ok(p),
            // 越界 / 敏感目录 / 绝对路径等策略拒绝与「文件不存在」无关，须保留统一错误码。
            Err(e) if e.is_policy_denied() => Err(ToolError::path_resolve_failed(e)),
            Err(e) => Err(ToolError::invalid_args(format!(
                "文件不存在（可设置 create_if_missing=true）：{}",
                e
            ))),
        }
    }
}

#[allow(clippy::result_large_err)]
fn append_create_parent_if_needed(
    base: &Path,
    create_if_missing: bool,
    target: &Path,
) -> Result<(), ToolError> {
    if !create_if_missing {
        return Ok(());
    }
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        create_directory_under_root(base, parent, true).map_err(|e| {
            ToolError::external_code("append_file_failed", format!("创建父目录失败：{}", e))
        })?;
    }
    Ok(())
}

fn append_file_needs_leading_newline(
    before: Option<&str>,
    content: &str,
    ensure_leading_newline: bool,
) -> bool {
    ensure_leading_newline
        && !content.is_empty()
        && !content.starts_with('\n')
        && before.is_some_and(|s| !s.is_empty() && !s.ends_with('\n'))
}

struct AppendFilePlan {
    path: String,
    target: PathBuf,
    base: PathBuf,
    before: Option<String>,
    append_body: String,
    inserted_leading_newline: bool,
    create_if_missing: bool,
}

fn append_file_display(working_dir: &Path, plan: &AppendFilePlan) -> String {
    path_for_tool_display(working_dir, &plan.target, Some(&plan.path))
}

fn append_file_build_body(prefix: &str, working_dir: &Path, plan: &AppendFilePlan) -> String {
    let mut body = format!(
        "{} {} 字节到 {}",
        prefix,
        plan.append_body.len(),
        append_file_display(working_dir, plan)
    );
    if plan.inserted_leading_newline {
        body.push_str("\n已为原文件末尾补 1 个前导换行，避免新内容接在旧末行后。");
    }
    body
}

fn append_file_after_preview(plan: &AppendFilePlan) -> Option<String> {
    plan.before
        .as_ref()
        .map(|b| {
            let mut s = b.clone();
            s.push_str(&plan.append_body);
            s
        })
        .or_else(|| plan.create_if_missing.then(|| plan.append_body.clone()))
}

#[allow(clippy::result_large_err)]
fn append_file_prepare(
    args: AppendFileArgs,
    access: &PathAccess<'_>,
) -> Result<(bool, AppendFilePlan), ToolError> {
    let working_dir = access.root();
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err(ToolError::invalid_args("缺少 path 参数".to_string())),
    };
    let target = append_resolve_target_path(access, &path, args.create_if_missing)?;
    let base = canonical_workspace_root(working_dir).map_err(ToolError::path_resolve_failed)?;
    append_create_parent_if_needed(&base, args.create_if_missing, &target)?;
    let before = std::fs::read_to_string(&target).ok();
    let inserted_leading_newline = append_file_needs_leading_newline(
        before.as_deref(),
        &args.content,
        args.ensure_leading_newline,
    );
    let mut append_body = String::new();
    if inserted_leading_newline {
        append_body.push('\n');
    }
    append_body.push_str(&args.content);
    Ok((
        args.dry_run,
        AppendFilePlan {
            path,
            target,
            base,
            before,
            append_body,
            inserted_leading_newline,
            create_if_missing: args.create_if_missing,
        },
    ))
}

fn append_file_preview(working_dir: &Path, plan: AppendFilePlan) -> String {
    let body =
        append_file_build_body("预览（dry_run=true）：将追加", working_dir, &plan) + "，未写盘";
    let after = append_file_after_preview(&plan);
    format_tool_output_with_write_diff_preview(
        tool_names::APPEND_FILE,
        true,
        body,
        vec![WriteDiffFileState {
            rel_path: plan.path,
            before: plan.before,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

#[allow(clippy::result_large_err)]
fn append_file_write(
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    plan: AppendFilePlan,
) -> Result<String, ToolError> {
    let mut file = open_file_append_under_root(&plan.base, &plan.target, plan.create_if_missing)
        .map_err(|e| {
            ToolError::external_code("append_file_failed", format!("打开文件失败：{}", e))
        })?;
    file.write_all(plan.append_body.as_bytes())
        .map_err(|e| ToolError::external_code("append_file_failed", format!("写入失败：{}", e)))?;
    let after = std::fs::read_to_string(&plan.target).ok();
    if let Some(c) = ctx.workspace_changelist {
        c.record_mutation(&plan.path, plan.before.clone(), after.clone());
    }
    let body = append_file_build_body("已追加", working_dir, &plan);
    Ok(format_tool_output_with_write_diff_preview(
        tool_names::APPEND_FILE,
        true,
        body,
        vec![WriteDiffFileState {
            rel_path: plan.path,
            before: plan.before,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    ))
}

/// 单次 `delete_files` 允许的最大文件数（防输出 / diff 预览爆炸）。
const DELETE_FILES_MAX_BATCH: usize = 32;

fn parse_delete_files_args(args_json: &str) -> Result<Vec<String>, String> {
    let args: DeleteFilesArgs = crate::cm_tools::tools::parse_args_typed(args_json)?;
    if !args.confirm.unwrap_or(false) {
        return Err("拒绝执行：delete_files 需要 confirm=true".to_string());
    }
    let mut seen = std::collections::HashSet::with_capacity(args.paths.len());
    let mut paths = Vec::with_capacity(args.paths.len());
    for raw in args.paths {
        let p = raw.trim().to_string();
        if p.is_empty() {
            return Err("paths 中含空路径".to_string());
        }
        if seen.insert(p.clone()) {
            paths.push(p);
        }
    }
    if paths.is_empty() {
        return Err("缺少 paths 参数".to_string());
    }
    if paths.len() > DELETE_FILES_MAX_BATCH {
        return Err(format!(
            "一次最多删除 {DELETE_FILES_MAX_BATCH} 个文件（收到 {} 个）",
            paths.len()
        ));
    }
    Ok(paths)
}

fn delete_file_remove(working_dir: &Path, target: &Path) -> Result<(), String> {
    let base =
        canonical_workspace_root(working_dir).map_err(tool_user_error_from_workspace_path)?;
    #[cfg(unix)]
    {
        unlink_file_under_root(&base, target).map_err(|e| format!("删除文件失败：{}", e))
    }
    #[cfg(not(unix))]
    {
        let _ = base;
        std::fs::remove_file(target).map_err(|e| format!("删除文件失败：{}", e))
    }
}

/// 校验阶段：全部路径须可解析且在根内、且是文件；任一非法则整批拒绝、不产生部分删除。
#[allow(clippy::result_large_err)]
fn delete_files_validate(
    access: &PathAccess<'_>,
    paths: &[String],
) -> Result<Vec<PathBuf>, ToolError> {
    let working_dir = access.root();
    let mut targets = Vec::with_capacity(paths.len());
    for path in paths {
        targets.push(access.for_read(path).map_err(ToolError::path_resolve_failed)?);
    }
    for (path, target) in paths.iter().zip(&targets) {
        if !target.is_file() {
            return Err(ToolError::invalid_args(format!(
                "错误：{} 不是文件（可能是目录，请用 delete_dir）",
                path_for_tool_display(working_dir, target, Some(path))
            )));
        }
    }
    Ok(targets)
}

/// 批量删除结果：成功项（相对路径 + 删除前内容，供 diff 预览/变更集）与失败项（相对路径 + 原因）。
type DeleteFilesOutcome = (Vec<(String, Option<String>)>, Vec<(String, String)>);

/// 删除阶段：继续删完，逐文件汇总成功与失败。
fn delete_files_execute(
    working_dir: &Path,
    ctx: &ToolContext<'_>,
    paths: &[String],
    targets: &[PathBuf],
) -> DeleteFilesOutcome {
    let mut deleted: Vec<(String, Option<String>)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for (path, target) in paths.iter().zip(targets) {
        let before = std::fs::read_to_string(target).ok();
        match delete_file_remove(working_dir, target) {
            Ok(()) => {
                if let Some(c) = ctx.workspace_changelist {
                    c.record_mutation(path, before.clone(), None);
                }
                deleted.push((path.clone(), before));
            }
            Err(e) => failed.push((path.clone(), e)),
        }
    }
    (deleted, failed)
}

/// String 面：Typed 迁移的对照面，仅测试使用。
#[cfg(test)]
pub fn delete_files(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    delete_files_try(args_json, &PathAccess::new(working_dir), ctx).unwrap_or_else(|e| e.message)
}

/// 批量删除工作区内文件：先**整体校验**（任一非法则整批拒绝、不产生部分删除），
/// 再**逐个删除**并汇总成功/失败（删除阶段继续删完）。
#[allow(clippy::result_large_err)]
pub fn delete_files_try(
    args_json: &str,
    access: &PathAccess<'_>,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let working_dir = access.root();
    let paths = parse_delete_files_args(args_json).map_err(ToolError::invalid_args)?;
    let targets = delete_files_validate(access, &paths)?;
    let (deleted, failed) = delete_files_execute(working_dir, ctx, &paths, &targets);

    let mut body = format!("已删除 {} 个文件：", deleted.len());
    for (path, _) in &deleted {
        body.push_str(&format!("\n- {path}"));
    }
    if !failed.is_empty() {
        body.push_str(&format!("\n\n删除失败（{} 个）：", failed.len()));
        for (path, e) in &failed {
            body.push_str(&format!("\n- {path}: {e}"));
        }
    }

    let diff_states: Vec<WriteDiffFileState> = deleted
        .iter()
        .map(|(path, before)| WriteDiffFileState {
            rel_path: path.clone(),
            before: before.clone(),
            after: None,
        })
        .collect();
    let output = format_tool_output_with_write_diff_preview(
        tool_names::DELETE_FILES,
        failed.is_empty(),
        body,
        diff_states,
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    );
    if failed.is_empty() {
        Ok(output)
    } else {
        Err(ToolError::external_code("delete_files_failed", output))
    }
}

// ── delete_dir ──────────────────────────────────────────────

/// String 面：Typed 迁移的对照面，仅测试使用。
#[cfg(test)]
pub fn delete_dir(args_json: &str, working_dir: &Path) -> String {
    delete_dir_try(args_json, &PathAccess::new(working_dir)).unwrap_or_else(|e| e.message)
}

/// 删除工作区内目录（`recursive=false` 仅空目录；`confirm=true` 必填）。
#[allow(clippy::result_large_err)]
pub fn delete_dir_try(args_json: &str, access: &PathAccess<'_>) -> Result<String, ToolError> {
    let working_dir = access.root();
    let args = parse_delete_dir_args(args_json).map_err(ToolError::invalid_args)?;
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err(ToolError::invalid_args("缺少 path 参数".to_string())),
    };
    if !args.confirm.unwrap_or(false) {
        return Err(ToolError::invalid_args(
            "拒绝执行：delete_dir 需要 confirm=true".to_string(),
        ));
    }
    let recursive = args.recursive;

    let target = delete_dir_validate_target(access, &path)?;
    let result = if recursive {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_dir(&target)
    };
    match result {
        Ok(()) => Ok(delete_dir_success_message(
            recursive,
            working_dir,
            &target,
            &path,
        )),
        Err(e) => Err(ToolError::external_code(
            "delete_dir_failed",
            delete_dir_failure_message(recursive, &e),
        )),
    }
}

// ── append_file ─────────────────────────────────────────────

/// String 面：Typed 迁移的对照面，仅测试使用。
#[cfg(test)]
pub fn append_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    append_file_try(args_json, &PathAccess::new(working_dir), ctx).unwrap_or_else(|e| e.message)
}

/// 追加到文件末尾（`dry_run` 默认只预览；`create_if_missing` 可建新文件）。
#[allow(clippy::result_large_err)]
pub fn append_file_try(
    args_json: &str,
    access: &PathAccess<'_>,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let working_dir = access.root();
    let args = parse_append_file_args(args_json).map_err(ToolError::invalid_args)?;
    let skip_precheck = args.skip_precheck;
    let (dry_run, plan) = append_file_prepare(args, access)?;
    if dry_run {
        return Ok(append_file_preview(working_dir, plan));
    }
    // 校验「原内容 + 追加内容」的拼接结果（原文件二进制/不可读时退化为仅校验追加段）。
    let merged = format!(
        "{}{}",
        plan.before.as_deref().unwrap_or(""),
        plan.append_body
    );
    if let Err(e) = super::write_precheck::precheck_before_write(&plan.path, &merged, skip_precheck)
    {
        return Err(ToolError::write_precheck_failed(tool_names::APPEND_FILE, e));
    }
    append_file_write(working_dir, ctx, plan)
}

// ── create_dir ──────────────────────────────────────────────

/// String 面：Typed 迁移的对照面，仅测试使用。
#[cfg(test)]
pub fn create_dir(args_json: &str, working_dir: &Path) -> String {
    create_dir_try(args_json, &PathAccess::new(working_dir)).unwrap_or_else(|e| e.message)
}

/// 创建目录（`parents=true` 递归；已存在不报错）。
#[allow(clippy::result_large_err)]
pub fn create_dir_try(args_json: &str, access: &PathAccess<'_>) -> Result<String, ToolError> {
    let working_dir = access.root();
    let args: CreateDirArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err(ToolError::invalid_args("缺少 path 参数".to_string())),
    };
    let parents = args.parents;

    let target = access.for_write(&path).map_err(ToolError::path_resolve_failed)?;
    if target.exists() {
        if target.is_dir() {
            return Ok(format!(
                "目录已存在：{}",
                path_for_tool_display(working_dir, &target, Some(&path))
            ));
        }
        return Err(ToolError::invalid_args(format!(
            "错误：路径已存在且为文件：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        )));
    }
    let base = canonical_workspace_root(working_dir).map_err(ToolError::path_resolve_failed)?;
    match create_directory_under_root(&base, &target, parents) {
        Ok(()) => Ok(format!(
            "已创建目录：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        )),
        Err(e) => Err(ToolError::external_code(
            "create_dir_failed",
            format!("创建目录失败：{}", e),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{test_ctx, write};
    use super::*;
    use std::fs;

    // ── 写盘前校验（write_precheck）集成 ──────────────────────────

    /// create_file 创建非法 Python → 拒写；`skip_precheck` 后写盘。
    #[test]
    fn create_file_py_bad_indent_rejected_by_precheck() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path());
        let out = super::super::write_ops::create_file(
            r#"{"path":"bad.py","content":"def f():\nx = 1\n"}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("校验失败（PY_SYNTAX_ERROR）"), "{out}");
        assert!(!tmp.path().join("bad.py").exists(), "校验失败应拒写");

        let ok = super::super::write_ops::create_file(
            r#"{"path":"bad.py","content":"def f():\nx = 1\n","skip_precheck":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(!ok.contains("校验失败"), "{ok}");
        assert!(tmp.path().join("bad.py").exists());
    }

    /// append_file 追加出 Python 缩进错误 → 拒写。
    #[test]
    fn append_file_py_bad_indent_rejected_by_precheck() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    pass\n");
        let before = fs::read_to_string(tmp.path().join("app.py")).unwrap();
        let ctx = test_ctx(tmp.path());
        let out = append_file(
            r#"{"path":"app.py","content":"x = 1\nreturn 2\n","dry_run":false}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("校验失败（PY_SYNTAX_ERROR）"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("app.py")).unwrap(),
            before,
            "校验失败应拒写"
        );
    }
}
