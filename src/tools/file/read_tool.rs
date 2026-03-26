//! 由 `file.rs` 拆分；与拆分前行为一致。
#![allow(clippy::manual_string_new)]

use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use super::path::{path_for_tool_display, resolve_for_read};

/// 单次 read_file 默认最多返回的行数（防撑爆上下文）
const READ_FILE_DEFAULT_MAX_LINES: usize = 500;
/// read_file 允许的单次上限
const READ_FILE_ABS_MAX_LINES: usize = 8000;
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
        && e < start_line
    {
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
        return format!(
            "文件为空: {}",
            path_for_tool_display(working_dir, &target, Some(&path))
        );
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
    out.push_str(&format!(
        "文件: {}\n",
        path_for_tool_display(working_dir, &target, Some(&path))
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
        let n = reader
            .read_line(&mut buf)
            .map_err(|e| format!("读取失败: {}", e))?;
        if n == 0 {
            break;
        }
        count += 1;
    }
    Ok(count)
}
