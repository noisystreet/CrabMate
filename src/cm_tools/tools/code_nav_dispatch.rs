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

use crate::cm_tools::tools::tool_param_types::{CodeNavAction, CodeNavArgs};
use crate::cm_tools::tools::{
    call_graph_sketch, code_nav, parse_args_json, parse_args_typed, symbol,
};

/// 缺 / 未知 `action` 的报错里列出的可选值。
pub const CODE_NAV_ACTIONS: &str = "find_symbol / find_references / file_outline / call_graph_sketch";

/// `code_nav` 唯一入口：按 `action` 分派到四个只读实现。
pub fn dispatch(args_json: &str, working_dir: &Path) -> String {
    // 先宽松取一次 action：这样「完全没有 action」能给出可选值清单，
    // 而不是 serde 那句只有字段名的 missing field。
    let raw = match parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let has_action = raw
        .get("action")
        .and_then(|x| x.as_str())
        .is_some_and(|s| !s.trim().is_empty());
    if !has_action {
        return format!("错误：缺少 action（可选值：{CODE_NAV_ACTIONS}）");
    }
    // 上面的探测只保证 `action` 是非空字符串；非法取值交给 `CodeNavAction` 反序列化报错（serde 会列出全部可选值）。
    let args: CodeNavArgs = match parse_args_typed(args_json) {
        Ok(a) => a,
        Err(e) => return e,
    };
    match args.action {
        CodeNavAction::FindSymbol => match non_empty(args.symbol.as_deref()) {
            Some(sym) => symbol::run(
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
            None => missing_field(args.action, "symbol"),
        },
        CodeNavAction::FindReferences => match non_empty(args.symbol.as_deref()) {
            Some(sym) => code_nav::find_references(
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
            None => missing_field(args.action, "symbol"),
        },
        CodeNavAction::FileOutline => match non_empty(args.path.as_deref()) {
            Some(path) => code_nav::rust_file_outline(
                &json!({
                    "path": path,
                    "include_use": args.include_use,
                    "max_items": args.max_items,
                })
                .to_string(),
                working_dir,
            ),
            None => missing_field(args.action, "path"),
        },
        CodeNavAction::CallGraphSketch => {
            let has_symbol = non_empty(args.symbol.as_deref()).is_some();
            let has_symbols = args.symbols.iter().any(|s| !s.trim().is_empty());
            if !has_symbol && !has_symbols {
                return "错误：`call_graph_sketch` 需要 `symbol` 或 `symbols`（至少一个非空）。"
                    .to_string();
            }
            call_graph_sketch::run(
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

fn missing_field(action: CodeNavAction, field: &str) -> String {
    let name = action.as_str();
    format!("错误：`{name}` 需要非空的 `{field}`（可选 action：{CODE_NAV_ACTIONS}）。")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(args: &str) -> String {
        dispatch(args, Path::new("."))
    }

    #[test]
    fn missing_action_lists_every_value() {
        let out = run("{}");
        assert!(out.contains("缺少 action"), "{out}");
        for name in [
            "find_symbol",
            "find_references",
            "file_outline",
            "call_graph_sketch",
        ] {
            assert!(out.contains(name), "缺少可选值 `{name}`: {out}");
        }
    }

    #[test]
    fn unknown_action_reports_expected_values() {
        let out = run(r#"{"action":"nope"}"#);
        assert!(out.contains("参数解析错误"), "{out}");
        // serde 的 unknown variant 文案会带 expected 列表，模型据此可自我纠正。
        assert!(out.contains("find_symbol"), "{out}");
    }

    #[test]
    fn per_action_required_fields_are_enforced() {
        assert!(run(r#"{"action":"find_symbol"}"#).contains("需要非空的 `symbol`"));
        assert!(run(r#"{"action":"find_references"}"#).contains("需要非空的 `symbol`"));
        assert!(run(r#"{"action":"file_outline"}"#).contains("需要非空的 `path`"));
        assert!(run(r#"{"action":"call_graph_sketch"}"#).contains("`symbol` 或 `symbols`"));
        // 全空白等同缺省：trim 后为空即拒，避免拿空白去扫整个工作区。
        assert!(run(r#"{"action":"find_symbol","symbol":"   "}"#).contains("需要非空的 `symbol`"));
        assert!(
            run(r#"{"action":"call_graph_sketch","symbols":["","  "]}"#).contains("`symbol` 或 `symbols`")
        );
    }

    #[test]
    fn fields_of_other_actions_are_ignored() {
        // 刻意语义：只挑本 action 读的字段，其余（`symbol` / `max_edges`）不报错。
        // 用 `Cargo.toml` 触发实现层的扩展名检查，避免真的扫工作区。
        let out = run(r#"{"action":"file_outline","path":"Cargo.toml","symbol":"x","max_edges":5}"#);
        assert!(out.contains("仅支持 .rs"), "{out}");
    }

    #[test]
    fn unknown_keys_still_fail() {
        // 「忽略无关字段」不等于放任拼错的键：`deny_unknown_fields` 仍生效。
        let out = run(r#"{"action":"find_symbol","symbol":"x","synbol":"y"}"#);
        assert!(out.contains("参数解析错误"), "{out}");
        assert!(out.contains("synbol"), "{out}");
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
