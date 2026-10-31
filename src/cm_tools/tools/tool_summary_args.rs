//! 摘要侧的「Args 同源」取值层（#1157）。
//!
//! 每个动态摘要工具在 `tool_summary_args/` 里写一个 `impl ToolArgsSummary for XArgs`，
//! `builtin_tool! { summary: dynamic }` 直接从 Args 类型派生摘要行。
//!
//! **不要** `serde_json::from_value::<XArgs>`：Args 普遍带 `deny_unknown_fields`（schema 需要
//! `additionalProperties: false`），模型多传一个字段就会让摘要静默消失。字段级取值一律走下面的
//! `arg_*` helper——它们把「缺键 / `null` 算没给」与「类型不符算失败」分开表达。

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

/// 取布尔参数：缺失或类型不符算「没给」（默认值由调用方决定）。
pub(super) fn arg_bool(args: &serde_json::Value, key: &str) -> Option<bool> {
    args.get(key).and_then(serde_json::Value::as_bool)
}

/// 取**原样**字符串参数：不 trim、不把空串当「没给」。
///
/// 迁移时用于旧结构体 `Option<String>` + `unwrap_or(默认值)` 的语义（`code_stats` 的
/// 缺省 `.`、`dependency_graph` 的缺省 `mermaid` 等）：`arg_str` 会把空白当「没给」并
/// 回落默认值，那会改变「显式传空白」时的输出，所以这类字段走这个更贴近原语义的取值。
pub(super) fn arg_str_raw<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(serde_json::Value::as_str)
}

/// `Option<String>` 语义：缺键 / `null` ⇒ `Some(None)`；类型不符 ⇒ `None`（放弃整条摘要）。
///
/// #1157：旧 `*SummaryArgs` 的宽容度是**逐字段**不同的——`Option<T>` / `Vec<T>` 缺键可以被
/// `#[serde(default)]` 放过，但**类型不符一定整体反序列化失败**（= 没有摘要）。摘要改成从
/// `&Value` 手取值后，最顺手的退化就是「类型不符也被当成没给」——摘要会凭空多出一行，且没有
/// 任何报错。所以这类字段一律走下面三个 helper，别直接 `args.get(..)?.as_str()`。
pub(super) fn arg_opt_str<'a>(args: &'a serde_json::Value, key: &str) -> Option<Option<&'a str>> {
    match args.get(key) {
        None => Some(None),
        Some(v) if v.is_null() => Some(None),
        Some(serde_json::Value::String(s)) => Some(Some(s)),
        Some(_) => None,
    }
}

/// `Vec<String>` + `#[serde(default)]` 语义：缺键 ⇒ 空数组；`null` / 非数组 / 含非字符串元素
/// ⇒ `None`（放弃整条摘要）。
pub(super) fn arg_str_vec<'a>(args: &'a serde_json::Value, key: &str) -> Option<Vec<&'a str>> {
    match args.get(key) {
        None => Some(Vec::new()),
        Some(v) => v.as_array()?.iter().map(|item| item.as_str()).collect(),
    }
}

/// `Option<u64>` 语义：缺键 / `null` ⇒ `Some(None)`；非整数或负数 ⇒ `None`（放弃整条摘要）。
pub(super) fn arg_opt_u64(args: &serde_json::Value, key: &str) -> Option<Option<u64>> {
    match args.get(key) {
        None => Some(None),
        Some(v) if v.is_null() => Some(None),
        Some(v) => v.as_u64().map(Some),
    }
}

/// `Option<bool>` 语义：缺键 / `null` ⇒ `Some(None)`；非布尔 ⇒ `None`（放弃整条摘要）。
pub(super) fn arg_opt_bool(args: &serde_json::Value, key: &str) -> Option<Option<bool>> {
    match args.get(key) {
        None => Some(None),
        Some(v) if v.is_null() => Some(None),
        Some(v) => v.as_bool().map(Some),
    }
}

/// `bool` + `#[serde(default)]` 语义：缺键 ⇒ `Some(false)`；`null` / 非布尔 ⇒ `None`。
pub(super) fn arg_bool_default(args: &serde_json::Value, key: &str) -> Option<bool> {
    match args.get(key) {
        None => Some(false),
        Some(v) => v.as_bool(),
    }
}

/// `String` + `#[serde(default)]` 语义：缺键 ⇒ `Some("")`；`null` / 非字符串 ⇒ `None`。
pub(super) fn arg_str_default<'a>(args: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    match args.get(key) {
        None => Some(""),
        Some(v) => v.as_str(),
    }
}

include!("tool_summary_args/fragment_args_summary.rs");
include!("tool_summary_args/fragment_core.rs");
include!("tool_summary_args/fragment_git_files_diff.rs");
include!("tool_summary_args/fragment_web_search.rs");
include!("tool_summary_args/fragment_file_ops.rs");
include!("tool_summary_args/fragment_gh_archive.rs");
