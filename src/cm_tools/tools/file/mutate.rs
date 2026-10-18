//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::RegexBuilder;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::path::{
    canonical_workspace_root, path_for_tool_display, resolve_for_read, resolve_for_write,
    tool_user_error_from_workspace_path,
};
use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::tool_param_types::{
    AppendFileArgs, CreateDirArgs, DeleteDirArgs, DeleteFilesArgs, SearchReplaceArgs,
};
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;
use crate::cm_tools::workspace::fs::{
    create_directory_under_root, open_file_append_under_root, unlink_file_under_root,
    write_bytes_under_root,
};

use super::write_precheck::NOT_WRITTEN_HINT;

fn parse_delete_dir_args(args_json: &str) -> Result<DeleteDirArgs, String> {
    crate::cm_tools::tools::parse_args_typed(args_json)
}

fn parse_append_file_args(args_json: &str) -> Result<AppendFileArgs, String> {
    crate::cm_tools::tools::parse_args_typed(args_json)
}

fn delete_dir_validate_target(working_dir: &Path, path: &str) -> Result<PathBuf, String> {
    let target =
        resolve_for_read(working_dir, path).map_err(tool_user_error_from_workspace_path)?;
    if !target.is_dir() {
        return Err(format!(
            "错误：{} 不是目录",
            path_for_tool_display(working_dir, &target, Some(path))
        ));
    }
    let base_canonical =
        canonical_workspace_root(working_dir).map_err(tool_user_error_from_workspace_path)?;
    if target == base_canonical {
        return Err("错误：不能删除工作区根目录".to_string());
    }
    Ok(target)
}

fn delete_dir_result_message(
    recursive: bool,
    working_dir: &Path,
    target: &Path,
    path: &str,
    result: std::io::Result<()>,
) -> String {
    match result {
        Ok(()) => format!(
            "已删除目录{}：{}",
            if recursive { "（递归）" } else { "" },
            path_for_tool_display(working_dir, target, Some(path))
        ),
        Err(e) => {
            if !recursive && e.kind() == std::io::ErrorKind::DirectoryNotEmpty {
                "删除失败：目录非空，需要 recursive=true 才能删除非空目录".to_string()
            } else {
                format!("删除目录失败：{}", e)
            }
        }
    }
}

#[allow(clippy::result_large_err)]
fn append_resolve_target_path(
    working_dir: &Path,
    path: &str,
    create_if_missing: bool,
) -> Result<PathBuf, ToolError> {
    if create_if_missing {
        resolve_for_write(working_dir, path).map_err(ToolError::path_resolve_failed)
    } else {
        match resolve_for_read(working_dir, path) {
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
    working_dir: &Path,
) -> Result<(bool, AppendFilePlan), ToolError> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err(ToolError::invalid_args("缺少 path 参数".to_string())),
    };
    let target = append_resolve_target_path(working_dir, &path, args.create_if_missing)?;
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
        "append_file",
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
        "append_file",
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
fn delete_files_validate(working_dir: &Path, paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut targets = Vec::with_capacity(paths.len());
    for path in paths {
        match resolve_for_read(working_dir, path) {
            Ok(p) => targets.push(p),
            Err(e) => return Err(tool_user_error_from_workspace_path(e)),
        }
    }
    for (path, target) in paths.iter().zip(&targets) {
        if !target.is_file() {
            return Err(format!(
                "错误：{} 不是文件（可能是目录，请用 delete_dir）",
                path_for_tool_display(working_dir, target, Some(path))
            ));
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

/// 批量删除工作区内文件：先**整体校验**（任一非法则整批拒绝、不产生部分删除），
/// 再**逐个删除**并汇总成功/失败（删除阶段继续删完）。
pub fn delete_files(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let paths = match parse_delete_files_args(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let targets = match delete_files_validate(working_dir, &paths) {
        Ok(t) => t,
        Err(e) => return e,
    };
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
    format_tool_output_with_write_diff_preview(
        "delete_files",
        failed.is_empty(),
        body,
        diff_states,
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

// ── delete_dir ──────────────────────────────────────────────

pub fn delete_dir(args_json: &str, working_dir: &Path) -> String {
    let args = match parse_delete_dir_args(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let confirm = args.confirm.unwrap_or(false);
    if !confirm {
        return "拒绝执行：delete_dir 需要 confirm=true".to_string();
    }
    let recursive = args.recursive;

    let target = match delete_dir_validate_target(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };

    let result = if recursive {
        std::fs::remove_dir_all(&target)
    } else {
        std::fs::remove_dir(&target)
    };
    delete_dir_result_message(recursive, working_dir, &target, &path, result)
}

// ── append_file ─────────────────────────────────────────────

pub fn append_file(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    append_file_try(args_json, working_dir, ctx).unwrap_or_else(|e| e.message)
}

/// [`append_file`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn append_file_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let args = parse_append_file_args(args_json).map_err(ToolError::invalid_args)?;
    let skip_precheck = args.skip_precheck;
    let (dry_run, plan) = append_file_prepare(args, working_dir)?;
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
        return Err(ToolError::write_precheck_failed("append_file", e));
    }
    append_file_write(working_dir, ctx, plan)
}

// ── create_dir ──────────────────────────────────────────────

pub fn create_dir(args_json: &str, working_dir: &Path) -> String {
    let args: CreateDirArgs = match crate::cm_tools::tools::parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let parents = args.parents;

    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if target.exists() {
        if target.is_dir() {
            return format!(
                "目录已存在：{}",
                path_for_tool_display(working_dir, &target, Some(&path))
            );
        }
        return format!(
            "错误：路径已存在且为文件：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }
    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    let result = create_directory_under_root(&base, &target, parents);
    match result {
        Ok(()) => format!(
            "已创建目录：{}",
            path_for_tool_display(working_dir, &target, Some(&path))
        ),
        Err(e) => format!("创建目录失败：{}", e),
    }
}

// ── search_replace ──────────────────────────────────────────

fn apply_search_replace_inner(
    content: &str,
    search: &str,
    replace: &str,
    is_regex: bool,
    max_replacements: usize,
) -> Result<(String, usize), String> {
    if is_regex {
        let re = RegexBuilder::new(search)
            .build()
            .map_err(|e| format!("正则表达式无效：{}", e))?;
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            let result = re.replace_all(content, replace);
            count = re.find_iter(content).count();
            result.to_string()
        } else {
            let mut result = content.to_string();
            for _ in 0..max_replacements {
                if let Some(m) = re.find(&result) {
                    let before = &result[..m.start()];
                    let after = &result[m.end()..];
                    result = format!("{}{}{}", before, replace, after);
                    count += 1;
                } else {
                    break;
                }
            }
            result
        };
        Ok((new, count))
    } else {
        let mut count = 0usize;
        let new = if max_replacements == 0 {
            count = content.matches(search).count();
            content.replace(search, replace)
        } else {
            let mut result = content.to_string();
            for _ in 0..max_replacements {
                if let Some(pos) = result.find(search) {
                    result = format!(
                        "{}{}{}",
                        &result[..pos],
                        replace,
                        &result[pos + search.len()..]
                    );
                    count += 1;
                } else {
                    break;
                }
            }
            result
        };
        Ok((new, count))
    }
}

fn search_replace_dry_run_preview(
    display: &str,
    count: usize,
    content: &str,
    new_content: &str,
) -> String {
    let mut preview = format!("预览（dry-run）：在 {} 中找到 {} 处匹配\n", display, count);
    let lines: Vec<&str> = new_content.lines().collect();
    let orig_lines: Vec<&str> = content.lines().collect();
    let mut shown = 0usize;
    for (i, (old, new)) in orig_lines.iter().zip(lines.iter()).enumerate() {
        if old != new && shown < 20 {
            // 保留原始前导/尾随空白：缩进敏感语言（Python）下 `trim()` 会让缩进差异不可见、误导判断。
            preview.push_str(&format!("  L{}: \"{}\" → \"{}\"\n", i + 1, old, new));
            shown += 1;
        }
    }
    if shown >= 20 {
        preview.push_str("  ... (更多变更已省略)\n");
    }
    preview.push_str("\n设置 dry_run=false, confirm=true 以实际写入");
    preview
}

fn parse_search_replace_args(args_json: &str) -> Result<SearchReplaceArgs, String> {
    crate::cm_tools::tools::parse_args_typed(args_json)
}

fn search_replace_path_and_query(args: &SearchReplaceArgs) -> Result<(String, String), String> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    // 仅用 `trim()` 做空值校验，**保留 `search` 原文**（含首行前导空白）。
    // 若对 `search` 取 `trim()`，会剥掉首行缩进：锚点变短后按子串替换，文件原缩进前缀与
    // `replace` 自带缩进叠加 → 写出「缩进翻倍」的坏文件（Python 等缩进敏感语言尤甚）。
    if args.search.trim().is_empty() {
        return Err("缺少 search 参数".to_string());
    }
    Ok((path, args.search.clone()))
}

/// 写盘前闸门：`confirm` 确认 + 语法预检（失败返回 `校验失败（CODE）…` 拒写消息）。
#[allow(clippy::result_large_err)]
fn search_replace_confirm_and_precheck(
    confirm: bool,
    skip_precheck: bool,
    path: &str,
    new_content: &str,
) -> Result<(), ToolError> {
    if !confirm {
        return Err(ToolError::invalid_args(format!(
            "拒绝执行：search_replace 写盘需要 confirm=true（{NOT_WRITTEN_HINT}）"
        )));
    }
    super::write_precheck::precheck_before_write(path, new_content, skip_precheck)
        .map_err(|e| ToolError::write_precheck_failed("search_replace", e))
}

#[allow(clippy::result_large_err)]
fn load_search_replace_file_bytes(
    working_dir: &Path,
    path: &str,
) -> Result<(PathBuf, String), ToolError> {
    let target = resolve_for_read(working_dir, path).map_err(ToolError::path_resolve_failed)?;
    if !target.is_file() {
        return Err(ToolError::invalid_args(format!("错误：{} 不是文件", path)));
    }
    let content = std::fs::read_to_string(&target).map_err(|e| {
        ToolError::external_code("search_replace_failed", format!("读取文件失败：{}", e))
    })?;
    const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
    if content.len() > MAX_FILE_SIZE {
        return Err(ToolError::invalid_args(format!(
            "错误：文件过大（{} 字节，上限 4MiB）",
            content.len()
        )));
    }
    Ok((target, content))
}

pub fn search_replace(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    search_replace_try(args_json, working_dir, ctx).unwrap_or_else(|e| e.message)
}

/// [`search_replace`] 的显式失败版本（成功正文与 String 面逐字节一致）。
#[allow(clippy::result_large_err)]
pub fn search_replace_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &ToolContext<'_>,
) -> Result<String, ToolError> {
    let args = parse_search_replace_args(args_json).map_err(ToolError::invalid_args)?;
    let (path, search) = search_replace_path_and_query(&args).map_err(ToolError::invalid_args)?;
    let replace = args.replace;
    let is_regex = args.regex;
    let dry_run = args.dry_run;
    let confirm = args.confirm;
    let max_replacements = args.max_replacements.unwrap_or(0) as usize;

    let (target, content) = load_search_replace_file_bytes(working_dir, &path)?;

    let (new_content, count) =
        apply_search_replace_inner(&content, &search, &replace, is_regex, max_replacements)
            .map_err(ToolError::invalid_args)?;

    if count == 0 {
        return Err(ToolError::invalid_args(format!(
            "未找到匹配：\"{}\" 在 {}",
            search, path
        )));
    }

    let display = path_for_tool_display(working_dir, &target, Some(&path));
    if dry_run {
        return Ok(search_replace_dry_run_preview(
            &display,
            count,
            &content,
            &new_content,
        ));
    }

    search_replace_confirm_and_precheck(confirm, args.skip_precheck, &path, &new_content)?;

    let before = content.clone();
    let base = canonical_workspace_root(working_dir).map_err(ToolError::path_resolve_failed)?;
    match write_bytes_under_root(&base, &target, new_content.as_bytes(), false, false) {
        Ok(()) => {
            record_file_state_after_write(
                ctx.workspace_changelist,
                working_dir,
                &path,
                Some(before.clone()),
            );
            let body = format!(
                "已替换 {} 处匹配（\"{}\" → \"{}\"）：{}",
                count, search, replace, display
            );
            Ok(format_tool_output_with_write_diff_preview(
                "search_replace",
                true,
                body,
                vec![WriteDiffFileState {
                    rel_path: path,
                    before: Some(before),
                    after: Some(new_content),
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            ))
        }
        Err(e) => Err(ToolError::external_code(
            "search_replace_failed",
            format!("写入文件失败：{}", e),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::super::test_support::{test_ctx, write};
    use super::*;
    use std::fs;

    // ── 写盘前校验（write_precheck）集成 ──────────────────────────

    /// search_replace 替换出 Python 缩进错误 → 拒写（写盘前校验），原文件不变。
    #[test]
    fn search_replace_py_bad_indent_rejected_by_precheck() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    return 1\n");
        let before = fs::read_to_string(tmp.path().join("app.py")).unwrap();
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"    return 1","replace":"x = 1\nreturn 2","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("校验失败（PY_SYNTAX_ERROR）"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("app.py")).unwrap(),
            before,
            "校验失败应拒写，文件不得被修改"
        );
    }

    /// search_replace `skip_precheck=true` 绕过校验并写盘。
    #[test]
    fn search_replace_skip_precheck_writes() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    return 1\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"return 1","replace":"x = 1","dry_run":false,"confirm":true,"skip_precheck":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(!out.contains("校验失败"), "{out}");
        assert!(out.contains("已替换"), "{out}");
        assert!(fs::read_to_string(tmp.path().join("app.py"))
            .unwrap()
            .contains("x = 1"));
    }

    /// 缩进敏感回归：`search` 首行带缩进时按字面量替换，不得出现「缩进翻倍」。
    /// 修复前 `search` 被 `trim()` 剥掉首行缩进，与 `replace` 自带缩进叠加，且叠加后语法常仍合法（`write_precheck` 拦不住）。
    /// 另外 `replace` 只改缩进层级也可精确生效（`search` 不再被剥掉前导空白）。
    #[test]
    fn search_replace_keeps_indentation_of_indented_anchor() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "app.py", "def f():\n    if x:\n        return 1\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"app.py","search":"        return 1","replace":"        return 2","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("app.py")).unwrap(),
            "def f():\n    if x:\n        return 2\n",
            "应保持原缩进（8 空格），而非翻倍"
        );
    }

    /// 复现：成功正文内嵌 search/replace 原文；若原文含「失败」/「超时」，
    /// 不应被工具结果状态启发式误判为失败（否则报错但文件已写入）。
    #[test]
    fn search_replace_success_with_failure_word_in_body_is_ok() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "old\n");
        let ctx = test_ctx(tmp.path());
        let out = search_replace(
            r#"{"path":"a.txt","search":"old","replace":"写入文件失败：x","dry_run":false,"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已替换"), "{out}");
        assert_eq!(
            fs::read_to_string(tmp.path().join("a.txt")).unwrap(),
            "写入文件失败：x\n",
            "写盘应已成功"
        );
        let r = crate::cm_tools::tool_result::ToolResult::from_legacy_output("search_replace", out);
        assert!(
            r.ok,
            "文件已成功写入，工具结果不应判失败: error_code={:?}",
            r.error_code
        );
    }

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
            r#"{"path":"app.py","content":"x = 1\nreturn 2\n"}"#,
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
