//! L2 语义意图分类器。
//!
//! 当前实现为额外一次无工具 LLM 调用，输出结构化 JSON。
//! 调用失败或解析失败时返回脱敏原因，由上层走弃用规则层兜底。

use crate::agent::intent_pipeline::L2IntentCandidate;
use crate::agent::intent_router::IntentKind;
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
    );
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
        r#"你是 CrabMate 的意图分类器。只输出**一段** JSON 对象，不要解释、不要推理过程；若必须包在代码块，请用 ```json ... ``` 包裹该 JSON。

【合并后的路由文本】（可能含前序+续接；用于消歧义）
{merged}

【当前轮用户原句】
{current}

分类规则要点：
- kind=greeting：纯寒暄/感谢，无任务。
- kind=qa：以解释/概念/比较为主，不要求改仓库或跑命令；含「不要执行/只解释」时倾向 qa。
- kind=execute：要改代码、跑测试/构建、查目录与文件、git 提交/PR、修报错等可执行工作。
- kind=ambiguous：信息不足且无法合理猜测执行目标。

primary_intent 从下列选用最贴切的一项（可接近则选 execute 子类）：
- meta.greeting
- qa.explain：概念/报错含义/用法解释，不要求动仓库
- qa.meta：自我介绍、你能做什么、技能与能力范围、会不会/能否使用某语言或技术（如「你会 C++ 吗」）；需要细分时可加后缀如 qa.meta.capability，与 qa.meta 等价归并即可
- qa.readonly / qa.codebase：需要结合仓库**只读**查看文件/目录后回答，不修改代码、不跑构建测试
- execute.read_inspect, execute.code_change, execute.debug_diagnose, execute.run_test_build, execute.docs_ops, execute.git_ops, unknown

secondary_intents：同句中其它显著意图，可空。
confidence：0.0-1.0。need_clarification：是否缺关键信息；abstain：是否应拒识为执行（与 ambiguous 常同向）。

严格 JSON 键名与类型（勿加注释）：
{{
  "kind": "greeting|qa|execute|ambiguous",
  "primary_intent": "string",
  "secondary_intents": ["string"],
  "confidence": 0.0,
  "need_clarification": false,
  "abstain": false
}}
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
        kind: String,
        primary_intent: String,
        #[serde(default)]
        secondary_intents: Vec<String>,
        confidence: f32,
        #[serde(default)]
        need_clarification: bool,
        #[serde(default)]
        abstain: bool,
    }
    let parsed: RawL2 = serde_json::from_str(json_str).ok()?;
    let kind = match parsed.kind.as_str() {
        "greeting" => IntentKind::Greeting,
        "qa" => IntentKind::Qa,
        "execute" => IntentKind::Execute,
        "ambiguous" => IntentKind::Ambiguous,
        _ => return None,
    };
    Some(L2IntentCandidate {
        kind,
        primary_intent: parsed.primary_intent.trim().to_string(),
        secondary_intents: parsed
            .secondary_intents
            .into_iter()
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        confidence: parsed.confidence.clamp(0.0, 1.0),
        need_clarification: parsed.need_clarification,
        abstain: parsed.abstain,
    })
}

#[cfg(test)]
mod tests {
    use super::{format_l2_http_status, format_l2_transport_error_text, parse_l2_response_json};
    use crate::agent::intent_router::IntentKind;

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"kind":"execute","primary_intent":"execute.read_inspect","secondary_intents":[],"confidence":0.86,"need_clarification":false,"abstain":false}"#;
        let x = parse_l2_response_json(raw).expect("parse");
        assert_eq!(x.kind, IntentKind::Execute);
        assert_eq!(x.primary_intent, "execute.read_inspect");
    }

    #[test]
    fn parse_fenced_json_block() {
        let raw = "```json\n{\"kind\":\"qa\",\"primary_intent\":\"qa.explain\",\"secondary_intents\":[],\"confidence\":0.9,\"need_clarification\":false,\"abstain\":false}\n```";
        let x = parse_l2_response_json(raw).expect("parse");
        assert_eq!(x.kind, IntentKind::Qa);
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
