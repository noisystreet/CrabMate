//! Typed tool-argument shapes for **dynamic** chat summaries (`ToolSummaryKind::Dynamic`).
//! Field names match tool JSON schemas; extra keys are ignored (no `deny_unknown_fields`).
//! Parsing uses `serde_json::from_value`; on failure the caller yields no summary (same as
//! missing required fields in the previous `Value::get` style).

use serde::Deserialize;

/// Deserialize from a clone of `v`, then build the summary line.
pub(super) fn summarize_from_value<T>(v: &serde_json::Value) -> Option<String>
where
    T: serde::de::DeserializeOwned + ToolSummaryLine,
{
    let t: T = serde_json::from_value(v.clone()).ok()?;
    t.summary_line()
}

pub(super) trait ToolSummaryLine {
    fn summary_line(self) -> Option<String>;
}

/// #1157：摘要与 Args **同源**后的实现形态——从 args JSON 宽容取值。
///
/// 为什么不直接 `serde_json::from_value::<XArgs>`：`tool_param_types` 的 Args 普遍带
/// `#[serde(deny_unknown_fields)]`（schema 需要 `additionalProperties: false`），而模型
/// 给的 `arguments` 常多带一两个字段。旧的 `*SummaryArgs` 是**故意**不带该属性、只声明
/// 摘要用得到的字段子集。若改成反序列化整个 Args，「参数合法但多一个字段」会让摘要
/// **静默消失**——Chat 面板少一行，且没有任何错误提示。因此摘要侧只按字段名取值。
pub(super) trait ToolArgsSummary {
    /// 缺失 / 类型不符的字段按「没给」处理，不影响其余部分。
    fn summary_line(args: &serde_json::Value) -> Option<String>;
}

/// 取字符串参数：缺失、类型不符、全空白都算「没给」。
pub(super) fn arg_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

/// 取非负整数参数：缺失或类型不符算「没给」。
pub(super) fn arg_u64(args: &serde_json::Value, key: &str) -> Option<u64> {
    args.get(key).and_then(serde_json::Value::as_u64)
}

include!("tool_summary_args/fragment_args_summary.rs");
include!("tool_summary_args/fragment_core.rs");
include!("tool_summary_args/fragment_git_files_diff.rs");
include!("tool_summary_args/fragment_git_files_file_ops.rs");
include!("tool_summary_args/fragment_git_files_tail.rs");
include!("tool_summary_args/fragment_gh_archive.rs");
