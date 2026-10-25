//! 纯文本行级 unified diff（与 Git 无关）：两段字符串或工作区内两个文件。

use std::fs;
use std::path::Path;

use serde_json::Value;
use similar::TextDiff;

use super::file;
use super::output_util;
use super::tool_param_types::TextDiffArgs;
use crate::cm_tools::tool_result::ToolError;

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

#[allow(clippy::result_large_err)]
fn read_workspace_text(path: &str, base: &Path) -> Result<String, ToolError> {
    let pb = file::resolve_for_read(base, path).map_err(ToolError::path_resolve_failed)?;
    let meta = fs::metadata(&pb).map_err(|e| {
        ToolError::external_code("text_diff_io_failed", format!("读取元数据失败: {e}"))
    })?;
    if meta.len() > MAX_FILE_BYTES {
        return Err(ToolError::invalid_args(format!(
            "文件过大：{} 字节，上限 {}",
            meta.len(),
            MAX_FILE_BYTES
        )));
    }
    fs::read_to_string(&pb).map_err(|e| {
        ToolError::external_code(
            "text_diff_io_failed",
            format!("读取文件失败（须为 UTF-8 文本）: {e}"),
        )
    })
}

/// 生成 unified diff；供本工具与 `structured_patch` 的写前预览共用（同一套 diff 渲染）。
pub(super) fn diff_unified(
    left: &str,
    right: &str,
    header_a: &str,
    header_b: &str,
    context: usize,
) -> String {
    let diff = TextDiff::from_lines(left, right);
    diff.unified_diff()
        .context_radius(context)
        .header(header_a, header_b)
        .to_string()
}

#[allow(clippy::result_large_err)]
fn load_inline_diff_sources(v: &Value) -> Result<(String, String, String, String), ToolError> {
    let left = match v.get("left").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => {
            return Err(ToolError::invalid_args(
                "inline 模式需要 left（字符串）".to_string(),
            ));
        }
    };
    let right = match v.get("right").and_then(|x| x.as_str()) {
        Some(s) => s,
        None => {
            return Err(ToolError::invalid_args(
                "inline 模式需要 right（字符串）".to_string(),
            ));
        }
    };
    if left.len() > MAX_INLINE_BYTES {
        return Err(ToolError::invalid_args(format!(
            "left 过长：{} 字节，上限 {}",
            left.len(),
            MAX_INLINE_BYTES
        )));
    }
    if right.len() > MAX_INLINE_BYTES {
        return Err(ToolError::invalid_args(format!(
            "right 过长：{} 字节，上限 {}",
            right.len(),
            MAX_INLINE_BYTES
        )));
    }
    Ok((
        left.to_string(),
        right.to_string(),
        "inline/left".to_string(),
        "inline/right".to_string(),
    ))
}

#[allow(clippy::result_large_err)]
fn load_paths_diff_sources(
    v: &Value,
    workspace_root: &Path,
) -> Result<(String, String, String, String), ToolError> {
    let pa = match v.get("left_path").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Err(ToolError::invalid_args(
                "paths 模式需要 left_path（相对工作区的非空路径）".to_string(),
            ));
        }
    };
    let pb = match v.get("right_path").and_then(|x| x.as_str()) {
        Some(s) if !s.trim().is_empty() => s.trim(),
        _ => {
            return Err(ToolError::invalid_args(
                "paths 模式需要 right_path（相对工作区的非空路径）".to_string(),
            ));
        }
    };
    let left = read_workspace_text(pa, workspace_root)?;
    let right = read_workspace_text(pb, workspace_root)?;
    let ha = format!("a/{}", pa.replace('\\', "/"));
    let hb = format!("b/{}", pb.replace('\\', "/"));
    Ok((left, right, ha, hb))
}

/// 执行 `text_diff` 工具：参数缺省 / 超限为 `invalid_args`，路径解析走 `path_resolve_failed`。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str, workspace_root: &Path) -> Result<String, ToolError> {
    let args: TextDiffArgs =
        crate::cm_tools::tools::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let v = serde_json::to_value(&args).map_err(|e| {
        ToolError::internal_code("text_diff_serialize_failed", format!("参数序列化错误: {e}"))
    })?;

    let mode = parse_mode(&v).map_err(ToolError::invalid_args)?;
    let context = parse_usize(&v, "context_lines", DEFAULT_CONTEXT, 0, MAX_CONTEXT)
        .map_err(ToolError::invalid_args)?;
    let max_out = parse_max_output(&v).map_err(ToolError::invalid_args)?;

    let (left, right, ha, hb) = match mode {
        "inline" => load_inline_diff_sources(&v)?,
        "paths" => load_paths_diff_sources(&v, workspace_root)?,
        other => {
            return Err(ToolError::internal_code(
                "text_diff_internal",
                format!("内部错误：未知 mode {other}"),
            ));
        }
    };

    let unified = diff_unified(&left, &right, &ha, &hb, context);
    Ok(output_util::truncate_output_bytes(&unified, max_out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_str(args_json: &str) -> String {
        match run_try(args_json, Path::new("/tmp")) {
            Ok(s) => s,
            Err(e) => e.message,
        }
    }

    #[test]
    fn inline_diff_has_hunk() {
        let j = serde_json::json!({
            "mode": "inline",
            "left": "a\nb\nc",
            "right": "a\nx\nc",
            "context_lines": 1,
            "max_output_bytes": 10000
        });
        let s = run_str(&j.to_string());
        // header 两行逐字 + 成功正文不得被注入 `crabmate_tool_output` 头。
        assert!(
            s.starts_with("--- inline/left\n+++ inline/right\n"),
            "header changed: {s}"
        );
        assert!(!s.contains("crabmate_tool_output"), "{s}");
        assert!(s.contains("@@") || s.contains("---"));
        assert!(s.contains("-b") || s.contains("+x"));
    }

    /// `mode` 的取值集合由 `TextDiffMode` 枚举在反序列化阶段把关，
    /// 非法值到不了 `parse_mode`（那里的 `mode 仅支持 inline 或 paths` 是不可达的防御分支）。
    #[test]
    fn unknown_mode_is_rejected_before_runner() {
        let j = serde_json::json!({ "mode": "sideways", "left": "a", "right": "b" });
        let err = run_try(&j.to_string(), Path::new("/tmp")).expect_err("unknown mode must fail");
        assert_eq!(err.code, "invalid_args");
        assert!(
            err.message.contains("unknown variant") && err.message.contains("sideways"),
            "expected a serde error naming the variant: {}",
            err.message
        );
    }

    #[test]
    fn missing_inline_operand_is_invalid_args() {
        let j = serde_json::json!({ "mode": "inline", "left": "a" });
        let err = run_try(&j.to_string(), Path::new("/tmp")).expect_err("missing right must fail");
        assert_eq!(err.code, "invalid_args");
        assert!(err.message.contains("inline 模式需要 right"));
    }
}
