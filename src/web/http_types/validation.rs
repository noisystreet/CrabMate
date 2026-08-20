//! HTTP JSON 语义上限（字段长度、条数），与传输层请求体大小限制配合。
//!
//! 纯校验在 **`crate::cm_web_host::http_types::limits`**；本模块再导出 handler 所需符号，并提供 axum
//! [`StatusCode`] / [`Json`] 包装（如 [`validate_chat_request_payload_limits`]）。

use axum::Json;
use axum::http::StatusCode;

use super::chat::{ApiError, ChatRequestBody};

pub(crate) use crate::cm_web_host::http_types::limits::{
    clamp_workspace_search_max_results, validate_workspace_file_write_payload,
    validate_workspace_file_write_request, validate_workspace_query_encoding_optional,
    workspace_search_pattern_or_error,
};

pub(crate) fn validate_chat_request_payload_limits(
    body: &ChatRequestBody,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    match crate::cm_web_host::http_types::limits::chat_request_payload_limit_error(body) {
        None => Ok(()),
        Some((code, message)) => Err((StatusCode::BAD_REQUEST, Json(ApiError::new(code, message)))),
    }
}

#[cfg(test)]
mod workspace_file_write_directory_tests {
    use super::super::workspace::WorkspaceFileWriteBody;
    use super::validate_workspace_file_write_request;

    #[test]
    fn create_directory_allows_empty_content() {
        let body = WorkspaceFileWriteBody {
            path: "dir".into(),
            content: String::new(),
            create_only: false,
            update_only: false,
            create_directory: true,
            parents: true,
        };
        validate_workspace_file_write_request(&body).expect("valid");
    }

    #[test]
    fn create_directory_rejects_non_empty_content() {
        let body = WorkspaceFileWriteBody {
            path: "dir".into(),
            content: "x".into(),
            create_only: false,
            update_only: false,
            create_directory: true,
            parents: false,
        };
        let err = validate_workspace_file_write_request(&body).expect_err("content");
        assert!(err.contains("content"), "{err}");
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::super::chat::ChatRequestBody;
    use super::super::workspace::WorkspaceSearchBody;
    use super::{
        clamp_workspace_search_max_results, validate_workspace_query_encoding_optional,
        workspace_search_pattern_or_error,
    };
    use crate::cm_web_host::http_types::chat_keys::{
        CHAT_REQUEST_BODY_ALLOWED_KEYS, reject_unknown_chat_body_keys,
    };
    use crate::cm_web_host::http_types::limits::{
        WORKSPACE_QUERY_ENCODING_MAX_BYTES, WORKSPACE_SEARCH_MAX_RESULTS_CAP,
        WORKSPACE_SEARCH_PATTERN_MAX_BYTES, validate_clarify_answers_json_budget,
        validate_workspace_search_pattern,
    };

    #[test]
    fn chat_request_body_allowed_keys_stay_sorted_for_binary_search() {
        let keys = CHAT_REQUEST_BODY_ALLOWED_KEYS;
        for w in keys.windows(2) {
            assert!(
                w[0] < w[1],
                "CHAT_REQUEST_BODY_ALLOWED_KEYS must be sorted: {:?} >= {:?}",
                w[0],
                w[1]
            );
        }
    }

    #[test]
    fn chat_request_body_deserializes_known_shape() {
        let raw = r#"{"message":"hi","conversation_id":"c1"}"#;
        let body: ChatRequestBody = serde_json::from_str(raw).expect("ok");
        assert_eq!(body.message, "hi");
        assert_eq!(body.conversation_id.as_deref(), Some("c1"));
    }

    #[test]
    fn reject_unknown_chat_body_keys_errors_on_extra() {
        let mut m = Map::new();
        m.insert("message".into(), Value::String("x".into()));
        m.insert("nope".into(), Value::Null);
        assert!(reject_unknown_chat_body_keys(&m).is_err());
    }

    #[test]
    fn clarify_answers_budget_ok_for_small_object() {
        validate_clarify_answers_json_budget(&json!({"a":1})).expect("ok");
    }

    #[test]
    fn workspace_encoding_rejects_oversized() {
        let s = "x".repeat(WORKSPACE_QUERY_ENCODING_MAX_BYTES + 1);
        assert!(validate_workspace_query_encoding_optional(Some(&s)).is_err());
    }

    #[test]
    fn workspace_search_pattern_helpers() {
        let body = WorkspaceSearchBody {
            pattern: "  foo  ".into(),
            path: None,
            max_results: Some(9_999),
            case_insensitive: None,
            ignore_hidden: None,
        };
        assert_eq!(workspace_search_pattern_or_error(&body).unwrap(), "foo");
        assert_eq!(
            clamp_workspace_search_max_results(body.max_results),
            Some(WORKSPACE_SEARCH_MAX_RESULTS_CAP)
        );
        let long = "a".repeat(WORKSPACE_SEARCH_PATTERN_MAX_BYTES + 1);
        assert!(validate_workspace_search_pattern(&long).is_err());
    }
}
