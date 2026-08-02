//! L2 语义意图分类器。
//!
//! 当前实现为额外一次无工具 LLM 调用，输出结构化 JSON。
//! 调用失败或解析失败时返回脱敏原因，由上层 **fail-open 进 Execute / 主模型**。

use crate::agent::intent_pipeline::{L2IntentCandidate, normalize_suggested_mode};
use crate::config::{AgentConfig, LlmHttpAuthMode};
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{
    CompleteChatRetryingParams, LlmCompleteError, LlmRetryingTransportOpts, complete_chat_retrying,
};
use crate::types::{Message, message_content_as_str};

/// 尝试执行 L2 语义分类；失败返回脱敏原因（由上层兜底）。
///
/// - `merged_routing_text`：续接合并后的**路由**文本（L2 主用；弃用 L1 兜底复用）
/// - `current_user_line`：当前轮用户原句，供模型区分指代
pub async fn classify_intent_l2_with_llm(
    merged_routing_text: &str,
    current_user_line: &str,
    cfg: &AgentConfig,
    llm_backend: &dyn ChatCompletionsBackend,
    client: &reqwest::Client,
    api_key: &str,
    turn_budget: Option<&std::sync::Arc<crate::agent::turn_budget::TurnBudgetCounter>>,
) -> Result<L2IntentCandidate, String> {
    if cfg.llm.llm_http_auth_mode == LlmHttpAuthMode::Bearer && api_key.trim().is_empty() {
        return Err("api_key_missing".to_string());
    }
    let prompt = build_l2_prompt(merged_routing_text, current_user_line);
    let request = crate::types::ChatRequest {
        core: crate::types::ChatRequestCore {
            model: cfg.llm.model.clone(),
            messages: vec![Message::user_only(&prompt)],
            tools: None,
            tool_choice: None,
            max_tokens: cfg.intent_routing.intent_l2_max_tokens,
            temperature: 0.0,
            seed: None,
            stream: Some(false),
        },
        vendor: crate::types::ChatRequestVendorExtensions {
            reasoning_split: None,
            thinking: None,
            reasoning_effort: None,
            response_format: None,
        },
    };
    let params = CompleteChatRetryingParams::new(
        llm_backend,
        client,
        api_key,
        cfg,
        LlmRetryingTransportOpts::headless_no_stream(),
        None,
        None,
    )
    .with_turn_budget(turn_budget);
    let (resp, _) = complete_chat_retrying(&params, &request)
        .await
        .map_err(format_l2_complete_error)?;
    let content = message_content_as_str(&resp.content)
        .ok_or_else(|| "empty_or_non_text_response".to_string())?
        .trim();
    if content.is_empty() {
        return Err("empty_response".to_string());
    }
    parse_l2_response_json(content).ok_or_else(|| {
        format!(
            "json_parse_failed: {}",
            preview_for_diagnostic(content, 120)
        )
    })
}

fn format_l2_complete_error(err: LlmCompleteError) -> String {
    match err {
        LlmCompleteError::Cancelled => "cancelled".to_string(),
        LlmCompleteError::Transport(e) => match e.http_status {
            Some(status) => format_l2_http_status(status),
            None => format_l2_transport_error_text(&e.user_message),
        },
        LlmCompleteError::Other(e) => {
            let msg = e.to_string();
            if msg.to_lowercase().contains("json") {
                "response_parse_error".to_string()
            } else {
                "llm_complete_error".to_string()
            }
        }
    }
}

fn format_l2_http_status(status: u16) -> String {
    match status {
        400 => "http_400_bad_request".to_string(),
        401 => "http_401_unauthorized".to_string(),
        403 => "http_403_forbidden".to_string(),
        404 => "http_404_not_found".to_string(),
        408 => "http_408_timeout".to_string(),
        429 => "http_429_rate_limited".to_string(),
        500..=599 => format!("http_{status}_server_error"),
        _ => format!("http_{status}"),
    }
}

fn format_l2_transport_error_text(err: &str) -> String {
    let lower = err.to_lowercase();
    if lower.contains("api_key")
        || lower.contains("authorization")
        || lower.contains("unauthorized")
    {
        return "auth_or_api_key_error".to_string();
    }
    if lower.contains("timeout") || lower.contains("timed out") {
        return "transport_timeout".to_string();
    }
    if lower.contains("dns") {
        return "transport_dns_error".to_string();
    }
    if lower.contains("tls") || lower.contains("certificate") || lower.contains("cert") {
        return "transport_tls_error".to_string();
    }
    if lower.contains("connect") || lower.contains("connection") || lower.contains("tcp") {
        return "transport_connect_error".to_string();
    }
    "transport_error".to_string()
}

fn preview_for_diagnostic(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for ch in s.chars().take(max_chars) {
        out.push(ch);
    }
    if s.chars().count() > max_chars {
        out.push_str("...");
    }
    out.replace('\n', "\\n")
}

fn build_l2_prompt(merged_routing_text: &str, current_user_line: &str) -> String {
    format!(
        r#"你是 CrabMate 的会话模式建议器。只输出**一段** JSON 对象，不要解释、不要推理过程；若必须包在代码块，请用 ```json ... ``` 包裹该 JSON。

【合并后的路由文本】（可能含前序+续接；用于消歧义）
{merged}

【当前轮用户原句】
{current}

根据用户本轮诉求，建议最合适的会话工作模式（与 agent_role 人格正交）：
- ask：只要解释/问答/只读理解，不改仓库、不跑构建测试。
- plan：可调研仓库并产出方案，仍不要写盘或跑破坏性命令。
- act：需要改代码、跑测试/构建、git 提交/PR 等可执行工作。

confidence：0.0–1.0，表示对该建议的把握。

严格 JSON 键名与类型（勿加注释）：
{{{{
  "suggested_mode": "ask|plan|act",
  "confidence": 0.0
}}}}
"#,
        merged = merged_routing_text,
        current = current_user_line,
    )
}

fn parse_l2_response_json(raw: &str) -> Option<L2IntentCandidate> {
    let raw = raw.trim();
    let json_block = raw.find("```").and_then(|start| {
        let after = &raw[start + 3..];
        let after = after
            .strip_prefix("json")
            .map(str::trim_start)
            .unwrap_or(after);
        after.find("```").map(|end| after[..end].trim())
    });
    let json_str = json_block.unwrap_or(raw);
    #[derive(serde::Deserialize)]
    struct RawL2 {
        suggested_mode: String,
        confidence: f32,
    }
    let parsed: RawL2 = serde_json::from_str(json_str).ok()?;
    let mode = normalize_suggested_mode(&parsed.suggested_mode)?.to_string();
    Some(L2IntentCandidate {
        suggested_mode: mode,
        confidence: parsed.confidence.clamp(0.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::{format_l2_http_status, format_l2_transport_error_text, parse_l2_response_json};

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"suggested_mode":"act","confidence":0.86}"#;
        let x = parse_l2_response_json(raw).expect("parse");
        assert_eq!(x.suggested_mode, "act");
        assert!((x.confidence - 0.86).abs() < f32::EPSILON);
    }

    #[test]
    fn parse_fenced_json_block() {
        let raw = "```json\n{\"suggested_mode\":\"ask\",\"confidence\":0.9}\n```";
        let x = parse_l2_response_json(raw).expect("parse");
        assert_eq!(x.suggested_mode, "ask");
    }

    #[test]
    fn parse_rejects_unknown_mode() {
        let raw = r#"{"suggested_mode":"fly","confidence":0.9}"#;
        assert!(parse_l2_response_json(raw).is_none());
    }

    #[test]
    fn l2_error_reason_classifies_common_failures() {
        assert_eq!(format_l2_http_status(401), "http_401_unauthorized");
        assert_eq!(format_l2_http_status(429), "http_429_rate_limited");
        assert_eq!(format_l2_http_status(503), "http_503_server_error");
        assert_eq!(
            format_l2_transport_error_text("error trying to connect: dns error"),
            "transport_dns_error"
        );
        assert_eq!(
            format_l2_transport_error_text("operation timed out"),
            "transport_timeout"
        );
        assert_eq!(
            format_l2_transport_error_text("connection refused"),
            "transport_connect_error"
        );
    }
}
