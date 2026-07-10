//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::path::Path;

use crate::workspace::path::WorkspacePathError;

use super::path::{
    path_for_tool_display, resolve_for_read_open, tool_user_error_from_workspace_path,
};
use super::read_file_pipeline::{
    ReadFileBodyError, ReadFileLinesDispatch, ReadFileLinesResult, ReadFileLinesSpec,
    dispatch_read_file_lines, format_encoding_header, guard_read_file_count_total_size,
    maybe_count_total_lines_for_read, resolve_read_end_line, sniff_opened_file_encoding,
};

#[path = "read_tool_parse.rs"]
mod read_tool_parse;

use read_tool_parse::{ReadFileParsedArgs, parse_read_file_args};

/// 路径不存在或无法解析时追加，减少误猜 `foo.rs` / `src/lib.rs` 等布局。
const READ_FILE_NOT_FOUND_LAYOUT_HINT: &str = "\n\n提示：若路径不存在，常见于误猜 Rust 布局——模块多为 `…/模块名/mod.rs` 而非同级 `…/模块名.rs`；workspace 子 crate 入口可能在包根 `lib.rs`（或 `Cargo.toml` 的 `[lib] path`），未必有 `src/lib.rs`。请先用 read_dir / glob_files / list_tree 确认真实路径后再 read_file。";

fn read_file_missing_path_hint_eligible(e: &WorkspacePathError) -> bool {
    match e {
        WorkspacePathError::PathResolveFailed(io)
        | WorkspacePathError::WorkspacePathInvalid(io)
        | WorkspacePathError::NormalizationFailed(io) => io.kind() == std::io::ErrorKind::NotFound,
        WorkspacePathError::NoExistingAncestor => true,
        _ => false,
    }
}

fn read_file_workspace_tool_error(e: WorkspacePathError) -> crate::tool_result::ToolError {
    let code: &'static str = match e.kind() {
        "empty_path" => "read_file_workspace_empty_path",
        "absolute_path_not_allowed" => "read_file_workspace_absolute_path_not_allowed",
        "workspace_set_path_empty" => "read_file_workspace_set_path_empty",
        "current_dir_unavailable" => "read_file_workspace_current_dir_unavailable",
        "workspace_path_invalid" => "read_file_workspace_path_invalid",
        "path_resolve_failed" => "read_file_workspace_path_resolve_failed",
        "workspace_resolve_failed" => "read_file_workspace_resolve_failed",
        "web_effective_workspace_unset" => "read_file_workspace_web_unset",
        "not_a_directory" => "read_file_workspace_not_a_directory",
        "sensitive_path_denied" => "read_file_workspace_sensitive_path_denied",
        "effective_root_sensitive" => "read_file_workspace_effective_root_sensitive",
        "outside_allowed_roots" => "read_file_workspace_outside_allowed_roots",
        "effective_root_outside_allowed" => "read_file_workspace_effective_root_outside_allowed",
        "outside_workspace_root" => "read_file_workspace_outside_workspace_root",
        "path_normalize_failed" => "read_file_workspace_path_normalize_failed",
        "no_existing_ancestor" => "read_file_workspace_no_existing_ancestor",
        _ => "read_file_workspace_other",
    };
    crate::tool_result::ToolError::external_code(code, tool_user_error_from_workspace_path(e))
}

fn read_file_workspace_tool_error_maybe_hint(
    e: WorkspacePathError,
) -> crate::tool_result::ToolError {
    let attach = read_file_missing_path_hint_eligible(&e);
    let mut err = read_file_workspace_tool_error(e);
    if attach {
        err.message.push_str(READ_FILE_NOT_FOUND_LAYOUT_HINT);
    }
    err
}

fn prepend_read_file_output_header(body: &str, meta: &ReadFileOutputMeta<'_>) -> String {
    let path_disp = path_for_tool_display(meta.working_dir, meta.target, Some(meta.user_path));
    let header = serde_json::json!({
        "kind": "crabmate_tool_output",
        "tool": "read_file",
        "version": 1,
        "path": path_disp,
        "start_line": meta.start_line,
        "end_line_shown": meta.end_line_shown,
        "line_count_returned": meta.line_count_returned,
        "total_lines": meta.total_lines,
        "truncated_by_max_lines": meta.truncated_by_max_lines,
        "has_more": meta.has_more,
        "file_empty": meta.file_empty,
    });
    let line = header.to_string();
    format!("{}\n{}", line, body)
}

struct ReadFileOutputMeta<'a> {
    working_dir: &'a Path,
    target: &'a Path,
    user_path: &'a str,
    start_line: usize,
    end_line_shown: usize,
    line_count_returned: usize,
    total_lines: Option<usize>,
    truncated_by_max_lines: bool,
    has_more: bool,
    file_empty: bool,
}

fn read_file_logical_cache_key(canonical: &std::path::Path, v: &serde_json::Value) -> String {
    let mut start_line = v.get("start_line").and_then(|n| n.as_u64()).unwrap_or(1);
    let mut end_line_opt = v.get("end_line").and_then(|n| n.as_u64());
    if let Some(e) = end_line_opt.as_mut()
        && *e < start_line
    {
        std::mem::swap(&mut start_line, e);
    }
    let end_line = end_line_opt
        .map(|n| n.to_string())
        .unwrap_or_else(|| "none".to_string());
    let max_lines = v.get("max_lines").and_then(|n| n.as_u64()).unwrap_or(500);
    let count_total = v
        .get("count_total_lines")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);
    let enc = v
        .get("encoding")
        .and_then(|x| x.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "utf-8".to_string());
    format!(
        "{}|sl={}|el={}|ml={}|ct={}|enc={}",
        canonical.display(),
        start_line,
        end_line,
        max_lines,
        count_total,
        enc
    )
}

fn read_file_build_empty_response(
    working_dir: &Path,
    target: &Path,
    path: &str,
    start_line: usize,
) -> String {
    let body = format!(
        "文件为空: {}",
        path_for_tool_display(working_dir, target, Some(path))
    );
    prepend_read_file_output_header(
        &body,
        &ReadFileOutputMeta {
            working_dir,
            target,
            user_path: path,
            start_line,
            end_line_shown: 0,
            line_count_returned: 0,
            total_lines: Some(0),
            truncated_by_max_lines: false,
            has_more: false,
            file_empty: true,
        },
    )
}

/// 读取文件：按行**流式**读取，不把整文件载入内存。
///
/// - `max_lines`：单次最多返回行数（默认 500，上限 8000）。若未指定 `end_line`，则读到 `start_line + max_lines - 1` 或 EOF。
/// - 若同时指定 `end_line` 与 `max_lines`，实际返回行数不超过 `max_lines`；若区间更宽会截断并提示 `has_more`。
/// - `count_total_lines=true` 时会再扫描一遍文件统计总行数（大文件较慢）；超过 32MiB 会拒绝（见 `read_file_count_total_too_large`）。
/// - `anchor_line` + `context_lines`（可选，默认每侧 120 行）：以锚点行为中心对称取上下文，仍受 `max_lines` 封顶；适合 `search_in_files` / `codebase_semantic_search` 命中行号后直接精读。**不要**与 `start_line`/`end_line` 同传。
/// - 若同时指定 `end_line` 与 `start_line` 且 **end_line 小于 start_line**（模型偶发起止写反），**自动交换**后再读，与单轮缓存键一致。
#[allow(clippy::result_large_err)]
pub fn read_file_try(
    args_json: &str,
    working_dir: &Path,
    ctx: &super::super::ToolContext<'_>,
) -> Result<String, crate::tool_result::ToolError> {
    let v = crate::tools::parse_args_json(args_json)
        .map_err(crate::tool_result::ToolError::invalid_args)?;
    let ReadFileParsedArgs {
        path,
        enc_name,
        start_line,
        end_line_opt,
        max_lines,
        count_total,
    } = parse_read_file_args(&v)?;

    let opened = resolve_for_read_open(working_dir, &path)
        .map_err(read_file_workspace_tool_error_maybe_hint)?;
    if !opened.metadata.is_file() {
        let msg = if opened.metadata.is_dir() {
            "错误：路径指向目录而非文件，read_file 无法读取目录；请对该路径使用 read_dir，或读取目录内的具体文件（例如某个 .rs 文件或常见入口 mod.rs）。"
                .to_string()
        } else {
            "错误：路径不是文件或不存在，无法读取".to_string()
        };
        return Err(crate::tool_result::ToolError::external_code(
            "read_file_not_file",
            msg,
        ));
    }

    let target = opened.resolved_path;
    let meta = opened.metadata;
    let cache_key = read_file_logical_cache_key(&target, &v);
    if let Some(cache) = ctx.read_file_turn_cache {
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let len = meta.len();
        if let Some(hit) = cache.try_get(&cache_key, modified, len) {
            return Ok(hit);
        }
    }
    if meta.len() == 0 {
        let out = read_file_build_empty_response(working_dir, &target, path.as_str(), start_line);
        if let Some(cache) = ctx.read_file_turn_cache {
            let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
            cache.insert(cache_key, modified, meta.len(), out.clone());
        }
        return Ok(out);
    }

    guard_read_file_count_total_size(meta.len(), count_total)?;

    let (resolved, decode_note, file, head) = sniff_opened_file_encoding(opened.file, enc_name)?;
    let total_lines = maybe_count_total_lines_for_read(count_total, &target, enc_name)?;
    let (end_line, truncated_by_max) = resolve_read_end_line(start_line, end_line_opt, max_lines);
    let enc_header = format_encoding_header(&decode_note);
    let line_spec = ReadFileLinesSpec {
        start_line,
        end_line,
        max_lines,
        total_lines: total_lines.as_ref(),
        truncated_by_max,
    };

    let lines_result = dispatch_read_file_lines(
        resolved,
        ReadFileLinesDispatch {
            working_dir,
            target: &target,
            path: path.as_str(),
            enc_name,
            line_spec: &line_spec,
            enc_header: enc_header.as_str(),
        },
        file,
        head,
    )
    .map_err(ReadFileBodyError::into_tool_error)?;

    let ReadFileLinesResult {
        raw_body,
        end_line_shown,
        line_count_returned,
        has_more,
    } = lines_result;

    let out = prepend_read_file_output_header(
        &raw_body,
        &ReadFileOutputMeta {
            working_dir,
            target: &target,
            user_path: path.as_str(),
            start_line,
            end_line_shown,
            line_count_returned,
            total_lines,
            truncated_by_max_lines: truncated_by_max,
            has_more,
            file_empty: false,
        },
    );

    if let Some(cache) = ctx.read_file_turn_cache {
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        cache.insert(cache_key, modified, meta.len(), out.clone());
    }
    Ok(out)
}

pub fn read_file(
    args_json: &str,
    working_dir: &Path,
    ctx: &super::super::ToolContext<'_>,
) -> String {
    match read_file_try(args_json, working_dir, ctx) {
        Ok(s) => s,
        Err(e) => e.message,
    }
}

/// 单测断言用：剥离首行 `crabmate_tool_output` JSON（成功路径前缀）。
#[cfg(test)]
pub fn strip_read_file_output_header_for_tests(s: &str) -> &str {
    if s.starts_with("{\"kind\":\"crabmate_tool_output\",\"tool\":\"read_file\"")
        && let Some(idx) = s.find('\n')
    {
        return &s[idx + 1..];
    }
    s
}
