//! L2 语义意图分类器（可灰度）。
//!
//! 当前实现为额外一次无工具 LLM 调用，输出结构化 JSON。
//! 调用失败或解析失败时返回 `None`，由上层走 L1 fail-open。

use crate::agent::intent_pipeline::L2IntentCandidate;
use crate::agent::intent_router::IntentKind;
use crate::config::AgentConfig;
use crate::llm::backend::ChatCompletionsBackend;
use crate::llm::{CompleteChatRetryingParams, LlmRetryingTransportOpts, complete_chat_retrying};
use crate::types::{Message, message_content_as_str};

/// 尝试执行 L2 语义分类；失败返回 `None`（fail-open）。
pub async fn classify_intent_l2_with_llm(
    task: &str,
    cfg: &AgentConfig,
    llm_backend: &dyn ChatCompletionsBackend,
    client: &reqwest::Client,
    api_key: &str,
) -> Option<L2IntentCandidate> {
    let prompt = build_l2_prompt(task);
    let request = crate::types::ChatRequest {
        model: cfg.model.clone(),
        messages: vec![Message::user_only(&prompt)],
        stream: Some(false),
        temperature: 0.0,
        max_tokens: cfg.intent_l2_max_tokens,
        tools: None,
        tool_choice: None,
        seed: None,
        reasoning_split: Some(false),
        thinking: None,
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
    let (resp, _) = complete_chat_retrying(&params, &request).await.ok()?;
    let content = message_content_as_str(&resp.content)?.trim();
    parse_l2_response_json(content)
}

fn build_l2_prompt(task: &str) -> String {
    format!(
        r#"你是一个意图分类器。请仅输出 JSON，不要输出解释文字。

任务：
{task}

你必须从以下 kind 选择一个：greeting, qa, execute, ambiguous
primary_intent 建议值：
- meta.greeting
- qa.explain
- execute.read_inspect
- execute.code_change
- execute.debug_diagnose
- execute.run_test_build
- execute.docs_ops
- execute.git_ops
- unknown

输出 JSON 结构：
{{
  "kind": "greeting|qa|execute|ambiguous",
  "primary_intent": "string",
  "secondary_intents": ["string"],
  "confidence": 0.0,
  "need_clarification": false,
  "abstain": false
}}
"#
    )
}

fn parse_l2_response_json(raw: &str) -> Option<L2IntentCandidate> {
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
    let parsed: RawL2 = serde_json::from_str(raw).ok()?;
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
    use super::parse_l2_response_json;
    use crate::agent::intent_router::IntentKind;

    #[test]
    fn parse_valid_json() {
        let raw = r#"{"kind":"execute","primary_intent":"execute.read_inspect","secondary_intents":[],"confidence":0.86,"need_clarification":false,"abstain":false}"#;
        let x = parse_l2_response_json(raw).expect("parse");
        assert_eq!(x.kind, IntentKind::Execute);
        assert_eq!(x.primary_intent, "execute.read_inspect");
    }
}
