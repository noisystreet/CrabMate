//! HTTP JSON 语义上限（字段长度、条数），与传输层请求体大小限制配合。
//!
//! 根级 [`super::chat::ChatRequestBody`] 未使用 `deny_unknown_fields`：异步接口
//! [`super::chat::ChatAsyncRequestBody`] 使用 `flatten` 与同层 `webhook_*` 字段共存，
//! serde 无法在根对象上稳定拒绝未知键；未知嵌套键由子结构体的 `deny_unknown_fields` 拦截。

use axum::Json;
use axum::http::StatusCode;

use super::chat::{ApiError, ChatRequestBody};
use super::workspace::{WorkspaceFileWriteBody, WorkspaceSearchBody};

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
    if let Some(ref c) = body.clarify_questionnaire_answers
        && c.questionnaire_id.len() > CLARIFY_QUESTIONNAIRE_ID_MAX_BYTES
    {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError::new(
                "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                "questionnaire_id 过长".to_string(),
            )),
        ));
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
    use super::*;

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
