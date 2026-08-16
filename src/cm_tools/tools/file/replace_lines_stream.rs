//! `modify_file` 的 replace_lines 模式：流式读原文件、写临时文件后原子替换。

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::Path;

use serde_json::Value;

use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::write_sse_preview::{
    WORKSPACE_WRITE_DIFF_BUDGET_CHARS, WriteDiffFileState,
    format_tool_output_with_write_diff_preview,
};
use crate::cm_tools::workspace::changelist::record_file_state_after_write;

#[inline]
fn tool_output_prepend_path(rel_display: &str, message: impl AsRef<str>) -> String {
    format!("路径：{}\n{}", rel_display.trim(), message.as_ref())
}

fn parse_replace_line_range(v: &Value) -> Result<(usize, usize, String), String> {
    let start_line = match v.get("start_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return Err("错误：replace_lines 需要 start_line（>=1）".to_string()),
    };
    let end_line = match v.get("end_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return Err("错误：replace_lines 需要 end_line（>=1）".to_string()),
    };
    let new_body = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((start_line.min(end_line), start_line.max(end_line), new_body))
}

fn parse_insert_after_line(v: &Value) -> Result<(usize, String), String> {
    let after_line = match v.get("after_line").and_then(|n| n.as_u64()) {
        Some(n) => n as usize,
        _ => return Err("错误：insert_after_line 需要 after_line（>=0）".to_string()),
    };
    let new_body = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((after_line, new_body))
}

fn write_tmp_bytes<W: Write>(writer: &mut W, bytes: &[u8]) -> Result<(), String> {
    writer
        .write_all(bytes)
        .map_err(|e| format!("写入临时文件失败: {}", e))
}

fn stream_replace_lines_to_writer<W: Write>(
    reader: &mut BufReader<File>,
    writer: &mut W,
    start_line: usize,
    end_line: usize,
    new_body: &str,
) -> Result<(usize, bool), String> {
    let mut line_no: usize = 0;
    let mut replaced = false;
    let mut buf = String::new();

    loop {
        buf.clear();
        let n = reader
            .read_line(&mut buf)
            .map_err(|e| format!("读取原文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        line_no += 1;
        if line_no == start_line {
            write_insert_body(writer, new_body)?;
            replaced = true;
        }
        if (start_line..=end_line).contains(&line_no) {
            continue;
        }
        write_tmp_bytes(writer, buf.as_bytes())?;
    }

    Ok((line_no, replaced))
}

fn validate_replace_coverage(
    line_no: usize,
    start_line: usize,
    end_line: usize,
    replaced: bool,
) -> Result<(), String> {
    if line_no < start_line {
        return Err(format!(
            "错误：start_line={} 超出文件行数（文件共 {} 行）",
            start_line, line_no
        ));
    }
    if line_no < end_line {
        return Err(format!(
            "错误：end_line={} 超出文件行数（文件共 {} 行）",
            end_line, line_no
        ));
    }
    if !replaced {
        return Err("错误：未执行替换（内部状态异常）".to_string());
    }
    Ok(())
}

fn append_tail_context_to_error(mut err: String, original: Option<&str>) -> String {
    let Some(original) = original else {
        return err;
    };
    let lines: Vec<&str> = original.lines().collect();
    if lines.is_empty() {
        err.push_str(
            "\n文件为空；如需写入文件开头，请使用 mode=insert_after_line 且 after_line=0。",
        );
        return err;
    }
    let start = lines.len().saturating_sub(5);
    err.push_str("\n文件末尾上下文：");
    for (idx, line) in lines.iter().enumerate().skip(start) {
        err.push_str(&format!("\n  {}|{}", idx + 1, line));
    }
    err
}

fn write_insert_body<W: Write>(writer: &mut W, new_body: &str) -> Result<(), String> {
    if new_body.is_empty() {
        return Ok(());
    }
    write_tmp_bytes(writer, new_body.as_bytes())?;
    if !new_body.ends_with('\n') {
        write_tmp_bytes(writer, b"\n")?;
    }
    Ok(())
}

fn stream_insert_after_line_to_writer<W: Write>(
    reader: &mut BufReader<File>,
    writer: &mut W,
    after_line: usize,
    new_body: &str,
) -> Result<usize, String> {
    let mut line_no: usize = 0;
    if after_line == 0 {
        write_insert_body(writer, new_body)?;
    }
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader
            .read_line(&mut buf)
            .map_err(|e| format!("读取原文件失败: {}", e))?;
        if n == 0 {
            break;
        }
        line_no += 1;
        writer
            .write_all(buf.as_bytes())
            .map_err(|e| format!("写入临时文件失败: {}", e))?;
        if line_no == after_line {
            write_insert_body(writer, new_body)?;
        }
    }
    if after_line > line_no {
        return Err(format!(
            "错误：after_line={} 超出文件行数（文件共 {} 行）",
            after_line, line_no
        ));
    }
    Ok(line_no)
}

fn replace_lines_after_content_in_memory(
    target: &Path,
    start_line: usize,
    end_line: usize,
    new_body: &str,
) -> Result<String, String> {
    let src = File::open(target).map_err(|e| format!("读取原文件失败: {}", e))?;
    let mut reader = BufReader::new(src);
    let mut out_buf: Vec<u8> = Vec::new();
    {
        let mut w = BufWriter::new(&mut out_buf);
        let (line_no, replaced) =
            stream_replace_lines_to_writer(&mut reader, &mut w, start_line, end_line, new_body)?;
        validate_replace_coverage(line_no, start_line, end_line, replaced)?;
        w.flush().map_err(|e| format!("刷新缓冲失败: {}", e))?;
    }
    String::from_utf8(out_buf).map_err(|e| {
        format!(
            "错误：dry_run 生成内容非合法 UTF-8（首个无效偏移 {}）",
            e.utf8_error().valid_up_to()
        )
    })
}

fn insert_after_content_in_memory(
    target: &Path,
    after_line: usize,
    new_body: &str,
) -> Result<String, String> {
    let src = File::open(target).map_err(|e| format!("读取原文件失败: {}", e))?;
    let mut reader = BufReader::new(src);
    let mut out_buf: Vec<u8> = Vec::new();
    {
        let mut w = BufWriter::new(&mut out_buf);
        stream_insert_after_line_to_writer(&mut reader, &mut w, after_line, new_body)?;
        w.flush().map_err(|e| format!("刷新缓冲失败: {}", e))?;
    }
    String::from_utf8(out_buf).map_err(|e| {
        format!(
            "错误：dry_run 生成内容非合法 UTF-8（首个无效偏移 {}）",
            e.utf8_error().valid_up_to()
        )
    })
}

fn commit_tmp_over_target(tmp_path: &Path, target: &Path) -> Result<(), String> {
    if target.exists() {
        std::fs::remove_file(target).map_err(|e| {
            let _ = std::fs::remove_file(tmp_path);
            format!("删除原文件以替换失败: {}", e)
        })?;
    }
    std::fs::rename(tmp_path, target).map_err(|e| {
        let _ = std::fs::remove_file(tmp_path);
        format!("替换目标文件失败: {}", e)
    })
}

fn edit_tmp_path(target: &Path) -> Result<std::path::PathBuf, String> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Err("错误：无法解析目标文件父目录".to_string()),
    };
    let fname = target
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    Ok(parent.join(format!(".{fname}.crabmate_edit_tmp")))
}

fn open_stream_edit_pair(
    target: &Path,
    tmp_path: &Path,
) -> Result<(BufReader<File>, BufWriter<File>), String> {
    let src = File::open(target).map_err(|e| format!("读取原文件失败: {}", e))?;
    let tmp_file = File::create(tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;
    Ok((BufReader::new(src), BufWriter::new(tmp_file)))
}

fn flush_or_abort_tmp(writer: &mut BufWriter<File>, tmp_path: &Path) -> Result<(), String> {
    writer.flush().map_err(|e| {
        let _ = std::fs::remove_file(tmp_path);
        format!("刷新临时文件失败: {}", e)
    })
}

fn modify_replace_dry_run(
    target: &Path,
    display_path: &str,
    rel_path: &str,
    original: Option<String>,
    start_line: usize,
    end_line: usize,
    new_body: &str,
) -> String {
    let preview =
        match replace_lines_after_content_in_memory(target, start_line, end_line, new_body) {
            Ok(s) => s,
            Err(e) => return append_tail_context_to_error(e, original.as_deref()),
        };
    let body = tool_output_prepend_path(
        display_path,
        format!(
            "预览（dry_run=true）：replace_lines {}-{} 未写盘。设置 dry_run=false 以执行。\n\
共替换 {} 行区间，新片段 {} 字节",
            start_line,
            end_line,
            end_line - start_line + 1,
            new_body.len()
        ),
    );
    format_tool_output_with_write_diff_preview(
        "modify_file",
        body,
        vec![WriteDiffFileState {
            rel_path: rel_path.to_string(),
            before: original,
            after: Some(preview),
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

fn modify_insert_dry_run(
    target: &Path,
    display_path: &str,
    rel_path: &str,
    original: Option<String>,
    after_line: usize,
    new_body: &str,
) -> String {
    let preview = match insert_after_content_in_memory(target, after_line, new_body) {
        Ok(s) => s,
        Err(e) => return append_tail_context_to_error(e, original.as_deref()),
    };
    let body = tool_output_prepend_path(
        display_path,
        format!(
            "预览（dry_run=true）：insert_after_line {} 未写盘。设置 dry_run=false 以执行。\n\
新片段 {} 字节",
            after_line,
            new_body.len()
        ),
    );
    format_tool_output_with_write_diff_preview(
        "modify_file",
        body,
        vec![WriteDiffFileState {
            rel_path: rel_path.to_string(),
            before: original,
            after: Some(preview),
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

fn finish_modify_file_success(
    ctx: &ToolContext<'_>,
    working_dir: &Path,
    rel_path: &str,
    original: Option<String>,
    target: &Path,
    body: String,
) -> String {
    record_file_state_after_write(
        ctx.workspace_changelist,
        working_dir,
        rel_path,
        original.clone(),
    );
    let after = std::fs::read_to_string(target).ok();
    format_tool_output_with_write_diff_preview(
        "modify_file",
        body,
        vec![WriteDiffFileState {
            rel_path: rel_path.to_string(),
            before: original,
            after,
        }],
        WORKSPACE_WRITE_DIFF_BUDGET_CHARS,
    )
}

pub(super) fn modify_file_replace_lines(
    v: &Value,
    target: &Path,
    display_path: &str,
    ctx: &ToolContext<'_>,
    working_dir: &Path,
    rel_path: &str,
) -> String {
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(false);
    let original = std::fs::read_to_string(target).ok();
    let (start_line, end_line, new_body) = match parse_replace_line_range(v) {
        Ok(x) => x,
        Err(e) => return e,
    };

    if dry_run {
        return modify_replace_dry_run(
            target,
            display_path,
            rel_path,
            original,
            start_line,
            end_line,
            &new_body,
        );
    }

    let tmp_path = match edit_tmp_path(target) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (mut reader, mut writer) = match open_stream_edit_pair(target, &tmp_path) {
        Ok(x) => x,
        Err(e) => return e,
    };

    let (line_no, replaced) = match stream_replace_lines_to_writer(
        &mut reader,
        &mut writer,
        start_line,
        end_line,
        &new_body,
    ) {
        Ok(x) => x,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return e;
        }
    };

    if let Err(e) = validate_replace_coverage(line_no, start_line, end_line, replaced) {
        let _ = std::fs::remove_file(&tmp_path);
        return append_tail_context_to_error(e, original.as_deref());
    }

    if let Err(e) = flush_or_abort_tmp(&mut writer, &tmp_path) {
        return e;
    }
    drop(writer);

    if let Err(e) = commit_tmp_over_target(&tmp_path, target) {
        return e;
    }

    let body = tool_output_prepend_path(
        display_path,
        format!(
            "已按行替换（行 {}-{}，共删除 {} 行，写入新内容 {} 字节）",
            start_line,
            end_line,
            end_line - start_line + 1,
            new_body.len()
        ),
    );
    finish_modify_file_success(ctx, working_dir, rel_path, original, target, body)
}

pub(super) fn modify_file_insert_after_line(
    v: &Value,
    target: &Path,
    display_path: &str,
    ctx: &ToolContext<'_>,
    working_dir: &Path,
    rel_path: &str,
) -> String {
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(false);
    let original = std::fs::read_to_string(target).ok();
    let (after_line, new_body) = match parse_insert_after_line(v) {
        Ok(x) => x,
        Err(e) => return e,
    };

    if dry_run {
        return modify_insert_dry_run(
            target,
            display_path,
            rel_path,
            original,
            after_line,
            &new_body,
        );
    }

    let tmp_path = match edit_tmp_path(target) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let (mut reader, mut writer) = match open_stream_edit_pair(target, &tmp_path) {
        Ok(x) => x,
        Err(e) => return e,
    };

    if let Err(e) =
        stream_insert_after_line_to_writer(&mut reader, &mut writer, after_line, &new_body)
    {
        let _ = std::fs::remove_file(&tmp_path);
        return append_tail_context_to_error(e, original.as_deref());
    }

    if let Err(e) = flush_or_abort_tmp(&mut writer, &tmp_path) {
        return e;
    }
    drop(writer);

    if let Err(e) = commit_tmp_over_target(&tmp_path, target) {
        return e;
    }

    let body = tool_output_prepend_path(
        display_path,
        format!(
            "已插入内容（after_line={}，写入新内容 {} 字节）",
            after_line,
            new_body.len()
        ),
    );
    finish_modify_file_success(ctx, working_dir, rel_path, original, target, body)
}
