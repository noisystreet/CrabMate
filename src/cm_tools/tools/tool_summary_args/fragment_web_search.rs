// ── web_search ───────────────────────────────────────────────

/// 旧的 `WebSearchSummaryArgs` 里 `query: String` 是**必填**（没有 `#[serde(default)]`）：
/// 缺键 / `null` / 类型不符 ⇒ 整条摘要消失；空白串能反序列化成功，但 trim 后为空 ⇒ 也不出摘要。
impl ToolArgsSummary for super::tool_param_types::WebSearchArgs {
    fn summary_line(args: &serde_json::Value) -> Option<String> {
        let q = arg_str_raw(args, "query")?.trim();
        (!q.is_empty()).then(|| format!("web search: {q}"))
    }
}

#[cfg(test)]
mod web_search_summary_tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::WebSearchArgs;
    use serde_json::json;

    #[test]
    fn query_is_trimmed() {
        assert_eq!(
            WebSearchArgs::summary_line(&json!({"query": "  rust async  ", "max_results": 5}))
                .as_deref(),
            Some("web search: rust async")
        );
    }

    /// 缺键 / `null` / 类型不符（旧结构体整体反序列化失败）与空白串都无摘要。
    #[test]
    fn missing_or_blank_query_has_no_line() {
        assert!(WebSearchArgs::summary_line(&json!({})).is_none());
        assert!(WebSearchArgs::summary_line(&json!({"query": null})).is_none());
        assert!(WebSearchArgs::summary_line(&json!({"query": 1})).is_none());
        assert!(WebSearchArgs::summary_line(&json!({"query": "   "})).is_none());
    }
}
