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
///
/// - `merged_routing_text`：L0 续接合并后的**路由**文本（与 L1 一致）
/// - `current_user_line`：当前轮用户原句，供模型区分指代
pub async fn classify_intent_l2_with_llm(
    merged_routing_text: &str,
    current_user_line: &str,
    cfg: &AgentConfig,
    llm_backend: &dyn ChatCompletionsBackend,
    client: &reqwest::Client,
    api_key: &str,
) -> Option<L2IntentCandidate> {
    let prompt = build_l2_prompt(merged_routing_text, current_user_line);
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

fn build_l2_prompt(merged_routing_text: &str, current_user_line: &str) -> String {
    format!(
        r#"你是 CrabMate 的意图分类器。只输出**一段** JSON 对象，不要解释；若必须包在代码块，请用 ```json ... ``` 包裹该 JSON。

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
- meta.greeting, qa.explain, execute.read_inspect, execute.code_change, execute.debug_diagnose, execute.run_test_build, execute.docs_ops, execute.git_ops, unknown

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
        let after = after.strip_prefix("json").unwrap_or(after);
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
    use super::parse_l2_response_json;
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
}
