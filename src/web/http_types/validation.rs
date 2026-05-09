//! HTTP JSON 语义上限（字段长度、条数），与传输层请求体大小限制配合。
//!
//! [`super::chat::ChatRequestBody`] / [`super::chat::ChatAsyncRequestBody`] 顶层键白名单见
//! [`CHAT_REQUEST_BODY_ALLOWED_KEYS`]（自定义 `Deserialize`）；嵌套对象仍由对应结构的
//! `deny_unknown_fields` 拦截。

use axum::Json;
use axum::http::StatusCode;
use serde_json::Value;

use super::chat::{ApiError, ChatRequestBody};
use super::workspace::{WorkspaceFileWriteBody, WorkspaceSearchBody};

/// `POST /chat*`、流式请求 JSON 顶层允许的键（字母序，供二分查找）。
pub(crate) const CHAT_REQUEST_BODY_ALLOWED_KEYS: &[&str] = &[
    "agent_role",
    "approval_session_id",
    "client_llm",
    "client_sse_protocol",
    "clarify_questionnaire_answers",
    "conversation_id",
    "execution_mode",
    "executor_llm",
    "image_urls",
    "message",
    "readonly_tool_ttl_cache_secs",
    "seed",
    "seed_policy",
    "stream_resume",
    "temperature",
];

/// `POST /chat/async` 除对话字段外允许的顶层键。
pub(crate) const CHAT_ASYNC_EXTRA_KEYS: &[&str] = &["webhook_secret", "webhook_url"];

/// `clarify_questionnaire_answers.answers` JSON 预算（防畸形嵌套占内存）。
const CLARIFY_ANSWERS_JSON_MAX_DEPTH: usize = 24;
const CLARIFY_ANSWERS_JSON_MAX_NODES: usize = 8192;

/// `encoding` 查询参数字节上限。
pub(crate) const WORKSPACE_QUERY_ENCODING_MAX_BYTES: usize = 64;

pub(crate) fn reject_unknown_chat_body_keys(
    obj: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    for k in obj.keys() {
        if CHAT_REQUEST_BODY_ALLOWED_KEYS
            .binary_search(&k.as_str())
            .is_err()
        {
            return Err(format!("未知的请求字段: {k}"));
        }
    }
    Ok(())
}

pub(crate) fn reject_unknown_async_chat_body_keys(
    obj: &serde_json::Map<String, Value>,
) -> Result<(), String> {
    for k in obj.keys() {
        if CHAT_REQUEST_BODY_ALLOWED_KEYS
            .binary_search(&k.as_str())
            .is_ok()
        {
            continue;
        }
        if CHAT_ASYNC_EXTRA_KEYS.binary_search(&k.as_str()).is_ok() {
            continue;
        }
        return Err(format!("未知的请求字段: {k}"));
    }
    Ok(())
}

fn clarify_answers_walk(
    v: &Value,
    depth: usize,
    max_depth: usize,
    nodes: &mut usize,
    max_nodes: usize,
) -> Result<(), String> {
    if depth > max_depth {
        return Err(format!(
            "clarify_questionnaire_answers.answers 嵌套过深（上限 {max_depth}）"
        ));
    }
    *nodes += 1;
    if *nodes > max_nodes {
        return Err(format!(
            "clarify_questionnaire_answers.answers 过大（节点上限 {max_nodes}）"
        ));
    }
    match v {
        Value::Array(a) => {
            for x in a {
                clarify_answers_walk(x, depth + 1, max_depth, nodes, max_nodes)?;
            }
        }
        Value::Object(o) => {
            for (_, x) in o {
                clarify_answers_walk(x, depth + 1, max_depth, nodes, max_nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

pub(crate) fn validate_clarify_answers_json_budget(v: &Value) -> Result<(), String> {
    let mut nodes = 0usize;
    clarify_answers_walk(
        v,
        0,
        CLARIFY_ANSWERS_JSON_MAX_DEPTH,
        &mut nodes,
        CLARIFY_ANSWERS_JSON_MAX_NODES,
    )
}

pub(crate) fn validate_workspace_query_encoding_optional(raw: Option<&str>) -> Result<(), String> {
    let Some(s) = raw else {
        return Ok(());
    };
    if s.len() > WORKSPACE_QUERY_ENCODING_MAX_BYTES {
        return Err(format!(
            "encoding 过长（上限 {} 字节）",
            WORKSPACE_QUERY_ENCODING_MAX_BYTES
        ));
    }
    Ok(())
}

/// 单条用户 `message` 字符串的字节上限（UTF-8）。
pub(crate) const CHAT_USER_MESSAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
/// 澄清问卷 `questionnaire_id` 字节上限。
pub(crate) const CLARIFY_QUESTIONNAIRE_ID_MAX_BYTES: usize = 512;
/// 工作区搜索正则/关键词字节上限。
pub(crate) const WORKSPACE_SEARCH_PATTERN_MAX_BYTES: usize = 8192;
/// `WorkspaceSearchBody::max_results` 上限（与工具侧合理默认一致）。
pub(crate) const WORKSPACE_SEARCH_MAX_RESULTS_CAP: usize = 5000;
/// Web 工作区文件写入正文上限（与单次 JSON 体上限分层防御）。
pub(crate) const WORKSPACE_FILE_WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn validate_chat_request_payload_limits(
    body: &ChatRequestBody,
) -> Result<(), (StatusCode, Json<ApiError>)> {
    if body.message.len() > CHAT_USER_MESSAGE_MAX_BYTES {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "MESSAGE_TOO_LARGE",
                format!(
                    "message 过长（上限 {} MiB）",
                    CHAT_USER_MESSAGE_MAX_BYTES / (1024 * 1024)
                ),
            )),
        ));
    }
    if let Some(ref c) = body.clarify_questionnaire_answers {
        if c.questionnaire_id.len() > CLARIFY_QUESTIONNAIRE_ID_MAX_BYTES {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError::new(
                    "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                    "questionnaire_id 过长".to_string(),
                )),
            ));
        }
        validate_clarify_answers_json_budget(&c.answers).map_err(|msg| {
            (
                StatusCode::BAD_REQUEST,
                Json(ApiError::new("INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS", msg)),
            )
        })?;
    }
    Ok(())
}

pub(crate) fn clamp_workspace_search_max_results(raw: Option<usize>) -> Option<usize> {
    raw.map(|n| n.clamp(1, WORKSPACE_SEARCH_MAX_RESULTS_CAP))
}

pub(crate) fn validate_workspace_search_pattern(pattern_trimmed: &str) -> Result<(), String> {
    if pattern_trimmed.len() > WORKSPACE_SEARCH_PATTERN_MAX_BYTES {
        return Err(format!(
            "pattern 过长（上限 {} 字节）",
            WORKSPACE_SEARCH_PATTERN_MAX_BYTES
        ));
    }
    Ok(())
}

/// 工作区搜索：`trim`、非空与长度上限。
pub(crate) fn workspace_search_pattern_or_error(
    body: &WorkspaceSearchBody,
) -> Result<&str, String> {
    let pattern = body.pattern.trim();
    if pattern.is_empty() {
        return Err("pattern 不能为空".to_string());
    }
    validate_workspace_search_pattern(pattern)?;
    Ok(pattern)
}

/// Web 写入接口：正文大小上限（路径与其它规则由 handler 校验）。
pub(crate) fn validate_workspace_file_write_request(
    body: &WorkspaceFileWriteBody,
) -> Result<(), String> {
    validate_workspace_file_write_payload(body.content.as_bytes())
}

pub(crate) fn validate_workspace_file_write_payload(content: &[u8]) -> Result<(), String> {
    if content.len() > WORKSPACE_FILE_WRITE_MAX_BYTES {
        return Err(format!(
            "content 过大（上限 {} MiB）",
            WORKSPACE_FILE_WRITE_MAX_BYTES / (1024 * 1024)
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use serde_json::{Map, Value, json};

    use super::super::chat::ChatRequestBody;
    use super::super::workspace::WorkspaceSearchBody;
    use super::{
        WORKSPACE_SEARCH_MAX_RESULTS_CAP, WORKSPACE_SEARCH_PATTERN_MAX_BYTES,
        clamp_workspace_search_max_results, reject_unknown_chat_body_keys,
        validate_clarify_answers_json_budget, validate_workspace_query_encoding_optional,
        validate_workspace_search_pattern, workspace_search_pattern_or_error,
    };

    #[test]
    fn deserialize_chat_request_body_rejects_unknown_top_level_key() {
        let j = r#"{"message":"hi","not_a_valid_field":1}"#;
        assert!(serde_json::from_str::<ChatRequestBody>(j).is_err());
    }

    #[test]
    fn reject_unknown_chat_body_keys_errors_on_extra() {
        let mut m = Map::new();
        m.insert("message".into(), json!("x"));
        m.insert("typo_field".into(), Value::Null);
        assert!(reject_unknown_chat_body_keys(&m).is_err());
    }

    #[test]
    fn clarify_answers_budget_rejects_deep_nesting() {
        let mut inner = json!(true);
        for _ in 0..40 {
            inner = json!([inner]);
        }
        assert!(validate_clarify_answers_json_budget(&inner).is_err());
    }

    #[test]
    fn workspace_query_encoding_optional_rejects_long() {
        let s = "x".repeat(super::WORKSPACE_QUERY_ENCODING_MAX_BYTES + 1);
        assert!(validate_workspace_query_encoding_optional(Some(&s)).is_err());
    }

    #[test]
    fn clamp_workspace_search_max_results_bounds() {
        assert_eq!(clamp_workspace_search_max_results(None), None);
        assert_eq!(clamp_workspace_search_max_results(Some(0)), Some(1));
        assert_eq!(clamp_workspace_search_max_results(Some(1)), Some(1));
        assert_eq!(
            clamp_workspace_search_max_results(Some(WORKSPACE_SEARCH_MAX_RESULTS_CAP)),
            Some(WORKSPACE_SEARCH_MAX_RESULTS_CAP)
        );
        assert_eq!(
            clamp_workspace_search_max_results(Some(WORKSPACE_SEARCH_MAX_RESULTS_CAP + 9)),
            Some(WORKSPACE_SEARCH_MAX_RESULTS_CAP)
        );
    }

    #[test]
    fn validate_workspace_search_pattern_rejects_oversized() {
        let p = "x".repeat(WORKSPACE_SEARCH_PATTERN_MAX_BYTES + 1);
        assert!(validate_workspace_search_pattern(&p).is_err());
    }

    #[test]
    fn workspace_search_pattern_or_error_empty() {
        let body = WorkspaceSearchBody {
            pattern: "   ".to_string(),
            path: None,
            max_results: None,
            case_insensitive: None,
            ignore_hidden: None,
        };
        assert!(workspace_search_pattern_or_error(&body).is_err());
    }
}
