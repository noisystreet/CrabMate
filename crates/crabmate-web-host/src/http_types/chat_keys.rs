//! `POST /chat*` 顶层 JSON 键白名单与未知键拒绝。

use serde_json::Value;

/// `POST /chat*`、流式请求 JSON 顶层允许的键（字母序，供二分查找）。
pub const CHAT_REQUEST_BODY_ALLOWED_KEYS: &[&str] = &[
    "agent_role",
    "approval_session_id",
    "clarify_questionnaire_answers",
    "client_llm",
    "client_sse_protocol",
    "conversation_id",
    "executor_llm",
    "image_urls",
    "message",
    "readonly_tool_ttl_cache_secs",
    "seed",
    "seed_policy",
    "session_mode",
    "stream_resume",
    "temperature",
];

/// `POST /chat/async` 除对话字段外允许的顶层键。
pub const CHAT_ASYNC_EXTRA_KEYS: &[&str] = &["webhook_secret", "webhook_url"];

pub fn reject_unknown_chat_body_keys(obj: &serde_json::Map<String, Value>) -> Result<(), String> {
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

pub fn reject_unknown_async_chat_body_keys(
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
