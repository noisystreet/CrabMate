//! `modify_file` 的 replace_lines 模式：流式读原文件、写临时文件后原子替换。

use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// 在全文中定位 `needle` 首次出现的行号（1-based，整行精确匹配）；返回命中行号列表。
fn find_exact_line_hits(lines: &[&str], needle: &str) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| **l == needle)
        .map(|(i, _)| i + 1)
        .collect()
}

/// 期望内容找不到时追加的纠偏提示（含实际所在行号或「未找到」说明）。
fn append_relocate_hint(err: &mut String, lines: &[&str], needle: &str, arg_name: &str) {
    let hits = find_exact_line_hits(lines, needle);
    match hits.len() {
        1 => err.push_str(&format!(
            "\n提示：期望内容实际位于行 {}（与传入行号不一致）；行号可能已因早前编辑偏移，请按当前行号重算后重试。",
            hits[0]
        )),
        n if n > 1 => err.push_str(&format!(
            "\n提示：期望内容在文件中出现 {} 处（首次位于行 {}）；请先 read_file 确认当前行号再编辑。",
            n, hits[0]
        )),
        _ => err.push_str(&format!(
            "\n提示：期望内容未在文件中找到；请先 read_file 获取当前内容后再设置 {}。",
            arg_name
        )),
    }
}

/// `mode=replace_lines` 的 `expect_content` 守卫：逐行精确比对 [start_line..=end_line]
/// 当前内容与期望；不一致即拒绝写盘，并给出实际内容与纠偏行号。
pub(super) fn verify_expect_content(
    lines: &[&str],
    start_line: usize,
    end_line: usize,
    expect: &str,
) -> Result<(), String> {
    let expect_body = expect.trim_end_matches('\n');
    let expect_lines: Vec<&str> = expect_body.lines().collect();
    if start_line > lines.len() || end_line > lines.len() {
        return Err(format!(
            "错误：expect_content 校验失败，未写盘：行区间 {}-{} 超出文件行数（文件共 {} 行）",
            start_line,
            end_line,
            lines.len()
        ));
    }
    let span = end_line.saturating_sub(start_line) + 1;
    if expect_lines.len() != span {
        return Err(format!(
            "错误：expect_content 校验失败，未写盘：期望内容共 {} 行，与替换区间行 {}-{}（共 {} 行）行数不一致。",
            expect_lines.len(),
            start_line,
            end_line,
            span
        ));
    }
    let mut mismatches: Vec<String> = Vec::new();
    for (offset, expect_line) in expect_lines.iter().enumerate() {
        let idx = start_line - 1 + offset;
        let actual = lines[idx];
        if actual != *expect_line {
            mismatches.push(format!(
                "  行 {}|实际: {}\n        期望: {}",
                idx + 1,
                actual,
                expect_line
            ));
        }
    }
    if mismatches.is_empty() {
        return Ok(());
    }
    let mut err = format!(
        "错误：expect_content 校验失败，未写盘。行 {}-{} 当前内容与期望不符：\n{}",
        start_line,
        end_line,
        mismatches.join("\n")
    );
    let first = expect_lines.iter().find(|l| !l.trim().is_empty());
    if let Some(first) = first {
        append_relocate_hint(&mut err, lines, first, "expect_content");
    }
    Err(err)
}

/// `mode=insert_after_line` 的 `expect_line_content` 守卫：校验锚点行 `after_line`
/// 当前内容与期望一致；不一致即拒绝写盘，并给出纠偏行号。
pub(super) fn verify_expect_line_content(
    lines: &[&str],
    after_line: usize,
    expect: &str,
) -> Result<(), String> {
    let expect_body = expect.trim_end_matches('\n');
    if after_line == 0 {
        return Err(
            "错误：expect_line_content 校验失败，未写盘：after_line=0 表示插入到文件开头，无锚点行可校验；请去掉 expect_line_content 或改用 after_line>=1。"
                .to_string(),
        );
    }
    let Some(actual) = lines.get(after_line - 1) else {
        return Err(format!(
            "错误：expect_line_content 校验失败，未写盘：锚点行 {} 超出文件行数（文件共 {} 行）",
            after_line,
            lines.len()
        ));
    };
    if *actual == expect_body {
        return Ok(());
    }
    let mut err = format!(
        "错误：expect_line_content 校验失败，未写盘。锚点行 {} 当前内容与期望不符：\n  行 {}|实际: {}\n        期望: {}",
        after_line, after_line, actual, expect_body
    );
    if !expect_body.is_empty() {
        append_relocate_hint(&mut err, lines, expect_body, "expect_line_content");
    }
    Err(err)
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

/// 临时文件守卫：正常提交后调用 [`TmpFileGuard::keep`]；否则 Drop 时删除，避免异常路径残留。
struct TmpFileGuard {
    path: PathBuf,
    committed: bool,
}

impl TmpFileGuard {
    fn new(path: PathBuf) -> Self {
        Self {
            path,
            committed: false,
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn keep(&mut self) {
        self.committed = true;
    }
}

impl Drop for TmpFileGuard {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

#[cfg(unix)]
fn copy_owner_best_effort(meta: &std::fs::Metadata, dst: &File) {
    use std::os::unix::fs::MetadataExt;
    use std::os::unix::io::AsRawFd;
    // best-effort：无 CAP_CHOWN 时忽略失败（属主通常已与当前进程一致）。
    unsafe {
        let _ = libc::fchown(
            dst.as_raw_fd(),
            meta.uid() as libc::uid_t,
            meta.gid() as libc::gid_t,
        );
    }
}

#[cfg(not(unix))]
fn copy_owner_best_effort(_meta: &std::fs::Metadata, _dst: &File) {}

/// 复制源文件扩展属性（含 POSIX ACL 的 `system.posix_acl_access`）到目标；逐项 best-effort。
#[cfg(target_os = "linux")]
fn copy_xattrs_best_effort(src_path: &Path, dst_path: &Path) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let src = match CString::new(src_path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return,
    };
    let dst = match CString::new(dst_path.as_os_str().as_bytes()) {
        Ok(c) => c,
        Err(_) => return,
    };

    let size = unsafe { libc::listxattr(src.as_ptr(), std::ptr::null_mut(), 0) };
    if size <= 0 {
        return;
    }
    let mut names = vec![0u8; size as usize];
    let got = unsafe {
        libc::listxattr(
            src.as_ptr(),
            names.as_mut_ptr() as *mut libc::c_char,
            names.len(),
        )
    };
    if got <= 0 {
        return;
    }
    names.truncate(got as usize);

    for name in names.split(|b| *b == 0u8) {
        if name.is_empty() {
            continue;
        }
        let name_c = match CString::new(name.to_vec()) {
            Ok(c) => c,
            Err(_) => continue,
        };
        let vlen = unsafe { libc::getxattr(src.as_ptr(), name_c.as_ptr(), std::ptr::null_mut(), 0) };
        if vlen < 0 {
            continue;
        }
        let mut val = vec![0u8; vlen as usize];
        let vgot = unsafe {
            libc::getxattr(
                src.as_ptr(),
                name_c.as_ptr(),
                val.as_mut_ptr() as *mut libc::c_void,
                val.len(),
            )
        };
        if vgot < 0 {
            continue;
        }
        val.truncate(vgot as usize);
        // `security.*`（如 SELinux label）等可能因权限失败，best-effort 忽略。
        unsafe {
            let _ = libc::setxattr(
                dst.as_ptr(),
                name_c.as_ptr(),
                val.as_ptr() as *const libc::c_void,
                val.len(),
                0,
            );
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn copy_xattrs_best_effort(_src_path: &Path, _dst_path: &Path) {}

/// 把原文件的元数据尽量继承到临时文件：属主（best-effort）→ 权限位 → 扩展属性（best-effort）。
/// 先 chown 再 chmod，因为 chown 可能清掉 setuid/setgid 位。
fn preserve_target_metadata(
    src: &File,
    dst: &File,
    src_path: &Path,
    dst_path: &Path,
) -> Result<(), String> {
    let meta = src
        .metadata()
        .map_err(|e| format!("读取原文件元数据失败: {}", e))?;
    copy_owner_best_effort(&meta, dst);
    dst.set_permissions(meta.permissions())
        .map_err(|e| format!("设置临时文件权限失败: {}", e))?;
    copy_xattrs_best_effort(src_path, dst_path);
    Ok(())
}

/// 目标是否为多硬链接（nlink > 1）。这类文件不能用 rename 换 inode，
/// 否则其他硬链接仍指向旧内容。
#[cfg(unix)]
fn target_has_multiple_links(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path)
        .map(|m| m.nlink() > 1)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn target_has_multiple_links(_path: &Path) -> bool {
    false
}

/// 硬链接场景：把临时文件内容写回原 inode（O_TRUNC 原地覆盖），保留 inode，
/// 使其他硬链接同步可见新内容。POSIX 下「保留 inode」与「rename 原子替换」不可兼得，
/// 故此路径非原子，仅在 nlink > 1 时使用。
fn copy_tmp_into_target_inode(tmp_path: &Path, target: &Path) -> Result<(), String> {
    let mut src = File::open(tmp_path).map_err(|e| format!("读取临时文件失败: {}", e))?;
    let mut dst = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(target)
        .map_err(|e| format!("写入原文件失败: {}", e))?;
    std::io::copy(&mut src, &mut dst).map_err(|e| format!("写回原文件失败: {}", e))?;
    dst.sync_all().map_err(|e| format!("落盘原文件失败: {}", e))?;
    // 写回成功后清理临时文件（best-effort；失败不影响已完成的写入）。
    let _ = std::fs::remove_file(tmp_path);
    Ok(())
}

fn commit_tmp_over_target(
    tmp_path: &Path,
    target: &Path,
    preserve_inode: bool,
) -> Result<(), String> {
    if preserve_inode {
        copy_tmp_into_target_inode(tmp_path, target)?;
    } else {
        // 同目录下 rename(2) 会对已存在的目标做原子替换；不再先 remove_file，
        // 既避免目标短暂消失，也避免 rename 失败时原文件已被删除（数据丢失）。
        std::fs::rename(tmp_path, target).map_err(|e| format!("替换目标文件失败: {}", e))?;
    }
    // 尽力 fsync 父目录，确保目录项落盘。
    if let Some(parent) = target.parent()
        && let Ok(dir) = File::open(parent)
    {
        let _ = dir.sync_all();
    }
    Ok(())
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

fn edit_tmp_path(target: &Path) -> Result<PathBuf, String> {
    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return Err("错误：无法解析目标文件父目录".to_string()),
    };
    let fname = target
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    // pid + 进程内自增序号，避免并发编辑同一文件时临时名冲突。
    let seq = TMP_SEQ.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!(
        ".{fname}.crabmate_edit_tmp.{}.{}",
        std::process::id(),
        seq
    )))
}

fn open_stream_edit_pair(
    target: &Path,
    tmp_path: &Path,
    inherit_metadata: bool,
) -> Result<(BufReader<File>, BufWriter<File>), String> {
    let src = File::open(target).map_err(|e| format!("读取原文件失败: {}", e))?;
    let tmp_file = File::create(tmp_path).map_err(|e| format!("创建临时文件失败: {}", e))?;
    if inherit_metadata {
        // 临时文件随后会 rename 覆盖目标，需继承原文件属主/权限/扩展属性，否则会退化为默认值。
        // 写回原 inode（硬链接）时目标元数据不变，无需复制。
        preserve_target_metadata(&src, &tmp_file, target, tmp_path)?;
    }
    Ok((BufReader::new(src), BufWriter::new(tmp_file)))
}

fn flush_or_abort_tmp(writer: &mut BufWriter<File>, tmp_path: &Path) -> Result<(), String> {
    writer.flush().map_err(|e| {
        let _ = std::fs::remove_file(tmp_path);
        format!("刷新临时文件失败: {}", e)
    })?;
    writer.get_ref().sync_all().map_err(|e| {
        let _ = std::fs::remove_file(tmp_path);
        format!("落盘临时文件失败: {}", e)
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

/// replace_lines 的 expect 守卫：交叉参数拒绝 + `expect_content` 逐行校验（基于当前磁盘快照）。
fn check_replace_expect_guards(
    v: &Value,
    original: Option<&str>,
    start_line: usize,
    end_line: usize,
) -> Result<(), String> {
    if v.get("expect_line_content").is_some() {
        return Err(
            "错误：expect_line_content 仅用于 insert_after_line 的锚点行校验；replace_lines 请使用 expect_content。"
                .to_string(),
        );
    }
    if let Some(expect) = v.get("expect_content").and_then(|c| c.as_str()) {
        let Some(orig) = original else {
            return Err(
                "错误：无法读取原文件内容进行 expect_content 校验（文件可能不是 UTF-8 文本），未写盘。"
                    .to_string(),
            );
        };
        let view: Vec<&str> = orig.lines().collect();
        verify_expect_content(&view, start_line, end_line, expect)?;
    }
    Ok(())
}

/// insert_after_line 的 expect 守卫：交叉参数拒绝 + `expect_line_content` 锚点行校验。
fn check_insert_expect_guards(
    v: &Value,
    original: Option<&str>,
    after_line: usize,
) -> Result<(), String> {
    if v.get("expect_content").is_some() {
        return Err(
            "错误：expect_content 仅用于 replace_lines 的区间内容校验；insert_after_line 请使用 expect_line_content。"
                .to_string(),
        );
    }
    if let Some(expect) = v.get("expect_line_content").and_then(|c| c.as_str()) {
        if after_line == 0 {
            return Err(
                "错误：after_line=0 表示插入到文件开头，无锚点行可校验；expect_line_content 需 after_line>=1。"
                    .to_string(),
            );
        }
        let Some(orig) = original else {
            return Err(
                "错误：无法读取原文件内容进行 expect_line_content 校验（文件可能不是 UTF-8 文本），未写盘。"
                    .to_string(),
            );
        };
        let view: Vec<&str> = orig.lines().collect();
        verify_expect_line_content(&view, after_line, expect)?;
    }
    Ok(())
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

    if let Err(e) = check_replace_expect_guards(v, original.as_deref(), start_line, end_line) {
        return e;
    }

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

    let preserve_inode = target_has_multiple_links(target);
    let tmp_path = match edit_tmp_path(target) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut guard = TmpFileGuard::new(tmp_path);
    let (mut reader, mut writer) =
        match open_stream_edit_pair(target, guard.path(), !preserve_inode) {
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
        Err(e) => return e,
    };

    if let Err(e) = validate_replace_coverage(line_no, start_line, end_line, replaced) {
        return append_tail_context_to_error(e, original.as_deref());
    }

    if let Err(e) = flush_or_abort_tmp(&mut writer, guard.path()) {
        return e;
    }
    drop(writer);
    drop(reader);

    if let Err(e) = commit_tmp_over_target(guard.path(), target, preserve_inode) {
        return e;
    }
    guard.keep();

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

    if let Err(e) = check_insert_expect_guards(v, original.as_deref(), after_line) {
        return e;
    }

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

    let preserve_inode = target_has_multiple_links(target);
    let tmp_path = match edit_tmp_path(target) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut guard = TmpFileGuard::new(tmp_path);
    let (mut reader, mut writer) =
        match open_stream_edit_pair(target, guard.path(), !preserve_inode) {
            Ok(x) => x,
            Err(e) => return e,
        };

    if let Err(e) =
        stream_insert_after_line_to_writer(&mut reader, &mut writer, after_line, &new_body)
    {
        return append_tail_context_to_error(e, original.as_deref());
    }

    if let Err(e) = flush_or_abort_tmp(&mut writer, guard.path()) {
        return e;
    }
    drop(writer);
    drop(reader);

    if let Err(e) = commit_tmp_over_target(guard.path(), target, preserve_inode) {
        return e;
    }
    guard.keep();

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
