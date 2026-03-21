//! 工作区文件创建与修改工具
//!
//! 路径均为**相对于工作目录**的相对路径（与 main 中 workspace 文件 API 一致，基于 run_command_working_dir）。
//!
//! 大文件：`read_file` 按行流式读取并默认限制单次返回行数；`modify_file` 支持按行区间替换，避免整文件读入内存。

use regex::RegexBuilder;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};

/// 单次 read_file 默认最多返回的行数（防撑爆上下文）
const READ_FILE_DEFAULT_MAX_LINES: usize = 500;
/// read_file 允许的单次上限
const READ_FILE_ABS_MAX_LINES: usize = 8000;

/// read_binary_meta：默认读取文件头参与哈希的字节数
const READ_BINARY_META_PREFIX_DEFAULT: usize = 8192;
/// read_binary_meta：前缀哈希最多读取字节（避免大文件读入过多）
const READ_BINARY_META_PREFIX_MAX: usize = 256 * 1024;

fn canonical_workspace_root(base: &Path) -> Result<PathBuf, String> {
    base.canonicalize()
        .map_err(|e| format!("工作目录无法解析: {}", e))
}

fn ensure_within_workspace(base_canonical: &Path, candidate: &Path) -> Result<(), String> {
    if candidate.starts_with(base_canonical) {
        Ok(())
    } else {
        Err("路径不能超出工作目录".to_string())
    }
}

// 对“目标路径或其最近存在祖先”做 canonical 边界校验，防止借助工作区内 symlink 逃逸。
fn ensure_existing_ancestor_within_workspace(
    base_canonical: &Path,
    target: &Path,
) -> Result<(), String> {
    let mut ancestor = target;
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "路径无法解析".to_string())?;
    }
    let ancestor_canonical = ancestor
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_within_workspace(base_canonical, &ancestor_canonical)
}

/// 解析用于读取或修改的路径（目标必须存在；path 必须为相对工作目录的相对路径）
fn resolve_for_read(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = canonical_workspace_root(base)?;
    let joined = base_canonical.join(sub);
    let canonical = joined
        .canonicalize()
        .map_err(|e| format!("路径无法解析: {}", e))?;
    ensure_within_workspace(&base_canonical, &canonical)?;
    Ok(canonical)
}

/// 解析用于写入的路径（目标可不存在；path 必须为相对工作目录的相对路径，且不能通过 .. 超出工作目录）
fn resolve_for_write(base: &Path, sub: &str) -> Result<PathBuf, String> {
    let sub = sub.trim();
    if sub.is_empty() {
        return Err("path 不能为空".to_string());
    }
    if Path::new(sub).is_absolute() {
        return Err("路径必须为相对于工作目录的相对路径，不能使用绝对路径".to_string());
    }
    let base_canonical = canonical_workspace_root(base)?;
    let joined = base_canonical.join(sub);
    // 规范化 .. 和 . 并确保仍在 base 下（路径穿越检查）
    let normalized = normalize_path(&joined);
    ensure_within_workspace(&base_canonical, &normalized)?;
    ensure_existing_ancestor_within_workspace(&base_canonical, &normalized)?;
    Ok(normalized)
}

/// 简单规范化：去掉 . 和 .. 段（不访问文件系统）
fn normalize_path(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

/// 创建文件：仅在文件不存在时创建；若已存在则报错。
/// 参数 args_json: { "path": string, "content": string }
pub fn create_file(args_json: &str, working_dir: &Path) -> String {
    let (path, content) = match parse_path_content(args_json) {
        Ok(pc) => pc,
        Err(e) => return e,
    };
    let target = match resolve_for_write(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if target.exists() {
        return "错误：文件已存在，无法仅创建".to_string();
    }
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(parent) {
                return format!("创建目录失败: {}", e);
            }
    match std::fs::write(&target, content.as_bytes()) {
        Ok(()) => format!("已创建文件: {}", target.display()),
        Err(e) => format!("写入文件失败: {}", e),
    }
}

/// 修改文件：仅在文件已存在时写入。
/// - 默认 `mode`=`full`：整文件覆盖（`content` 为全文）。
/// - `mode`=`replace_lines`：`start_line`..=`end_line`（1-based，含边界）替换为 `content`（流式读写，适合大文件）。
pub fn modify_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p.to_string(),
        None => return "缺少 path 参数".to_string(),
    };

    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "full".to_string());

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法仅修改".to_string();
    }

    if mode == "replace_lines" || mode == "lines" {
        modify_file_replace_lines(&v, &target)
    } else if mode == "full" || mode.is_empty() {
        let content = v
            .get("content")
            .and_then(|c| c.as_str())
            .map(String::from)
            .unwrap_or_default();
        match std::fs::write(&target, content.as_bytes()) {
            Ok(()) => format!("已整文件覆盖: {}", target.display()),
            Err(e) => format!("写入文件失败: {}", e),
        }
    } else {
        format!(
            "错误：mode 仅支持 full 或 replace_lines（收到 {:?}）",
            mode
        )
    }
}

fn modify_file_replace_lines(v: &serde_json::Value, target: &Path) -> String {
    let start_line = match v.get("start_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return "错误：replace_lines 需要 start_line（>=1）".to_string(),
    };
    let end_line = match v.get("end_line").and_then(|n| n.as_u64()) {
        Some(n) if n >= 1 => n as usize,
        _ => return "错误：replace_lines 需要 end_line（>=1）".to_string(),
    };
    if end_line < start_line {
        return "错误：end_line 不能小于 start_line".to_string();
    }

    let new_body = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();

    let parent = match target.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => return "错误：无法解析目标文件父目录".to_string(),
    };
    let fname = target
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");
    let tmp_path = parent.join(format!(".{fname}.crabmate_edit_tmp"));

    let src = match File::open(target) {
        Ok(f) => f,
        Err(e) => return format!("读取原文件失败: {}", e),
    };
    let tmp_file = match File::create(&tmp_path) {
        Ok(f) => f,
        Err(e) => return format!("创建临时文件失败: {}", e),
    };
    let mut reader = BufReader::new(src);
    let mut writer = BufWriter::new(tmp_file);
    let mut line_no: usize = 0;
    let mut replaced = false;
    let mut buf = String::new();

    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(n) => n,
            Err(e) => return format!("读取原文件失败: {}", e),
        };
        if n == 0 {
            break;
        }
        line_no += 1;
        if line_no < start_line {
            if let Err(e) = writer.write_all(buf.as_bytes()) {
                return format!("写入临时文件失败: {}", e);
            }
            continue;
        }
        if line_no == start_line {
            if !new_body.is_empty() {
                if let Err(e) = writer.write_all(new_body.as_bytes()) {
                    return format!("写入临时文件失败: {}", e);
                }
                if !new_body.ends_with('\n')
                    && let Err(e) = writer.write_all(b"\n") {
                        return format!("写入临时文件失败: {}", e);
                    }
            }
            replaced = true;
        }
        if line_no >= start_line && line_no <= end_line {
            continue;
        }
        if line_no > end_line
            && let Err(e) = writer.write_all(buf.as_bytes()) {
                return format!("写入临时文件失败: {}", e);
            }
    }

    if line_no < start_line {
        return format!(
            "错误：start_line={} 超出文件行数（文件共 {} 行）",
            start_line, line_no
        );
    }
    if line_no < end_line {
        return format!(
            "错误：end_line={} 超出文件行数（文件共 {} 行）",
            end_line, line_no
        );
    }
    if !replaced {
        return "错误：未执行替换（内部状态异常）".to_string();
    }

    if let Err(e) = writer.flush() {
        let _ = std::fs::remove_file(&tmp_path);
        return format!("刷新临时文件失败: {}", e);
    }
    drop(writer);
    // Windows 上 rename 不能覆盖已存在目标，需先删原文件
    if target.exists()
        && let Err(e) = std::fs::remove_file(target) {
            let _ = std::fs::remove_file(&tmp_path);
            return format!("删除原文件以替换失败: {}", e);
        }
    if let Err(e) = std::fs::rename(&tmp_path, target) {
        let _ = std::fs::remove_file(&tmp_path);
        return format!("替换目标文件失败: {}", e);
    }

    format!(
        "已按行替换: {} (行 {}-{}，共删除 {} 行，写入新内容 {} 字节)",
        target.display(),
        start_line,
        end_line,
        end_line - start_line + 1,
        new_body.len()
    )
}

/// 读取文件：按行**流式**读取，不把整文件载入内存。
///
/// - `max_lines`：单次最多返回行数（默认 500，上限 8000）。若未指定 `end_line`，则读到 `start_line + max_lines - 1` 或 EOF。
/// - 若同时指定 `end_line` 与 `max_lines`，实际返回行数不超过 `max_lines`；若区间更宽会截断并提示 `has_more`。
/// - `count_total_lines=true` 时会再扫描一遍文件统计总行数（大文件较慢）。
pub fn read_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v.get("path").and_then(|p| p.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => return "缺少 path 参数".to_string(),
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
        && e < start_line {
            return "错误：end_line 不能小于 start_line".to_string();
        }

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法读取".to_string();
    }

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    if meta.len() == 0 {
        return format!("文件为空: {}", target.display());
    }

    let total_lines = if count_total {
        match count_lines_in_file(&target) {
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
    // 用户指定了很大的区间时，仍按 max_lines 截断单次返回
    let allowed_span = max_lines.saturating_sub(1);
    let max_end_by_cap = start_line.saturating_add(allowed_span);
    let truncated_by_max = end_line > max_end_by_cap;
    if truncated_by_max {
        end_line = max_end_by_cap;
    }

    let file = match File::open(&target) {
        Ok(f) => f,
        Err(e) => return format!("打开文件失败: {}", e),
    };
    let mut reader = BufReader::new(file);
    let mut buf = String::new();
    let mut line_no: usize = 0;
    let mut collected: Vec<(usize, String)> = Vec::new();
    let mut eof_before_start = false;

    loop {
        buf.clear();
        let n = match reader.read_line(&mut buf) {
            Ok(n) => n,
            Err(e) => return format!("读取文件失败: {}", e),
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
        return format!(
            "错误：start_line={} 超出文件行数（已知总行数: {}）",
            start_line, hint
        );
    }

    let mut has_more = false;
    if line_no > end_line {
        has_more = true;
    } else {
        buf.clear();
        match reader.read_line(&mut buf) {
            Ok(n) if n > 0 => has_more = true,
            _ => {}
        }
    }

    if collected.is_empty() {
        return format!(
            "错误：未读取到任何行（start_line={}，end_line={}）。请检查区间。",
            start_line, end_line
        );
    }

    let last_shown = collected.last().map(|(l, _)| *l).unwrap_or(start_line);
    let mut out = String::new();
    out.push_str(&format!("文件: {}\n", target.display()));
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
        out.push_str(&format!("{}|{}\n", idx, line.trim_end_matches('\n')));
    }
    out.trim_end().to_string()
}

fn count_lines_in_file(path: &Path) -> Result<usize, String> {
    let file = File::open(path).map_err(|e| format!("打开文件失败: {}", e))?;
    let mut reader = BufReader::new(file);
    let mut count = 0usize;
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader.read_line(&mut buf).map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 {
            break;
        }
        count += 1;
    }
    Ok(count)
}

/// 读取目录：返回指定目录下的文件/子目录列表（受控只读）。
/// 参数：{ "path"?: string, "max_entries"?: integer, "include_hidden"?: boolean }
pub fn read_dir(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(".");

    if path.starts_with('/') || path.contains("..") {
        return "错误：path 必须是工作区内的相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let max_entries = v
        .get("max_entries")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(200);
    let include_hidden = v
        .get("include_hidden")
        .and_then(|b| b.as_bool())
        .unwrap_or(false);

    let root = match resolve_for_read(working_dir, path) {
        Ok(p) => p,
        Err(e) => return format!("错误：无法解析目录路径：{}", e),
    };
    if !root.is_dir() {
        return format!("错误：指定路径不是目录：{}", root.display());
    }

    let mut out = String::new();
    out.push_str(&format!("目录: {}\n", root.display()));
    match std::fs::read_dir(&root) {
        Ok(rd) => {
            let mut count = 0usize;
            let mut shown = 0usize;
            // 先遍历计数与展示（受 max_entries 限制）
            let mut entries: Vec<(String, bool)> = Vec::new();
            for e in rd.flatten() {
                count += 1;
                let name = e.file_name().to_string_lossy().to_string();
                if !include_hidden && name.starts_with('.') {
                    continue;
                }
                let is_dir = e.metadata().map(|m| m.is_dir()).unwrap_or(false);
                entries.push((name, is_dir));
            }
            entries.sort_by(|a, b| match (a.1, b.1) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.0.to_lowercase().cmp(&b.0.to_lowercase()),
            });
            for (name, is_dir) in entries.into_iter().take(max_entries) {
                shown += 1;
                out.push_str(&format!(
                    "{}{}\n",
                    if is_dir { "dir: " } else { "file: " },
                    name
                ));
            }
            out.push_str(&format!("总计遍历: {}，展示: {}\n", count, shown));
            out.trim_end().to_string()
        }
        Err(e) => format!("读取目录失败：{}", e),
    }
}

/// 检查文件/目录是否存在。
/// 参数：{ "path": string, "kind"?: "file"|"dir"|"any" }
pub fn file_exists(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p,
        None => return "错误：缺少 path 参数".to_string(),
    };

    if path.starts_with('/') || path.contains("..") {
        return "错误：path 必须是工作区内相对路径，且不能包含 .. 或绝对路径".to_string();
    }

    let kind = v
        .get("kind")
        .and_then(|k| k.as_str())
        .unwrap_or("any")
        .trim()
        .to_lowercase();

    let target = working_dir.join(path);
    let exists = target.exists();
    let type_ok = match kind.as_str() {
        "file" => target.is_file(),
        "dir" => target.is_dir(),
        "any" => exists,
        _ => return "错误：kind 仅支持 file|dir|any".to_string(),
    };

    let mut out = String::new();
    out.push_str(&format!("path: {}\n", path));
    out.push_str(&format!("exists: {}\n", exists));
    out.push_str(&format!("type_match: {}\n", type_ok));
    out.push_str(&format!("kind: {}\n", kind));
    out.trim_end().to_string()
}

/// 只读二进制/任意文件的**元数据**：大小、可选修改时间、文件头一段的 SHA256（不把整文件载入上下文）。
///
/// 参数：`path`（必填）；`prefix_hash_bytes`（可选，默认 8192，0 表示不算哈希，上限 256KiB）。
pub fn read_binary_meta(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };
    let path = match v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        Some(p) => p.to_string(),
        None => return "错误：缺少 path 参数".to_string(),
    };

    let prefix_hash_bytes = v
        .get("prefix_hash_bytes")
        .and_then(|n| n.as_u64())
        .map(|n| n as usize)
        .unwrap_or(READ_BINARY_META_PREFIX_DEFAULT)
        .min(READ_BINARY_META_PREFIX_MAX);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在".to_string();
    }

    let meta = match std::fs::metadata(&target) {
        Ok(m) => m,
        Err(e) => return format!("读取元数据失败: {}", e),
    };
    let size = meta.len();
    let modified_unix = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs());

    let mut out = String::new();
    out.push_str(&format!("path: {}\n", path));
    out.push_str(&format!("resolved: {}\n", target.display()));
    out.push_str(&format!("size_bytes: {}\n", size));

    if let Some(secs) = modified_unix {
        out.push_str(&format!("modified_unix_secs: {}\n", secs));
    } else {
        out.push_str("modified_unix_secs: (不可用)\n");
    }

    if prefix_hash_bytes == 0 {
        out.push_str("sha256_prefix: (已跳过，prefix_hash_bytes=0)\n");
        out.push_str("sha256_prefix_bytes: 0\n");
        return out.trim_end().to_string();
    }

    let to_read = (size as usize).min(prefix_hash_bytes);
    let mut file = match File::open(&target) {
        Ok(f) => f,
        Err(e) => return format!("打开文件失败: {}", e),
    };
    let mut buf = vec![0u8; to_read];
    if to_read > 0
        && let Err(e) = file.read_exact(&mut buf) {
            return format!("读取文件头失败: {}", e);
        }

    let digest = Sha256::digest(&buf);
    let hex = bytes_to_hex(&digest);
    out.push_str(&format!("sha256_prefix: {}\n", hex));
    out.push_str(&format!(
        "sha256_prefix_bytes: {}（文件共 {} 字节；仅头 {} 字节参与哈希）\n",
        to_read,
        size,
        to_read
    ));
    if (size as usize) > to_read {
        out.push_str("note: 文件大于前缀长度，哈希仅为文件头摘要，非整文件校验。\n");
    }
    out.trim_end().to_string()
}

fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// 在文件中按正则抽取匹配行（只读）。
/// 参数：
/// { "path": string, "pattern": string, "start_line"?: int, "end_line"?: int,
///   "max_matches"?: int, "case_insensitive"?: bool, "max_snippet_chars"?: int }
pub fn extract_in_file(args_json: &str, working_dir: &Path) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数 JSON 无效: {}", e),
    };

    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_default();
    if path.is_empty() {
        return "缺少 path 参数".to_string();
    }

    let pattern = v
        .get("pattern")
        .and_then(|p| p.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let pattern = match pattern {
        Some(p) => p,
        None => return "缺少 pattern 参数".to_string(),
    };

    let start_line = v.get("start_line").and_then(|n| n.as_u64());
    let end_line = v.get("end_line").and_then(|n| n.as_u64());

    let start_line = match start_line {
        Some(n) if n >= 1 => Some(n as usize),
        Some(_) => return "错误：start_line 必须是大于等于 1 的整数".to_string(),
        None => None,
    };
    let end_line = match end_line {
        Some(n) if n >= 1 => Some(n as usize),
        Some(_) => return "错误：end_line 必须是大于等于 1 的整数".to_string(),
        None => None,
    };
    if let (Some(s), Some(e)) = (start_line, end_line)
        && e < s {
            return "错误：end_line 不能小于 start_line".to_string();
        }

    let max_matches = v
        .get("max_matches")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(50);
    let case_insensitive = v
        .get("case_insensitive")
        .and_then(|b| b.as_bool())
        .unwrap_or(true);
    let max_snippet_chars = v
        .get("max_snippet_chars")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(400);
    let mode = v
        .get("mode")
        .and_then(|m| m.as_str())
        .map(|s| s.trim().to_lowercase())
        .unwrap_or_else(|| "lines".to_string());
    let max_block_chars = v
        .get("max_block_chars")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(8000);
    let max_block_lines = v
        .get("max_block_lines")
        .and_then(|n| n.as_u64())
        .map(|n| n.max(1) as usize)
        .unwrap_or(500);

    let target = match resolve_for_read(working_dir, &path) {
        Ok(p) => p,
        Err(e) => return e,
    };
    if !target.is_file() {
        return "错误：路径不是文件或不存在，无法读取".to_string();
    }

    let content = match std::fs::read_to_string(&target) {
        Ok(s) => s,
        Err(e) => return format!("读取文件失败: {}", e),
    };
    let all_lines: Vec<&str> = content.lines().collect();
    let total = all_lines.len();
    if total == 0 {
        return format!("文件为空: {}", target.display());
    }

    let from = start_line.unwrap_or(1);
    let to = end_line.unwrap_or(total);
    if from > total {
        return format!("错误：start_line 超出文件总行数（总行数: {}）", total);
    }
    let to = to.min(total);

    let re = match RegexBuilder::new(&pattern)
        .case_insensitive(case_insensitive)
        .build()
    {
        Ok(r) => r,
        Err(e) => return format!("错误：无效的正则表达式：{}", e),
    };

    if mode == "lines" {
        let mut matches: Vec<(usize, String)> = Vec::new();
        for idx in from..=to {
            let line = all_lines[idx - 1];
            if re.is_match(line) {
                matches.push((idx, truncate_line(line, max_snippet_chars)));
                if matches.len() >= max_matches {
                    break;
                }
            }
        }

        if matches.is_empty() {
            return format!(
                "未找到匹配：pattern=\"{}\"（文件: {}, 行范围 {}-{}）",
                pattern,
                target.display(),
                from,
                to
            );
        }

        let mut out = String::new();
        out.push_str(&format!(
            "文件: {}\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n匹配结果（最多 {} 条，实际 {} 条）：\n",
            target.display(),
            pattern,
            from,
            to,
            total,
            max_matches,
            matches.len()
        ));
        for (line_no, line) in matches {
            out.push_str(&format!("{}|{}\n", line_no, line));
        }
        return out.trim_end().to_string();
    }

    if mode != "rust_fn_block" {
        return format!(
            "错误：不支持的 mode=\"{}\"（仅支持 \"lines\" 或 \"rust_fn_block\"）",
            mode
        );
    }

    // Rust 函数块提取：从匹配行开始找后续第一个 `{`，再按花括号配对抓到块结束。
    let mut blocks: Vec<(usize, usize, String)> = Vec::new(); // (start_line, end_line, text)
    for idx in from..=to {
        let line = all_lines[idx - 1];
        if !re.is_match(line) {
            continue;
        }

        let block =
            match extract_rust_brace_block(&all_lines, idx, max_block_lines, max_block_chars) {
                Ok(Some((s, e, txt))) => (s, e, txt),
                Ok(None) => continue,
                Err(e) => return e,
            };
        if blocks.len() >= max_matches {
            break;
        }
        blocks.push(block);
    }

    if blocks.is_empty() {
        return format!(
            "未找到 Rust 代码块：pattern=\"{}\"（文件: {}, 行范围 {}-{}）",
            pattern,
            target.display(),
            from,
            to
        );
    }

    let mut out = String::new();
    out.push_str(&format!(
        "文件: {}\nmode: rust_fn_block\npattern: \"{}\"\n行范围: {}-{} / 总行数 {}\n块结果（最多 {} 条，实际 {} 条）：\n",
        target.display(),
        pattern,
        from,
        to,
        total,
        max_matches,
        blocks.len()
    ));
    for (s, e, txt) in blocks {
        out.push_str(&format!("block: {}-{}\n", s, e));
        out.push_str(&format!("{}\n", txt));
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn truncate_line(s: &str, max_chars: usize) -> String {
    let s = s.trim_end();
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i >= max_chars {
                break;
            }
            out.push(ch);
        }
        format!("{}... (截断)", out)
    }
}

/// 从 start_line（1-based）开始向后提取 `{ ... }` 配对块。
/// 说明：会在扫描时跳过注释/字符串/原始字符串/字符字面量里的 `{`/`}`，
/// 以避免花括号误判块边界。
fn extract_rust_brace_block(
    all_lines: &[&str],
    start_line_1based: usize,
    max_block_lines: usize,
    max_block_chars: usize,
) -> Result<Option<(usize, usize, String)>, String> {
    if start_line_1based == 0 {
        return Ok(None);
    }
    let start_idx = start_line_1based - 1;
    if start_idx >= all_lines.len() {
        return Ok(None);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ScanState {
        Normal,
        LineComment,
        BlockComment,
        StringLit { escape: bool },
        CharLit { escape: bool },
        RawString { hash_count: usize },
    }

    let mut state = ScanState::Normal;
    let mut brace_count: i32 = 0;
    let mut started = false;
    let mut end_line: Option<usize> = None;

    // 扫描成本上限，避免极端文件导致非常大的扫描开销
    let mut char_budget: usize = max_block_chars.saturating_mul(3);

    for (line_idx, line) in all_lines.iter().enumerate().skip(start_idx) {
        if line_idx >= start_idx + max_block_lines || end_line.is_some() || char_budget == 0 {
            break;
        }

        let line = *line;
        let chars: Vec<char> = line.chars().collect();
        let mut pos: usize = 0;

        while pos < chars.len() {
            if char_budget == 0 {
                break;
            }
            let ch = chars[pos];
            char_budget = char_budget.saturating_sub(1);

            match state {
                ScanState::Normal => {
                    // // ... 直到行尾
                    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
                        state = ScanState::LineComment;
                        pos += 2;
                        continue;
                    }
                    // /* ... */
                    if ch == '/' && pos + 1 < chars.len() && chars[pos + 1] == '*' {
                        state = ScanState::BlockComment;
                        pos += 2;
                        continue;
                    }

                    // 原始字符串 r###" ... "###
                    if ch == 'r' || ch == 'R' {
                        // r" ... "
                        if pos + 1 < chars.len() && chars[pos + 1] == '"' {
                            state = ScanState::RawString { hash_count: 0 };
                            pos += 2;
                            continue;
                        }

                        // r#"... "#  /  r##"... "## ...
                        if pos + 1 < chars.len() && chars[pos + 1] == '#' {
                            let mut hash_count = 0usize;
                            let mut j = pos + 1;
                            while j < chars.len() && chars[j] == '#' {
                                hash_count += 1;
                                j += 1;
                            }
                            if j < chars.len() && chars[j] == '"' {
                                state = ScanState::RawString { hash_count };
                                pos = j + 1;
                                continue;
                            }
                        }
                    }

                    // 字符串
                    if ch == '"' {
                        state = ScanState::StringLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    // 字符字面量
                    if ch == '\'' {
                        state = ScanState::CharLit { escape: false };
                        pos += 1;
                        continue;
                    }

                    // brace counting (只在 Normal 状态)
                    if !started {
                        if ch == '{' {
                            started = true;
                            brace_count = 1;
                        }
                    } else if ch == '{' {
                        brace_count += 1;
                    } else if ch == '}' {
                        brace_count -= 1;
                        if brace_count == 0 {
                            end_line = Some(line_idx);
                            break;
                        }
                    }

                    pos += 1;
                }
                ScanState::LineComment => {
                    // 跳过到行尾
                    break;
                }
                ScanState::BlockComment => {
                    if ch == '*' && pos + 1 < chars.len() && chars[pos + 1] == '/' {
                        state = ScanState::Normal;
                        pos += 2;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::StringLit { escape } => {
                    if escape {
                        state = ScanState::StringLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    if ch == '\\' {
                        state = ScanState::StringLit { escape: true };
                        pos += 1;
                        continue;
                    }
                    if ch == '"' {
                        state = ScanState::Normal;
                        pos += 1;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::CharLit { escape } => {
                    if escape {
                        state = ScanState::CharLit { escape: false };
                        pos += 1;
                        continue;
                    }
                    if ch == '\\' {
                        state = ScanState::CharLit { escape: true };
                        pos += 1;
                        continue;
                    }
                    if ch == '\'' {
                        state = ScanState::Normal;
                        pos += 1;
                        continue;
                    }
                    pos += 1;
                }
                ScanState::RawString { hash_count } => {
                    if ch == '"' {
                        // 检查后续是否为 hash_count 个 # 组成的结束定界符
                        let mut ok = true;
                        for k in 0..hash_count {
                            if pos + 1 + k >= chars.len() || chars[pos + 1 + k] != '#' {
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            state = ScanState::Normal;
                            pos = pos + 1 + hash_count;
                            continue;
                        }
                    }
                    pos += 1;
                }
            }
        }

        // // ... 在下一行会自动回到 Normal
        if state == ScanState::LineComment {
            state = ScanState::Normal;
        }
    }

    let end_line = match end_line {
        Some(e) => e,
        None => return Ok(None),
    };

    let text = all_lines[start_idx..=end_line].join("\n");
    let text_trunc = truncate_by_chars(&text, max_block_chars);
    Ok(Some((start_line_1based, end_line + 1, text_trunc)))
}

fn truncate_by_chars(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out = String::new();
        for (i, ch) in s.chars().enumerate() {
            if i >= max_chars {
                break;
            }
            out.push(ch);
        }
        format!("{}... (截断)", out)
    }
}

fn parse_path_content(args_json: &str) -> Result<(String, String), String> {
    let v: serde_json::Value =
        serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {}", e))?;
    let path = v
        .get("path")
        .and_then(|p| p.as_str())
        .map(String::from)
        .ok_or_else(|| "缺少 path 参数".to_string())?;
    let content = v
        .get("content")
        .and_then(|c| c.as_str())
        .map(String::from)
        .unwrap_or_default();
    Ok((path, content))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(1);

    fn make_test_dir() -> PathBuf {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let seq = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "crabmate_file_tool_test_{}_{}_{}",
            std::process::id(),
            ts,
            seq
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_read_file_with_line_range() {
        let dir = make_test_dir();
        let file = dir.join("a.txt");
        std::fs::write(&file, "a\nb\nc\nd\n").unwrap();
        let out = read_file(r#"{"path":"a.txt","start_line":2,"end_line":3}"#, &dir);
        assert!(out.contains("2|b"), "应包含第 2 行: {}", out);
        assert!(out.contains("3|c"), "应包含第 3 行: {}", out);
        assert!(!out.contains("1|a"), "不应包含第 1 行: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_respects_max_lines_without_end_line() {
        let dir = make_test_dir();
        let file = dir.join("big.txt");
        let mut s = String::new();
        for i in 1..=1200 {
            s.push_str(&format!("line{i}\n"));
        }
        std::fs::write(&file, &s).unwrap();
        let out = read_file(r#"{"path":"big.txt","max_lines":100}"#, &dir);
        assert!(out.contains("仍有后续内容"), "应提示分段: {}", out);
        assert!(out.contains("下一段可将 start_line 设为 101"), "{}", out);
        assert!(out.contains("100|line100"), "{}", out);
        assert!(!out.contains("101|line101"), "不应超过 max_lines: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_binary_meta_prefix_hash() {
        let dir = make_test_dir();
        let file = dir.join("bin.dat");
        std::fs::write(&file, [1u8, 2, 3, 4, 5]).unwrap();
        let out = read_binary_meta(
            r#"{"path":"bin.dat","prefix_hash_bytes":64}"#,
            &dir,
        );
        assert!(out.contains("size_bytes: 5"), "{}", out);
        assert!(out.contains("sha256_prefix:"), "{}", out);
        assert!(out.contains("sha256_prefix_bytes: 5"), "{}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_binary_meta_skip_hash() {
        let dir = make_test_dir();
        let file = dir.join("x.bin");
        std::fs::write(&file, b"x").unwrap();
        let out = read_binary_meta(r#"{"path":"x.bin","prefix_hash_bytes":0}"#, &dir);
        assert!(out.contains("size_bytes: 1"), "{}", out);
        assert!(out.contains("已跳过"), "{}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_modify_file_replace_lines() {
        let dir = make_test_dir();
        let file = dir.join("m.txt");
        std::fs::write(&file, "L1\nL2\nL3\nL4\n").unwrap();
        let out = modify_file(
            r#"{"path":"m.txt","mode":"replace_lines","start_line":2,"end_line":3,"content":"X"}"#,
            &dir,
        );
        assert!(out.contains("已按行替换"), "{}", out);
        let body = std::fs::read_to_string(&file).unwrap();
        assert_eq!(body, "L1\nX\nL4\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_reject_invalid_range() {
        let dir = make_test_dir();
        let file = dir.join("a.txt");
        std::fs::write(&file, "x\n").unwrap();
        let out = read_file(r#"{"path":"a.txt","start_line":3}"#, &dir);
        assert!(out.contains("超出文件行数"), "应报越界错误: {}", out);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_rust_fn_block() {
        let dir = make_test_dir();
        let file = dir.join("a.rs");
        let content = r##"
pub fn foo(x: i32) -> i32 {
    // braces in line comment: { }
    let s1 = "{";
    let s2 = "}";
    let s3 = r#"{"a":1}"#; // braces inside raw string
    let s4 = r#"}"#;        // raw string with '}' earlier than function end
    /* block comment with { and } should be ignored: { } */
    let c = '}';

    let _ = some_macro!({
        // comment with { } inside macro invocation should not break extraction
        println!("macro {{ }} {}", x);
        if x > 0 { x + 1 } else { x - 1 }
    });

    // The real return is still from the outer if/else, so braces above must not affect boundaries.
    if x > 0 {
        x + 1 // { in comment { }
    } else {
        x - 1
    }
}

pub fn bar() { println!("hi"); }
"##;
        std::fs::write(&file, content).unwrap();

        let out = extract_in_file(
            r#"{"path":"a.rs","pattern":"pub\\s+fn\\s+foo","mode":"rust_fn_block","max_matches":1,"max_block_lines":200,"max_block_chars":2000}"#,
            &dir,
        );
        assert!(out.contains("pub fn foo"));
        assert!(out.contains("else"));
        assert!(out.contains("x - 1"));
        assert!(out.contains("let s4 = r#\"}\"#;"));
        assert!(out.trim_end().ends_with('}'));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_read_file_reject_outside_workspace() {
        let dir = make_test_dir();
        let outside_name = format!("crabmate_outside_read_{}.txt", std::process::id());
        let outside = std::env::temp_dir().join(&outside_name);
        std::fs::write(&outside, "outside\n").unwrap();
        let arg = serde_json::json!({ "path": format!("../{}", outside_name) }).to_string();
        let out = read_file(&arg, &dir);
        assert!(
            out.contains("路径不能超出工作目录"),
            "应拒绝越界读取: {}",
            out
        );
        let _ = std::fs::remove_file(&outside);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn test_create_file_reject_symlink_escape() {
        use std::os::unix::fs::symlink;

        let dir = make_test_dir();
        let outside = std::env::temp_dir().join(format!(
            "crabmate_outside_symlink_{}_{}",
            std::process::id(),
            TEST_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&outside).unwrap();
        let link = dir.join("link_out");
        symlink(&outside, &link).unwrap();

        let out = create_file(r#"{"path":"link_out/pwned.txt","content":"x"}"#, &dir);
        assert!(
            out.contains("路径不能超出工作目录"),
            "应拒绝 symlink 绕过写入: {}",
            out
        );

        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&outside);
    }
}
