//! `read_file` 正文嗅探、总行数统计与按行解码读取（从 `read_tool` 拆分以降低单文件规模）。
#![allow(clippy::manual_string_new)]

use std::fs::File;
use std::io::{BufRead, BufReader, ErrorKind, Seek, SeekFrom};
use std::path::Path;

use crate::text_encoding::{
    DecodedFileNote, ResolvedTextEncoding, SNIFF_MAX_BYTES, TextEncodingName, count_decoded_lines,
    for_each_decoded_line_from_file_with_head, open_file_and_read_head,
    open_file_and_read_head_from, resolve_text_encoding,
};

use super::path::path_for_tool_display;

/// `count_total_lines=true` 时允许的最大文件字节数（避免整文件多次扫描拖垮回合）。
pub(super) const READ_FILE_COUNT_TOTAL_MAX_BYTES: u64 = 32 * 1024 * 1024;

/// 自已打开文件读取嗅探前缀；返回 **未消费的** `File`（指针位于已读字节之后）与 `head`。
pub(super) fn sniff_head_bytes_from(
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

pub(super) fn count_lines_for_read(
    path: &Path,
    enc_name: TextEncodingName,
) -> Result<usize, String> {
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

pub(super) fn format_encoding_header(note: &DecodedFileNote) -> String {
    if note.auto_detected {
        format!("文本编码: {}（自动探测）\n", note.label)
    } else {
        format!("文本编码: {}\n", note.label)
    }
}

/// 行区间与截断语义（`read_file_utf8_lines` / `read_file_decoded_lines` 共用）。
pub(super) struct ReadFileLinesSpec<'a> {
    pub(super) start_line: usize,
    pub(super) end_line: usize,
    pub(super) max_lines: usize,
    pub(super) total_lines: Option<&'a usize>,
    pub(super) truncated_by_max: bool,
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

/// `read_file` 正文读取阶段的结构化错误（映射为稳定 `ToolError.code`）。
pub(super) enum ReadFileBodyError {
    Utf8Decode(String),
    InvalidRange(String),
    Internal(String),
    Io(String),
}

impl ReadFileBodyError {
    pub(super) fn into_tool_error(self) -> crate::tool_result::ToolError {
        match self {
            Self::Utf8Decode(msg) => {
                crate::tool_result::ToolError::external_code("read_file_utf8_decode", msg)
            }
            Self::InvalidRange(msg) => {
                crate::tool_result::ToolError::external_code("read_file_invalid_range", msg)
            }
            Self::Internal(msg) => {
                crate::tool_result::ToolError::internal_code("read_file_internal", msg)
            }
            Self::Io(msg) => crate::tool_result::ToolError::external_code("read_file_io", msg),
        }
    }
}

/// 正文读取管线成功结果（避免扫 `raw_body` 推断行号与 `has_more`）。
pub(super) struct ReadFileLinesResult {
    pub(super) raw_body: String,
    pub(super) end_line_shown: usize,
    pub(super) line_count_returned: usize,
    pub(super) has_more: bool,
}

/// 将仍返回 `String` 的下层（如 `text_encoding`）错误归为结构化类别；**唯一**依赖启发式子串之处。
fn read_file_body_error_from_pipeline_string(message: String) -> ReadFileBodyError {
    if message.contains("内部错误") {
        ReadFileBodyError::Internal(message)
    } else if message.starts_with("错误：start_line=")
        || message.starts_with("错误：未读取到任何行")
    {
        ReadFileBodyError::InvalidRange(message)
    } else if message.contains("UTF-8")
        || message.contains("非法字节")
        || message.contains("解码")
        || message.contains("非法序列")
    {
        ReadFileBodyError::Utf8Decode(message)
    } else {
        ReadFileBodyError::Io(message)
    }
}

#[allow(clippy::result_large_err)]
pub(super) fn guard_read_file_count_total_size(
    meta_len: u64,
    count_total: bool,
) -> Result<(), crate::tool_result::ToolError> {
    if count_total && meta_len > READ_FILE_COUNT_TOTAL_MAX_BYTES {
        return Err(crate::tool_result::ToolError::external_code(
            "read_file_count_total_too_large",
            format!(
                "错误：文件过大（{} 字节），超过 count_total_lines 允许上限 {} 字节；请省略 count_total_lines 或分段读取。",
                meta_len, READ_FILE_COUNT_TOTAL_MAX_BYTES
            ),
        ));
    }
    Ok(())
}

pub(super) fn resolve_read_end_line(
    start_line: usize,
    end_line_opt: Option<usize>,
    max_lines: usize,
) -> (usize, bool) {
    let mut end_line =
        end_line_opt.unwrap_or_else(|| start_line.saturating_add(max_lines.saturating_sub(1)));
    let allowed_span = max_lines.saturating_sub(1);
    let max_end_by_cap = start_line.saturating_add(allowed_span);
    let truncated_by_max = end_line > max_end_by_cap;
    if truncated_by_max {
        end_line = max_end_by_cap;
    }
    (end_line, truncated_by_max)
}

#[allow(clippy::result_large_err)]
pub(super) fn sniff_opened_file_encoding(
    file: File,
    enc_name: TextEncodingName,
) -> Result<(ResolvedTextEncoding, DecodedFileNote, File, Vec<u8>), crate::tool_result::ToolError> {
    let (file, head) = sniff_head_bytes_from(file, enc_name)
        .map_err(|e| crate::tool_result::ToolError::external_code("read_file_io", e))?;
    let (resolved, decode_note) = resolve_text_encoding(&head, enc_name)
        .map_err(|e| crate::tool_result::ToolError::external_code("read_file_encoding", e))?;
    Ok((resolved, decode_note, file, head))
}

#[allow(clippy::result_large_err)]
pub(super) fn maybe_count_total_lines_for_read(
    count_total: bool,
    target: &Path,
    enc_name: TextEncodingName,
) -> Result<Option<usize>, crate::tool_result::ToolError> {
    if !count_total {
        return Ok(None);
    }
    count_lines_for_read(target, enc_name)
        .map_err(|e| read_file_body_error_from_pipeline_string(e).into_tool_error())
        .map(Some)
}

pub(super) struct ReadFileLinesDispatch<'a> {
    pub(super) working_dir: &'a Path,
    pub(super) target: &'a Path,
    pub(super) path: &'a str,
    pub(super) enc_name: TextEncodingName,
    pub(super) line_spec: &'a ReadFileLinesSpec<'a>,
    pub(super) enc_header: &'a str,
}

pub(super) fn dispatch_read_file_lines(
    resolved: ResolvedTextEncoding,
    d: ReadFileLinesDispatch<'_>,
    file: File,
    head: Vec<u8>,
) -> Result<ReadFileLinesResult, ReadFileBodyError> {
    match resolved {
        ResolvedTextEncoding::Utf8Strict => read_file_utf8_lines(
            d.working_dir,
            d.target,
            d.path,
            d.line_spec,
            0,
            d.enc_header,
            file,
        ),
        ResolvedTextEncoding::Utf8Sig { skip_bom } => read_file_utf8_lines(
            d.working_dir,
            d.target,
            d.path,
            d.line_spec,
            skip_bom,
            d.enc_header,
            file,
        ),
        ResolvedTextEncoding::Decoder { .. } => read_file_decoded_lines(
            d.working_dir,
            d.target,
            d.path,
            d.enc_name,
            d.line_spec,
            d.enc_header,
            file,
            head,
        ),
    }
}

struct Utf8CollectOutcome {
    collected: Vec<(usize, String)>,
    line_no: usize,
    eof_before_start: bool,
}

fn utf8_collect_lines_in_range(
    reader: &mut BufReader<File>,
    buf: &mut String,
    start_line: usize,
    end_line: usize,
    max_lines: usize,
) -> Result<Utf8CollectOutcome, ReadFileBodyError> {
    let mut line_no = 0usize;
    let mut collected: Vec<(usize, String)> = Vec::new();
    let mut eof_before_start = false;
    loop {
        buf.clear();
        let n = match reader.read_line(buf) {
            Ok(n) => n,
            Err(e) if e.kind() == ErrorKind::InvalidData => {
                return Err(ReadFileBodyError::Utf8Decode(
                    "读取失败：按 UTF-8 解码时遇到非法字节序列。请使用 encoding=gb18030、big5、utf-8-sig 或 auto 等重试。"
                        .to_string(),
                ));
            }
            Err(e) => return Err(ReadFileBodyError::Io(format!("读取文件失败: {}", e))),
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
    Ok(Utf8CollectOutcome {
        collected,
        line_no,
        eof_before_start,
    })
}

fn utf8_probe_has_more_lines(
    reader: &mut BufReader<File>,
    buf: &mut String,
    line_no: usize,
    end_line: usize,
) -> Result<bool, ReadFileBodyError> {
    if line_no > end_line {
        return Ok(true);
    }
    buf.clear();
    match reader.read_line(buf) {
        Ok(n) if n > 0 => Ok(true),
        Err(e) if e.kind() == ErrorKind::InvalidData => Err(ReadFileBodyError::Utf8Decode(
            "读取失败：按 UTF-8 解码时遇到非法字节序列。请使用 encoding=gb18030、big5 或 auto 等重试。"
                .to_string(),
        )),
        Err(e) => Err(ReadFileBodyError::Io(format!("读取文件失败: {}", e))),
        _ => Ok(false),
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
) -> Result<ReadFileLinesResult, ReadFileBodyError> {
    let ReadFileLinesSpec {
        start_line,
        end_line,
        max_lines,
        total_lines,
        truncated_by_max,
    } = *spec;
    f.seek(SeekFrom::Start(skip_bom as u64))
        .map_err(|e| ReadFileBodyError::Io(format!("定位文件失败: {}", e)))?;
    let mut reader = BufReader::new(f);
    let mut buf = String::new();
    let Utf8CollectOutcome {
        collected,
        line_no,
        eof_before_start,
    } = utf8_collect_lines_in_range(&mut reader, &mut buf, start_line, end_line, max_lines)?;

    if eof_before_start {
        let hint = total_lines
            .map(|t| t.to_string())
            .unwrap_or_else(|| "未知（未请求 count_total_lines）".to_string());
        return Err(ReadFileBodyError::InvalidRange(format!(
            "错误：start_line={} 超出文件行数（已知总行数: {}）",
            start_line, hint
        )));
    }

    let has_more = utf8_probe_has_more_lines(&mut reader, &mut buf, line_no, end_line)?;

    if collected.is_empty() {
        return Err(ReadFileBodyError::InvalidRange(format!(
            "错误：未读取到任何行（start_line={}，end_line={}）。请检查区间。",
            start_line, end_line
        )));
    }

    let end_line_shown = collected.last().map(|(l, _)| *l).unwrap_or(start_line);
    let line_count_returned = collected.len();
    let raw_body = assemble_read_output(AssembleReadOutputParams {
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
    });
    Ok(ReadFileLinesResult {
        raw_body,
        end_line_shown,
        line_count_returned,
        has_more,
    })
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
) -> Result<ReadFileLinesResult, ReadFileBodyError> {
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
    let (resolved, _) = resolve_text_encoding(&head, enc_name)
        .map_err(read_file_body_error_from_pipeline_string)?;
    let ResolvedTextEncoding::Decoder { .. } = resolved else {
        return Err(ReadFileBodyError::Internal(
            "内部错误：read_file_decoded_lines 需要解码器路径".to_string(),
        ));
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
    })
    .map_err(read_file_body_error_from_pipeline_string)?;

    if last_line_no < start_line && collected.is_empty() {
        eof_before_start = true;
    }

    if eof_before_start {
        let hint = total_lines
            .map(|t| t.to_string())
            .unwrap_or_else(|| "未知（未请求 count_total_lines）".to_string());
        return Err(ReadFileBodyError::InvalidRange(format!(
            "错误：start_line={} 超出文件行数（已知总行数: {}）",
            start_line, hint
        )));
    }

    if collected.is_empty() {
        return Err(ReadFileBodyError::InvalidRange(format!(
            "错误：未读取到任何行（start_line={}，end_line={}）。请检查区间。",
            start_line, end_line
        )));
    }

    let end_line_shown = collected.last().map(|(l, _)| *l).unwrap_or(start_line);
    let line_count_returned = collected.len();
    let raw_body = assemble_read_output(AssembleReadOutputParams {
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
    });
    Ok(ReadFileLinesResult {
        raw_body,
        end_line_shown,
        line_count_returned,
        has_more,
    })
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
