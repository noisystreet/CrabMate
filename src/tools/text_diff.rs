//! 纯文本行级 unified diff（与 Git 无关）：两段字符串或工作区内两个文件。

use std::fs;
use std::path::Path;

use serde_json::Value;
use similar::TextDiff;

use super::file;
use super::output_util;

/// 内联模式每侧最大字节
const MAX_INLINE_BYTES: usize = 256 * 1024;
/// 与工作区文件读取上限对齐（`structured_data`）
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const DEFAULT_CONTEXT: usize = 3;
const MAX_CONTEXT: usize = 20;
const DEFAULT_MAX_OUTPUT: usize = 50_000;
const ABS_MAX_OUTPUT: usize = 500_000;

fn parse_mode(v: &Value) -> Result<&'static str, String> {
    let s = v
        .get("mode")
        .and_then(|x| x.as_str())
        .unwrap_or("inline")
        .trim()
        .to_lowercase();
    match s.as_str() {
        "inline" => Ok("inline"),
        "paths" => Ok("paths"),
        _ => Err("mode 仅支持 inline 或 paths".to_string()),
    }
}

fn parse_usize(
    v: &Value,
    key: &str,
    default: usize,
    min: usize,
    max: usize,
) -> Result<usize, String> {
    let n = match v.get(key) {
        None => default,
        Some(n) => n.as_u64().ok_or_else(|| format!("{} 须为正整数", key))? as usize,
    };
    if n < min || n > max {
        return Err(format!("{} 须在 {}～{} 之间", key, min, max));
    }
    Ok(n)
}

fn parse_max_output(v: &Value) -> Result<usize, String> {
    parse_usize(v, "max_output_bytes", DEFAULT_MAX_OUTPUT, 1, ABS_MAX_OUTPUT)
}

fn read_workspace_text(path: &str, base: &Path) -> Result<String, String> {
    let pb =
        file::resolve_for_read(base, path).map_err(|e| format!("错误：{}", e.user_message()))?;
    let meta = fs::metadata(&pb).map_err(|e| format!("读取元数据失败: {}", e))?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "文件过大：{} 字节，上限 {}",
            meta.len(),
            MAX_FILE_BYTES
        ));
    }
    fs::read_to_string(&pb).map_err(|e| format!("读取文件失败（须为 UTF-8 文本）: {}", e))
}

fn diff_unified(left: &str, right: &str, header_a: &str, header_b: &str, context: usize) -> String {
    let diff = TextDiff::from_lines(left, right);
    diff.unified_diff()
        .context_radius(context)
        .header(header_a, header_b)
        .to_string()
}

/// 执行 `text_diff` 工具。
pub fn run(args_json: &str, workspace_root: &Path) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };

    let mode = match parse_mode(&v) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let context = match parse_usize(&v, "context_lines", DEFAULT_CONTEXT, 0, MAX_CONTEXT) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let max_out = match parse_max_output(&v) {
        Ok(m) => m,
        Err(e) => return e,
    };

    let (left, right, ha, hb) = match mode {
        "inline" => {
            let left = match v.get("left").and_then(|x| x.as_str()) {
                Some(s) => s,
                None => return "inline 模式需要 left（字符串）".to_string(),
            };
            let right = match v.get("right").and_then(|x| x.as_str()) {
                Some(s) => s,
                None => return "inline 模式需要 right（字符串）".to_string(),
            };
            if left.len() > MAX_INLINE_BYTES {
                return format!("left 过长：{} 字节，上限 {}", left.len(), MAX_INLINE_BYTES);
            }
            if right.len() > MAX_INLINE_BYTES {
                return format!(
                    "right 过长：{} 字节，上限 {}",
                    right.len(),
                    MAX_INLINE_BYTES
                );
            }
            (
                left.to_string(),
                right.to_string(),
                "inline/left".to_string(),
                "inline/right".to_string(),
            )
        }
        "paths" => {
            let pa = match v.get("left_path").and_then(|x| x.as_str()) {
                Some(s) if !s.trim().is_empty() => s.trim(),
                _ => return "paths 模式需要 left_path（相对工作区的非空路径）".to_string(),
            };
            let pb = match v.get("right_path").and_then(|x| x.as_str()) {
                Some(s) if !s.trim().is_empty() => s.trim(),
                _ => return "paths 模式需要 right_path（相对工作区的非空路径）".to_string(),
            };
            let left = match read_workspace_text(pa, workspace_root) {
                Ok(s) => s,
                Err(e) => return e,
            };
            let right = match read_workspace_text(pb, workspace_root) {
                Ok(s) => s,
                Err(e) => return e,
            };
            let ha = format!("a/{}", pa.replace('\\', "/"));
            let hb = format!("b/{}", pb.replace('\\', "/"));
            (left, right, ha, hb)
        }
        _ => unreachable!(),
    };

    let unified = diff_unified(&left, &right, &ha, &hb, context);
    output_util::truncate_output_bytes(&unified, max_out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_diff_has_hunk() {
        let j = serde_json::json!({
            "mode": "inline",
            "left": "a\nb\nc",
            "right": "a\nx\nc",
            "context_lines": 1,
            "max_output_bytes": 10000
        });
        let s = run(&j.to_string(), Path::new("/tmp"));
        assert!(s.contains("@@") || s.contains("---"));
        assert!(s.contains("-b") || s.contains("+x"));
    }
}
