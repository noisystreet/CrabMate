//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Seek, SeekFrom};
use std::path::Path;

use crate::text_encoding::{
    DecodedFileNote, ResolvedTextEncoding, SNIFF_MAX_BYTES, TextEncodingName, count_decoded_lines,
    for_each_decoded_line_from_file_with_head, open_file_and_read_head,
    open_file_and_read_head_from, parse_text_encoding_name, resolve_text_encoding,
};

use super::path::{
    path_for_tool_display, resolve_for_read_open, tool_user_error_from_workspace_path,
};

fn read_file_logical_cache_key(canonical: &std::path::Path, v: &serde_json::Value) -> String {
    let start_line = v.get("start_line").and_then(|n| n.as_u64()).unwrap_or(1);
    let end_line = v
        .get("end_line")
        .and_then(|n| n.as_u64())
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

/// 自已打开文件读取嗅探前缀；返回 **未消费的** `File`（指针位于已读字节之后）与 `head`。
fn sniff_head_bytes_from(
    file: std::fs::File,
    enc_name: TextEncodingName,
) -> Result<(std::fs::File, Vec<u8>), String> {
    let cap = match enc_name {
        TextEncodingName::Utf8 => 0usize,
        TextEncodingName::Utf8Sig => 3,
        _ => SNIFF_MAX_BYTES,
    };
    if cap == 0 {
        return Ok((file, Vec::new()));
    }
    open_file_and_read_head_from(file, cap)
}

fn count_lines_for_read(path: &Path, enc_name: TextEncodingName) -> Result<usize, String> {
    let cap = match enc_name {
        TextEncodingName::Utf8 => 0usize,
        TextEncodingName::Utf8Sig => 3,
        _ => SNIFF_MAX_BYTES,
    };
    let head = if cap == 0 {
        Vec::new()
    } else {
        let (_f, h) = open_file_and_read_head(path, cap)?;
        h
    };
    let (resolved, _) = resolve_text_encoding(&head, enc_name)?;
    match resolved {
        ResolvedTextEncoding::Utf8Strict => count_lines_utf8_strict(path, 0),
        ResolvedTextEncoding::Utf8Sig { skip_bom } => count_lines_utf8_strict(path, skip_bom),
        ResolvedTextEncoding::Decoder { .. } => count_decoded_lines(path, enc_name),
    }
}

fn count_lines_utf8_strict(path: &Path, skip: usize) -> Result<usize, String> {
    let mut f = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    if skip > 0 {
        f.seek(SeekFrom::Start(skip as u64))
            .map_err(|e| format!("定位文件失败: {}", e))?;
    }
    let mut reader = BufReader::new(f);
    let mut count = 0usize;
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                return Err(
                    "按 UTF-8 统计行数失败：文件含非法字节序列。请指定 encoding（如 gb18030、big5）或使用 auto。"
                        .to_string(),
                );
            }
            Err(e) => return Err(format!("读取失败: {}", e)),
        };
        if n == 0 {
            break;
        }
        count += 1;
    }
    Ok(count)
}

fn format_encoding_header(note: &DecodedFileNote) -> String {
    if note.auto_detected {
        format!("文本编码: {}（自动探测）\n", note.label)
    } else {
        format!("文本编码: {}\n", note.label)
    }
}

/// 行区间与截断语义（`read_file_utf8_lines` / `read_file_decoded_lines` 共用）。
struct ReadFileLinesSpec<'a> {
    start_line: usize,
    end_line: usize,
    max_lines: usize,
    total_lines: Option<&'a usize>,
    truncated_by_max: bool,
}

/// [`assemble_read_output`] 入参。
struct AssembleReadOutputParams<'a> {
    working_dir: &'a Path,
    target: &'a Path,
    path: &'a str,
    collected: &'a [(usize, String)],
    start_line: usize,
    end_line: usize,
    max_lines: usize,
    total_lines: Option<&'a usize>,
    truncated_by_max: bool,
    has_more: bool,
    enc_header: &'a str,
}

/// 单次 read_file 默认最多返回的行数（防撑爆上下文）
const READ_FILE_DEFAULT_MAX_LINES: usize = 500;
/// read_file 允许的单次上限
const READ_FILE_ABS_MAX_LINES: usize = 8000;
/// 读取文件：按行**流式**读取，不把整文件载入内存。
///
/// - `max_lines`：单次最多返回行数（默认 500，上限 8000）。若未指定 `end_line`，则读到 `start_line + max_lines - 1` 或 EOF。
/// - 若同时指定 `end_line` 与 `max_lines`，实际返回行数不超过 `max_lines`；若区间更宽会截断并提示 `has_more`。
/// - `count_total_lines=true` 时会再扫描一遍文件统计总行数（大文件较慢）。
/// - `encoding`：可选 `utf-8`（默认，严格）、`utf-8-sig`、`gb18030`、`gbk`、`gb2312`、`big5`、`utf-16le`、`utf-16be`、`auto`（BOM 优先，否则嗅探）；非法序列返回明确错误。
pub fn read_file(
    args_json: &str,
    working_dir: &Path,
    ctx: &super::super::ToolContext<'_>,
) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return "缺少 path 参数".to_string(),
    };
    let enc_name = match parse_text_encoding_name(v.get("encoding").and_then(|x| x.as_str())) {
        Ok(n) => n,
        Err(e) => return e,
    };
    let start_line = match v.get("start_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => v as usize,
            _ => return "错误：start_line 必须是大于等于 1 的整数".to_string(),
        },
        None => 1usize,
    };
    let end_line_opt = match v.get("end_line") {
        Some(n) => match n.as_u64() {
            Some(v) if v >= 1 => Some(v as usize),
            _ => return "错误：end_line 必须是大于等于 1 的整数".to_string(),
        },
        None => None,
    };
    let max_lines = v
        .get("max_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(READ_FILE_DEFAULT_MAX_LINES)
        .min(READ_FILE_ABS_MAX_LINES);

    let count_total = v
        .get("count_total_lines")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    if let Some(e) = end_line_opt
        && e < start_line
    {
        return "错误：end_line 不能小于 start_line".to_string();
    }

    let opened = match resolve_for_read_open(working_dir, &path) {
        Ok(o) => o,
        Err(e) => return tool_user_error_from_workspace_path(e),
    };
    if !opened.metadata.is_file() {
        return "错误：路径不是文件或不存在，无法读取".to_string();
    }

    let target = opened.resolved_path;
    let meta = opened.metadata;
    let cache_key = read_file_logical_cache_key(&target, &v);
    if let Some(cache) = ctx.read_file_turn_cache {
        let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        let len = meta.len();
        if let Some(hit) = cache.try_get(&cache_key, modified, len) {
            return hit;
        }
    }
    if meta.len() == 0 {
        return format!(
            "文件为空: {}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
    }

    let (file, head) = match sniff_head_bytes_from(opened.file, enc_name) {
        Ok(x) => x,
        Err(e) => return e,
    };
    let (resolved, decode_note) = match resolve_text_encoding(&head, enc_name) {
        Ok(x) => x,
        Err(e) => return e,
    };

    let total_lines = if count_total {
        match count_lines_for_read(&target, enc_name) {
            Ok(n) => Some(n),
            Err(e) => return e,
        }
    } else {
        None
    };

    let mut end_line = match end_line_opt {
        Some(e) => e,
        None => start_line.saturating_add(max_lines.saturating_sub(1)),
    };
    let allowed_span = max_lines.saturating_sub(1);
    let max_end_by_cap = start_line.saturating_add(allowed_span);
    let truncated_by_max = end_line > max_end_by_cap;
    if truncated_by_max {
        end_line = max_end_by_cap;
    }

    let enc_header = format_encoding_header(&decode_note);

    let line_spec = ReadFileLinesSpec {
        start_line,
        end_line,
        max_lines,
        total_lines: total_lines.as_ref(),
        truncated_by_max,
    };
    let body = match resolved {
        ResolvedTextEncoding::Utf8Strict => read_file_utf8_lines(
            working_dir,
            &target,
            &path,
            &line_spec,
            0,
            &enc_header,
            file,
        ),
        ResolvedTextEncoding::Utf8Sig { skip_bom } => read_file_utf8_lines(
            working_dir,
            &target,
            &path,
            &line_spec,
            skip_bom,
            &enc_header,
            file,
        ),
        ResolvedTextEncoding::Decoder { .. } => read_file_decoded_lines(
            working_dir,
            &target,
            &path,
            enc_name,
            &line_spec,
            &enc_header,
            file,
            head,
        ),
    };

    match body {
        Ok(out) => {
            if let Some(cache) = ctx.read_file_turn_cache {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                cache.insert(cache_key, modified, meta.len(), out.clone());
            }
            out
        }
        Err(e) => e,
    }
}

fn read_file_utf8_lines(
    working_dir: &Path,
    target: &Path,
    path: &str,
    spec: &ReadFileLinesSpec<'_>,
    skip_bom: usize,
    enc_header: &str,
    mut f: File,
) -> Result<String, String> {
    let ReadFileLinesSpec {
        start_line,
        end_line,
        max_lines,
        total_lines,
        truncated_by_max,
    } = *spec;
    f.seek(SeekFrom::Start(skip_bom as u64))
        .map_err(|e| format!("定位文件失败: {}", e))?;
    let mut reader = BufReader::new(f);
    let mut buf = String::new();
    let mut line_no: usize = 0;
    let mut collected: Vec<(usize, String)> = Vec::new();
    let mut eof_before_start = false;

    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                return Err(
                    "读取失败：按 UTF-8 解码时遇到非法字节序列。请使用 encoding=gb18030、big5、utf-8-sig 或 auto 等重试。"
                        .to_string(),
                );
            }
            Err(e) => return Err(format!("读取文件失败: {}", e)),
        };
        if n == 0 {
            if line_no < start_line {
                eof_before_start = true;
            }
            break;
        }
        line_no += 1;
        if line_no < start_line {
            continue;
        }
        if line_no > end_line {
            break;
        }
        collected.push((line_no, buf.clone()));
        if collected.len() >= max_lines {
            break;
        }
    }

    if eof_before_start {
        let hint = total_lines
            .map(|t| t.to_string())
            .unwrap_or_else(|| "未知（未请求 count_total_lines）".to_string());
        return Err(format!(
            "错误：start_line={} 超出文件行数（已知总行数: {}）",
            start_line, hint
        ));
    }

    let mut has_more = false;
    if line_no > end_line {
        has_more = true;
    } else {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(n) if n > 0 => has_more = true,
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                return Err(
                    "读取失败：按 UTF-8 解码时遇到非法字节序列。请使用 encoding=gb18030、big5 或 auto 等重试。"
                        .to_string(),
                );
            }
            Err(e) => return Err(format!("读取文件失败: {}", e)),
            _ => {}
        }
    }

    if collected.is_empty() {
        return Err(format!(
            "错误：未读取到任何行（start_line={}，end_line={}）。请检查区间。",
            start_line, end_line
        ));
    }

    Ok(assemble_read_output(AssembleReadOutputParams {
        working_dir,
        target,
        path,
        collected: &collected,
        start_line,
        end_line,
        max_lines,
        total_lines,
        truncated_by_max,
        has_more,
        enc_header,
    }))
}

#[allow(clippy::too_many_arguments)] // 与 UTF-8 路径分支共享上层 `read_file` 管线，参数略多
fn read_file_decoded_lines(
    working_dir: &Path,
    target: &Path,
    path: &str,
    enc_name: TextEncodingName,
    spec: &ReadFileLinesSpec<'_>,
    enc_header: &str,
    file: File,
    head: Vec<u8>,
) -> Result<String, String> {
    let ReadFileLinesSpec {
        start_line,
        end_line,
        max_lines,
        total_lines,
        truncated_by_max,
    } = *spec;
    let mut collected: Vec<(usize, String)> = Vec::new();
    let mut last_line_no = 0usize;
    let mut eof_before_start = false;
    let mut has_more = false;
    // `head` 与 `file` 当前偏移与 `read_file` 嗅探阶段一致，避免再次按路径 `open`。
    let (resolved, _) = resolve_text_encoding(&head, enc_name)?;
    let ResolvedTextEncoding::Decoder { .. } = resolved else {
        return Err("内部错误：read_file_decoded_lines 需要解码器路径".to_string());
    };
    for_each_decoded_line_from_file_with_head(file, head, enc_name, |ln, line| {
        last_line_no = ln;
        if ln < start_line {
            return std::ops::ControlFlow::Continue(());
        }
        if ln > end_line {
            has_more = true;
            return std::ops::ControlFlow::Break(());
        }
        if ln >= start_line && ln <= end_line {
            if collected.len() < max_lines {
                collected.push((ln, format!("{}\n", line)));
            }
            if collected.len() >= max_lines && ln < end_line {
                has_more = true;
                return std::ops::ControlFlow::Break(());
            }
        }
        std::ops::ControlFlow::Continue(())
    })?;

    if last_line_no < start_line && collected.is_empty() {
        eof_before_start = true;
    }

    if eof_before_start {
        let hint = total_lines
            .map(|t| t.to_string())
            .unwrap_or_else(|| "未知（未请求 count_total_lines）".to_string());
        return Err(format!(
            "错误：start_line={} 超出文件行数（已知总行数: {}）",
            start_line, hint
        ));
    }

    if collected.is_empty() {
        return Err(format!(
            "错误：未读取到任何行（start_line={}，end_line={}）。请检查区间。",
            start_line, end_line
        ));
    }

    Ok(assemble_read_output(AssembleReadOutputParams {
        working_dir,
        target,
        path,
        collected: &collected,
        start_line,
        end_line,
        max_lines,
        total_lines,
        truncated_by_max,
        has_more,
        enc_header,
    }))
}

fn assemble_read_output(p: AssembleReadOutputParams<'_>) -> String {
    let AssembleReadOutputParams {
        working_dir,
        target,
        path,
        collected,
        start_line,
        end_line: _end_line,
        max_lines,
        total_lines,
        truncated_by_max,
        has_more,
        enc_header,
    } = p;
    let last_shown = collected.last().map(|(l, _)| *l).unwrap_or(start_line);
    let mut out = String::new();
    out.push_str(enc_header);
    out.push_str(&format!(
        "文件: {}\n",
        path_for_tool_display(working_dir, target, Some(path))
    ));
    if let Some(t) = total_lines {
        out.push_str(&format!("总行数: {}\n", t));
    } else {
        out.push_str("总行数: 未统计（大文件可避免 count_total_lines 以省时间）\n");
    }
    out.push_str(&format!(
        "本段行范围: {}-{}（单次 max_lines={}）\n",
        if collected.is_empty() {
            start_line
        } else {
            collected[0].0
        },
        last_shown,
        max_lines
    ));
    if truncated_by_max {
        out.push_str("说明: 请求的 end_line 区间超过 max_lines，已截断本段输出。\n");
    }
    if has_more {
        out.push_str(&format!(
            "仍有后续内容: 下一段可将 start_line 设为 {}\n",
            last_shown.saturating_add(1)
        ));
    } else {
        out.push_str("已读到文件末尾（本段范围内无更多行）。\n");
    }
    out.push('\n');
    for (idx, line) in collected {
        out.push_str(&format!(
            "{}|{}\n",
            idx,
            line.trim_end_matches(['\n', '\r'])
        ));
    }
    out.trim_end().to_string()
}
