//! HTTP JSON 语义上限（纯函数；不含 axum 状态码包装）。

use serde_json::Value;

use super::chat::ChatRequestBody;
use super::workspace::{WorkspaceFileWriteBody, WorkspaceSearchBody};

/// `clarify_questionnaire_answers.answers` JSON 预算（防畸形嵌套占内存）。
const CLARIFY_ANSWERS_JSON_MAX_DEPTH: usize = 24;
const CLARIFY_ANSWERS_JSON_MAX_NODES: usize = 8192;

/// `encoding` 查询参数字节上限。
pub const WORKSPACE_QUERY_ENCODING_MAX_BYTES: usize = 64;

/// 单条用户 `message` 字符串的字节上限（UTF-8）。
pub const CHAT_USER_MESSAGE_MAX_BYTES: usize = 16 * 1024 * 1024;
/// 澄清问卷 `questionnaire_id` 字节上限。
pub const CLARIFY_QUESTIONNAIRE_ID_MAX_BYTES: usize = 512;
/// 工作区搜索正则/关键词字节上限。
pub const WORKSPACE_SEARCH_PATTERN_MAX_BYTES: usize = 8192;
/// `WorkspaceSearchBody::max_results` 上限。
pub const WORKSPACE_SEARCH_MAX_RESULTS_CAP: usize = 5000;
/// Web 工作区文件写入正文上限。
pub const WORKSPACE_FILE_WRITE_MAX_BYTES: usize = 16 * 1024 * 1024;

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

pub fn validate_clarify_answers_json_budget(v: &Value) -> Result<(), String> {
    let mut nodes = 0usize;
    clarify_answers_walk(
        v,
        0,
        CLARIFY_ANSWERS_JSON_MAX_DEPTH,
        &mut nodes,
        CLARIFY_ANSWERS_JSON_MAX_NODES,
    )
}

pub fn validate_workspace_query_encoding_optional(raw: Option<&str>) -> Result<(), String> {
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

/// 返回面向 HTTP 的机器可读错误码与消息（由宿主映射为 StatusCode）。
pub fn chat_request_payload_limit_error(body: &ChatRequestBody) -> Option<(&'static str, String)> {
    if body.message.len() > CHAT_USER_MESSAGE_MAX_BYTES {
        return Some((
            "MESSAGE_TOO_LARGE",
            format!(
                "message 过长（上限 {} MiB）",
                CHAT_USER_MESSAGE_MAX_BYTES / (1024 * 1024)
            ),
        ));
    }
    if let Some(ref c) = body.clarify_questionnaire_answers {
        if c.questionnaire_id.len() > CLARIFY_QUESTIONNAIRE_ID_MAX_BYTES {
            return Some((
                "INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS",
                "questionnaire_id 过长".to_string(),
            ));
        }
        if let Err(msg) = validate_clarify_answers_json_budget(&c.answers) {
            return Some(("INVALID_CLARIFY_QUESTIONNAIRE_ANSWERS", msg));
        }
    }
    None
}

pub fn clamp_workspace_search_max_results(raw: Option<usize>) -> Option<usize> {
    raw.map(|n| n.clamp(1, WORKSPACE_SEARCH_MAX_RESULTS_CAP))
}

pub fn validate_workspace_search_pattern(pattern_trimmed: &str) -> Result<(), String> {
    if pattern_trimmed.len() > WORKSPACE_SEARCH_PATTERN_MAX_BYTES {
        return Err(format!(
            "pattern 过长（上限 {} 字节）",
            WORKSPACE_SEARCH_PATTERN_MAX_BYTES
        ));
    }
    Ok(())
}

pub fn workspace_search_pattern_or_error(body: &WorkspaceSearchBody) -> Result<&str, String> {
    let pattern = body.pattern.trim();
    if pattern.is_empty() {
        return Err("pattern 不能为空".to_string());
    }
    validate_workspace_search_pattern(pattern)?;
    Ok(pattern)
}

pub fn validate_workspace_file_write_request(body: &WorkspaceFileWriteBody) -> Result<(), String> {
    if body.create_directory {
        if body.create_only || body.update_only {
            return Err("create_directory 不能与 create_only 或 update_only 同时使用".to_string());
        }
        if !body.content.is_empty() {
            return Err("create_directory 时 content 须为空".to_string());
        }
        return Ok(());
    }
    validate_workspace_file_write_payload(body.content.as_bytes())
}

pub fn validate_workspace_file_write_payload(content: &[u8]) -> Result<(), String> {
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
    use serde_json::json;

    #[test]
    fn clarify_answers_budget_rejects_deep_nesting() {
        let mut v = json!(0);
        for _ in 0..30 {
            v = json!([v]);
        }
        assert!(validate_clarify_answers_json_budget(&v).is_err());
    }
}
