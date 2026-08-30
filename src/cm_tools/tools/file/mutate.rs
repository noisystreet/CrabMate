//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use regex::RegexBuilder;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::path::{
    canonical_workspace_root, path_for_tool_display, resolve_for_read, resolve_for_write,
    tool_user_error_from_workspace_path,
};
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

fn parse_delete_dir_args(args_json: &str) -> Result<DeleteDirArgs, String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))
}

fn parse_append_file_args(args_json: &str) -> Result<AppendFileArgs, String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))
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

fn append_resolve_target_path(
    working_dir: &Path,
    path: &str,
    create_if_missing: bool,
) -> Result<PathBuf, String> {
    if create_if_missing {
        resolve_for_write(working_dir, path).map_err(tool_user_error_from_workspace_path)
    } else {
        match resolve_for_read(working_dir, path) {
            Ok(p) => Ok(p),
            Err(e) => Err(format!(
                "文件不存在（可设置 create_if_missing=true）：{}",
                e
            )),
        }
    }
}

fn append_create_parent_if_needed(
    base: &Path,
    create_if_missing: bool,
    target: &Path,
) -> Result<(), String> {
    if !create_if_missing {
        return Ok(());
    }
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        create_directory_under_root(base, parent, true)
            .map_err(|e| format!("创建父目录失败：{}", e))?;
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

fn append_file_prepare(
    args: AppendFileArgs,
    working_dir: &Path,
) -> Result<(bool, AppendFilePlan), String> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    let target = append_resolve_target_path(working_dir, &path, args.create_if_missing)?;
    let base =
        canonical_workspace_root(working_dir).map_err(tool_user_error_from_workspace_path)?;
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
        body,
        vec![WriteDiffFileState {
            rel_path: plan.path,
            before: plan.before,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

fn append_file_write(working_dir: &Path, ctx: &ToolContext<'_>, plan: AppendFilePlan) -> String {
    let mut file =
        match open_file_append_under_root(&plan.base, &plan.target, plan.create_if_missing) {
            Ok(f) => f,
            Err(e) => return format!("打开文件失败：{}", e),
        };
    if let Err(e) = file.write_all(plan.append_body.as_bytes()) {
        return format!("写入失败：{}", e);
    }
    let after = std::fs::read_to_string(&plan.target).ok();
    if let Some(c) = ctx.workspace_changelist {
        c.record_mutation(&plan.path, plan.before.clone(), after.clone());
    }
    let body = append_file_build_body("已追加", working_dir, &plan);
    format_tool_output_with_write_diff_preview(
        "append_file",
        body,
        vec![WriteDiffFileState {
            rel_path: plan.path,
            before: plan.before,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

/// 单次 `delete_files` 允许的最大文件数（防输出 / diff 预览爆炸）。
const DELETE_FILES_MAX_BATCH: usize = 32;

fn parse_delete_files_args(args_json: &str) -> Result<Vec<String>, String> {
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    let args: DeleteFilesArgs =
        serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))?;
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
    let args = match parse_append_file_args(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let (dry_run, plan) = match append_file_prepare(args, working_dir) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if dry_run {
        return append_file_preview(working_dir, plan);
    }
    append_file_write(working_dir, ctx, plan)
}

// ── create_dir ──────────────────────────────────────────────

pub fn create_dir(args_json: &str, working_dir: &Path) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: CreateDirArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {e}"),
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
            preview.push_str(&format!(
                "  L{}: \"{}\" → \"{}\"\n",
                i + 1,
                old.trim(),
                new.trim()
            ));
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
    let v = crate::cm_tools::tools::parse_args_json(args_json)?;
    serde_json::from_value(v).map_err(|e| format!("参数解析错误: {e}"))
}

fn search_replace_path_and_query(args: &SearchReplaceArgs) -> Result<(String, String), String> {
    let path = match args.path.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 path 参数".to_string()),
    };
    let search = match args.search.trim() {
        s if !s.is_empty() => s.to_string(),
        _ => return Err("缺少 search 参数".to_string()),
    };
    Ok((path, search))
}

fn load_search_replace_file_bytes(
    working_dir: &Path,
    path: &str,
) -> Result<(PathBuf, String), String> {
    let target =
        resolve_for_read(working_dir, path).map_err(tool_user_error_from_workspace_path)?;
    if !target.is_file() {
        return Err(format!("错误：{} 不是文件", path));
    }
    let content = std::fs::read_to_string(&target).map_err(|e| format!("读取文件失败：{}", e))?;
    const MAX_FILE_SIZE: usize = 4 * 1024 * 1024;
    if content.len() > MAX_FILE_SIZE {
        return Err(format!(
            "错误：文件过大（{} 字节，上限 4MiB）",
            content.len()
        ));
    }
    Ok((target, content))
}

pub fn search_replace(args_json: &str, working_dir: &Path, ctx: &ToolContext<'_>) -> String {
    let args = match parse_search_replace_args(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    let (path, search) = match search_replace_path_and_query(&args) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let replace = args.replace;
    let is_regex = args.regex;
    let dry_run = args.dry_run;
    let confirm = args.confirm;
    let max_replacements = args.max_replacements.unwrap_or(0) as usize;

    let (target, content) = match load_search_replace_file_bytes(working_dir, &path) {
        Ok(x) => x,
        Err(e) => return e,
    };

    let (new_content, count) =
        match apply_search_replace_inner(&content, &search, &replace, is_regex, max_replacements) {
            Ok(x) => x,
            Err(e) => return e,
        };

    if count == 0 {
        return format!("未找到匹配：\"{}\" 在 {}", search, path);
    }

    let display = path_for_tool_display(working_dir, &target, Some(&path));
    if dry_run {
        return search_replace_dry_run_preview(&display, count, &content, &new_content);
    }

    if !confirm {
        return "拒绝执行：search_replace 写盘需要 confirm=true".to_string();
    }

    let before = content.clone();
    let base = match canonical_workspace_root(working_dir) {
        Ok(p) => p,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
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
            format_tool_output_with_write_diff_preview(
                "search_replace",
                body,
                vec![WriteDiffFileState {
                    rel_path: path,
                    before: Some(before),
                    after: Some(new_content),
                }],
                WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
            )
        }
        Err(e) => format!("写入文件失败：{}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_config::WebSearchProvider;
    use std::fs;

    fn test_ctx(working_dir: &std::path::Path) -> ToolContext<'_> {
        ToolContext {
            cfg: None,
            codebase_semantic_host: None,
            command_max_output_len: 1 << 20,
            weather_timeout_secs: 5,
            allowed_commands: &[],
            working_dir,
            web_search_timeout_secs: 5,
            web_search_provider: WebSearchProvider::Worbrow,
            web_search_api_key: "",
            web_search_max_results: 3,
            http_fetch_allowed_prefixes: &[],
            http_fetch_timeout_secs: 5,
            http_fetch_max_response_bytes: 1024,
            command_timeout_secs: 5,
            read_file_turn_cache: None,
            workspace_changelist: None,
            test_result_cache_enabled: false,
            test_result_cache_max_entries: 0,
            long_term_memory_host: None,
        }
    }

    fn write(ws: &std::path::Path, rel: &str, content: &str) {
        let p = ws.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, content).unwrap();
    }

    #[test]
    fn delete_files_batch_success_dedup() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "A");
        write(tmp.path(), "b.txt", "B");
        let ctx = test_ctx(tmp.path());
        let out = delete_files(
            r#"{"paths":["a.txt","b.txt","a.txt"],"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(out.contains("已删除 2 个文件"), "{out}");
        assert!(!tmp.path().join("a.txt").exists());
        assert!(!tmp.path().join("b.txt").exists());
    }

    #[test]
    fn delete_files_requires_confirm() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "A");
        let ctx = test_ctx(tmp.path());
        let out = delete_files(r#"{"paths":["a.txt"]}"#, tmp.path(), &ctx);
        assert!(out.contains("需要 confirm=true"), "{out}");
        assert!(tmp.path().join("a.txt").exists());
    }

    #[test]
    fn delete_files_validation_rejects_whole_batch() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "A");
        let ctx = test_ctx(tmp.path());
        let out = delete_files(
            r#"{"paths":["a.txt","missing.txt"],"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        assert!(!out.contains("已删除"), "{out}");
        assert!(out.contains("路径无法解析") || out.contains("错误"), "{out}");
        assert!(
            tmp.path().join("a.txt").exists(),
            "任一路径非法时整批拒绝，不应删除任何文件"
        );
    }

    #[test]
    fn delete_files_rejects_directory() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join("d")).unwrap();
        let ctx = test_ctx(tmp.path());
        let out = delete_files(r#"{"paths":["d"],"confirm":true}"#, tmp.path(), &ctx);
        assert!(out.contains("不是文件"), "{out}");
        assert!(tmp.path().join("d").is_dir());
    }

    #[test]
    fn delete_files_caps_batch_size() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path());
        let paths: Vec<String> = (0..33).map(|i| format!("f{i}.txt")).collect();
        let args = serde_json::json!({ "paths": paths, "confirm": true }).to_string();
        let out = delete_files(&args, tmp.path(), &ctx);
        assert!(out.contains("最多删除 32"), "{out}");
    }

    #[test]
    fn delete_files_empty_paths_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let ctx = test_ctx(tmp.path());
        let out = delete_files(r#"{"paths":[],"confirm":true}"#, tmp.path(), &ctx);
        assert!(out.contains("缺少 paths"), "{out}");
    }

    /// 删除阶段遇个别失败（只读目录内的文件）时继续删完并汇总。
    #[cfg(unix)]
    #[test]
    fn delete_files_partial_failure_continues() {
        use std::os::unix::fs::PermissionsExt;
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "a.txt", "A");
        fs::create_dir(tmp.path().join("ro")).unwrap();
        write(tmp.path(), "ro/x.txt", "X");
        fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
        let ctx = test_ctx(tmp.path());
        let out = delete_files(
            r#"{"paths":["a.txt","ro/x.txt"],"confirm":true}"#,
            tmp.path(),
            &ctx,
        );
        fs::set_permissions(tmp.path().join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(out.contains("已删除 1 个文件"), "{out}");
        assert!(out.contains("删除失败（1 个）"), "{out}");
        assert!(!tmp.path().join("a.txt").exists());
        assert!(tmp.path().join("ro/x.txt").exists());
    }
}
