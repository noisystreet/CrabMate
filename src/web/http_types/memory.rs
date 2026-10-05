//! `GET /memory/list` / `DELETE /memory/{id}` JSON 契约（仅 serde 类型，无 handler）。
//!
//! 契约真源见 `docs/design/memory_management_api.md` §3；路由表见 [`crate::web::routes::memory`]。

use serde::{Deserialize, Serialize};

/// `GET /memory/list` 查询参数。
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MemoryListQuery {
    pub(crate) conversation_id: Option<String>,
    pub(crate) limit: Option<usize>,
    pub(crate) offset: Option<usize>,
    pub(crate) kind: Option<String>,
    pub(crate) tag: Option<String>,
    pub(crate) q: Option<String>,
    pub(crate) sort: Option<String>,
}

/// `DELETE /memory/{id}` 查询参数（`conversation_id` 为必填作用域守卫）。
#[derive(Debug, Clone, Default, Deserialize)]
pub(crate) struct MemoryDeleteQuery {
    pub(crate) conversation_id: Option<String>,
}

/// 单条记忆投影：**不含** `embedding`，也不含内部 `source_role` / `source_kind`。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct MemoryEntryView {
    pub(crate) id: i64,
    pub(crate) text: String,
    /// 对外稳定枚举：`turn` / `explicit` / `experience` / `other`。
    pub(crate) kind: &'static str,
    pub(crate) tags: Vec<String>,
    pub(crate) created_at_unix: i64,
    /// `null` = 不过期。
    pub(crate) expires_at_unix: Option<i64>,
}

/// `GET /memory/list` 响应。
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct MemoryListResponse {
    pub(crate) conversation_id: String,
    pub(crate) entries: Vec<MemoryEntryView>,
    /// 过滤后（不含分页）总数。
    pub(crate) total: i64,
    pub(crate) limit: usize,
    pub(crate) offset: usize,
    pub(crate) has_more: bool,
}

/// 记忆路由错误体：在 [`ApiError`](crate::cm_web_host::http_types::api::ApiError) 形态上可选带
/// `enabled`——禁用态 503 用于让 UI 直接展示原因；400 时该字段被省略。
#[derive(Debug, Clone, Serialize)]
pub(crate) struct MemoryErrorBody {
    pub(crate) code: &'static str,
    pub(crate) message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) enabled: Option<bool>,
}

impl MemoryErrorBody {
    pub(crate) fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            enabled: None,
        }
    }

    /// `503 LONG_TERM_MEMORY_DISABLED`（`enabled: false`）。
    pub(crate) fn disabled() -> Self {
        Self {
            code: crate::cm_api_contract::error_codes::LONG_TERM_MEMORY_DISABLED,
            message: "长期记忆未启用".to_string(),
            enabled: Some(false),
        }
    }
}

/// 解析 `tags_json`；非法或含非字符串项一律按 `[]` / 丢弃处理（与既有 `unwrap_or("[]")` 一致）。
pub(crate) fn parse_tags_json(raw: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::Array(items)) => items
            .into_iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_json_handles_valid_and_invalid() {
        assert_eq!(parse_tags_json(r#"["a","b"]"#), vec!["a", "b"]);
        assert!(parse_tags_json("not json").is_empty());
        assert!(parse_tags_json(r#"{"a":1}"#).is_empty());
        // 数组内非字符串项被丢弃。
        assert_eq!(parse_tags_json(r#"["a",1,null]"#), vec!["a"]);
    }

    #[test]
    fn disabled_body_serializes_enabled_false() {
        let v = serde_json::to_value(MemoryErrorBody::disabled()).unwrap();
        assert_eq!(v["code"], "LONG_TERM_MEMORY_DISABLED");
        assert_eq!(v["enabled"], false);
    }

    #[test]
    fn bad_request_body_omits_enabled() {
        let v = serde_json::to_value(MemoryErrorBody::new("INVALID_MEMORY_ID", "x")).unwrap();
        assert!(v.get("enabled").is_none());
    }
}
