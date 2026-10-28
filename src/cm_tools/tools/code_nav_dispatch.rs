//! `code_nav` 工具的 `action` 选择器分派层（2026-10-25 四合一）。
//!
//! 四个既有实现（`symbol::run` / `code_nav::find_references` /
//! `code_nav::rust_file_outline` / `call_graph_sketch::run`）原样保留，这里只把 union 入参
//! 拆回各自的 per-action 形状；合并动的是对外 schema，不重写已验证的检索流程。
//!
//! 为什么需要逐 action 重组 JSON：子 DTO 都是 `deny_unknown_fields`，把 union 原始 JSON
//! 直接透传会让「别的 action 的字段」被判成未知键（如 `find_symbol` 顺带塞了 `max_edges`），
//! 所以每个分支只挑本动作读的字段。代价是一次小体量 JSON 往返。
//!
//! per-action 必填（`find_symbol` / `find_references` 需 `symbol`，`file_outline` 需 `path`，
//! `call_graph_sketch` 需 `symbol` 或 `symbols`）由本层校验——union 结构体表达不了这种条件必填，
//! 缺失时报错文案直接说明该动作要什么。

use std::path::Path;

use serde_json::json;

use crate::cm_tools::tool_result::ToolError;
use crate::cm_tools::tools::tool_param_types::{CodeNavAction, CodeNavArgs};
use crate::cm_tools::tools::{
    call_graph_sketch, code_nav, parse_args_json, parse_args_typed, symbol,
};

/// 缺 / 未知 `action` 的报错里列出的可选值。
pub const CODE_NAV_ACTIONS: &str = "find_symbol / find_references / file_outline / call_graph_sketch";

/// `code_nav` 唯一入口：按 `action` 分派到四个只读实现。
///
/// 迁 `Typed` 后 per-action 必填缺失是显式 `invalid_args`（旧版本靠「错误：」前缀让
/// `parse_legacy_output` 恰好判对），子实现失败则原样透传各自的码
/// （`code_nav_path_*` 属策略拒绝、`code_nav_search_failed` / `code_nav_read_failed` 属 IO）。
#[allow(clippy::result_large_err)]
pub fn dispatch_try(args_json: &str, working_dir: &Path) -> Result<String, ToolError> {
    // 先宽松取一次 action：这样「完全没有 action」能给出可选值清单，
    // 而不是 serde 那句只有字段名的 missing field。
    let raw = parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let has_action = raw
        .get("action")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if !has_action {
        return Err(ToolError::invalid_args(format!(
            "错误：缺少 action（可选值：{CODE_NAV_ACTIONS}）"
        )));
    }
    // 上面的探测只保证 `action` 是非空字符串；非法取值交给 `CodeNavAction` 反序列化报错（serde 会列出全部可选值）。
    let args: CodeNavArgs = parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    match args.action {
        CodeNavAction::FindSymbol => match non_empty(args.symbol.as_deref()) {
            Some(sym) => symbol::run_try(
                &json!({
                    "symbol": sym,
                    "path": args.path,
                    "kind": args.kind,
                    "max_results": args.max_results,
                    "context_lines": args.context_lines,
                    "case_insensitive": args.case_insensitive.unwrap_or(true),
                    "include_hidden": args.include_hidden.unwrap_or(false),
                })
                .to_string(),
                working_dir,
            ),
            None => Err(missing_field(args.action, "symbol")),
        },
        CodeNavAction::FindReferences => match non_empty(args.symbol.as_deref()) {
            Some(sym) => code_nav::find_references_try(
                &json!({
                    "symbol": sym,
                    "path": args.path,
                    "max_results": args.max_results,
                    "case_sensitive": args.case_sensitive.unwrap_or(false),
                    "exclude_definitions": args.exclude_definitions.unwrap_or(true),
                    "include_hidden": args.include_hidden.unwrap_or(false),
                })
                .to_string(),
                working_dir,
            ),
            None => Err(missing_field(args.action, "symbol")),
        },
        CodeNavAction::FileOutline => match non_empty(args.path.as_deref()) {
            Some(path) => code_nav::rust_file_outline_try(
                &json!({
                    "path": path,
                    "include_use": args.include_use,
                    "max_items": args.max_items,
                })
                .to_string(),
                working_dir,
            ),
            None => Err(missing_field(args.action, "path")),
        },
        CodeNavAction::CallGraphSketch => {
            let has_symbol = non_empty(args.symbol.as_deref()).is_some();
            let has_symbols = args.symbols.iter().any(|s| !s.trim().is_empty());
            if !has_symbol && !has_symbols {
                return Err(ToolError::invalid_args(
                    "错误：`call_graph_sketch` 需要 `symbol` 或 `symbols`（至少一个非空）。"
                        .to_string(),
                ));
            }
            call_graph_sketch::run_try(
                &json!({
                    "symbol": args.symbol,
                    "symbols": args.symbols,
                    "path": args.path,
                    "max_edges": args.max_edges,
                    "max_files": args.max_files,
                    "include_hidden": args.include_hidden.unwrap_or(false),
                })
                .to_string(),
                working_dir,
            )
        }
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn missing_field(action: CodeNavAction, field: &str) -> ToolError {
    let name = action.as_str();
    ToolError::invalid_args(format!(
        "错误：`{name}` 需要非空的 `{field}`（可选 action：{CODE_NAV_ACTIONS}）。"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 失败一律取 `Err`：`message` 断文案，`code` 断分类。
    fn err(args: &str) -> ToolError {
        dispatch_try(args, Path::new(".")).expect_err("这些入参都应失败")
    }

    #[test]
    fn missing_action_lists_every_value() {
        let e = err("{}");
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("缺少 action"), "{}", e.message);
        for name in [
            "find_symbol",
            "find_references",
            "file_outline",
            "call_graph_sketch",
        ] {
            assert!(
                e.message.contains(name),
                "缺少可选值 `{name}`: {}",
                e.message
            );
        }
    }

    #[test]
    fn unknown_action_reports_expected_values() {
        let e = err(r#"{"action":"nope"}"#);
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("参数解析错误"), "{}", e.message);
        // serde 的 unknown variant 文案会带 expected 列表，模型据此可自我纠正。
        assert!(e.message.contains("find_symbol"), "{}", e.message);
    }

    #[test]
    fn per_action_required_fields_are_enforced() {
        for args in [
            r#"{"action":"find_symbol"}"#,
            r#"{"action":"find_references"}"#,
            // 全空白等同缺省：trim 后为空即拒，避免拿空白去扫整个工作区。
            r#"{"action":"find_symbol","symbol":"   "}"#,
        ] {
            let e = err(args);
            assert_eq!(e.code, "invalid_args", "{args}");
            assert!(
                e.message.contains("需要非空的 `symbol`"),
                "{args}: {}",
                e.message
            );
        }
        let e = err(r#"{"action":"file_outline"}"#);
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("需要非空的 `path`"), "{}", e.message);
        for args in [
            r#"{"action":"call_graph_sketch"}"#,
            r#"{"action":"call_graph_sketch","symbols":["","  "]}"#,
        ] {
            let e = err(args);
            assert_eq!(e.code, "invalid_args", "{args}");
            assert!(
                e.message.contains("`symbol` 或 `symbols`"),
                "{args}: {}",
                e.message
            );
        }
    }

    #[test]
    fn fields_of_other_actions_are_ignored() {
        // 刻意语义：只挑本 action 读的字段，其余（`symbol` / `max_edges`）不报错。
        // 用 `Cargo.toml` 触发实现层的扩展名检查，避免真的扫工作区。
        let e = err(r#"{"action":"file_outline","path":"Cargo.toml","symbol":"x","max_edges":5}"#);
        assert_eq!(e.code, "code_nav_path_not_rust_file");
        assert!(e.message.contains("仅支持 .rs"), "{}", e.message);
    }

    #[test]
    fn unknown_keys_still_fail() {
        // 「忽略无关字段」不等于放任拼错的键：`deny_unknown_fields` 仍生效。
        let e = err(r#"{"action":"find_symbol","symbol":"x","synbol":"y"}"#);
        assert_eq!(e.code, "invalid_args");
        assert!(e.message.contains("参数解析错误"), "{}", e.message);
        assert!(e.message.contains("synbol"), "{}", e.message);
    }

    #[test]
    fn action_names_match_the_documented_list() {
        // `CODE_NAV_ACTIONS` 是给模型的静态文案，`as_str` 是运行时真源；
        // 两者一旦漂移，模型会看到不存在的可选值。
        for action in [
            CodeNavAction::FindSymbol,
            CodeNavAction::FindReferences,
            CodeNavAction::FileOutline,
            CodeNavAction::CallGraphSketch,
        ] {
            assert!(
                CODE_NAV_ACTIONS.contains(action.as_str()),
                "`{}` 未出现在 CODE_NAV_ACTIONS 中: {CODE_NAV_ACTIONS}",
                action.as_str()
            );
        }
        assert_eq!(CODE_NAV_ACTIONS.matches(" / ").count(), 3, "{CODE_NAV_ACTIONS}");
    }
}
