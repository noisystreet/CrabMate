//! PER 终答规划：可选的极短二次 LLM 校验（计划 vs 最近工具结果摘要），默认关闭。

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use tokio::sync::mpsc;

use crate::agent::plan_artifact;
use crate::config::AgentConfig;
use crate::llm::{
    ChatCompletionsBackend, CompleteChatRetryingParams, complete_chat_retrying,
    no_tools_chat_request,
};
use crate::types::{LlmSeedOverride, Message};

fn truncate_unicode(s: &str, max_chars: usize) -> String {
    let n = s.chars().count();
    if n <= max_chars {
        return s.to_string();
    }
    let mut out = s.chars().take(max_chars).collect::<String>();
    out.push_str("…(truncated)");
    out
}

const SIDE_SYSTEM: &str = "你是 CrabMate 的**规划一致性审计员**。只根据给定的「最近工具结果摘要」与「模型输出的 agent_reply_plan JSON」判断是否明显矛盾（例如计划声称成功但工具报错、计划步骤与工具输出冲突）。\n\
你必须**只输出一个 JSON 对象**（不要 Markdown 代码围栏、不要前后缀说明）。字段要求：\n\
- 若**无法判断**或**无明显矛盾**：{\"consistent\":true}\n\
- 若**明确矛盾**：{\"consistent\":false,\"violation_codes\":[\"…\"],\"rationale\":\"不超过 80 字的中文理由\"}\n\
其中 **violation_codes** 为字符串数组：1–8 个元素，每项仅含小写字母、数字、下划线，长度 1–64。建议码：`tool_outcome_contradiction`（与工具成功/失败状态冲突）、`plan_step_tool_mismatch`（步骤与工具输出明显不符）、`claim_not_supported_by_tools`（断言缺乏工具证据）、`semantic_mismatch_other`（其它明确矛盾）。\n\
**兼容**：若你只能输出纯文本，可在一行内写 INCONSISTENT 或 CONSISTENT（将按旧规则解析，但优先使用 JSON）。";

/// 侧向语义校验的解析结果（供重写 user 消息附带 `violation_codes`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlanSemanticLlmOutcome {
    pub consistent: bool,
    /// 仅在 `consistent == false` 时有意义；经规范化，至少含一个后备码。
    pub violation_codes: Vec<String>,
    pub rationale: Option<String>,
}

const MAX_VIOLATION_CODES: usize = 8;
const MAX_CODE_LEN: usize = 64;
const MAX_RATIONALE_CHARS: usize = 120;

fn is_valid_violation_code_token(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    s.len() <= MAX_CODE_LEN
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn normalize_violation_codes(raw: &[serde_json::Value]) -> Vec<String> {
    let mut out = Vec::new();
    for v in raw {
        let Some(s) = v.as_str() else {
            continue;
        };
        let t = s.trim();
        if is_valid_violation_code_token(t) {
            out.push(t.to_string());
        }
        if out.len() >= MAX_VIOLATION_CODES {
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

fn truncate_rationale(s: &str) -> String {
    let mut t = s.trim().to_string();
    let n = t.chars().count();
    if n > MAX_RATIONALE_CHARS {
        t = t.chars().take(MAX_RATIONALE_CHARS).collect::<String>();
        t.push('…');
    }
    t
}

fn outcome_from_json_value(v: &serde_json::Value) -> Option<PlanSemanticLlmOutcome> {
    let consistent = v.get("consistent")?.as_bool()?;
    let mut codes = v
        .get("violation_codes")
        .and_then(|x| x.as_array())
        .map(|a| normalize_violation_codes(a.as_slice()))
        .unwrap_or_default();
    let rationale = v
        .get("rationale")
        .and_then(|x| x.as_str())
        .map(truncate_rationale)
        .filter(|s| !s.is_empty());
    if !consistent && codes.is_empty() {
        codes.push("semantic_mismatch_unspecified".to_string());
    }
    Some(PlanSemanticLlmOutcome {
        consistent,
        violation_codes: codes,
        rationale,
    })
}

fn extract_json_object_slice(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then_some(&s[start..=end])
}

/// 从侧向模型正文中解析 [`PlanSemanticLlmOutcome`]；无法识别时 **fail-open**（视为一致）。
pub(crate) fn parse_plan_semantic_side_reply(text: &str) -> PlanSemanticLlmOutcome {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
        };
    }

    if let Some(slice) = extract_json_object_slice(trimmed)
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(slice)
        && let Some(o) = outcome_from_json_value(&v)
    {
        return o;
    }

    if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(o) = outcome_from_json_value(&v)
    {
        return o;
    }

    let upper = trimmed.to_uppercase();
    if upper.contains("INCONSISTENT") {
        return PlanSemanticLlmOutcome {
            consistent: false,
            violation_codes: vec!["semantic_mismatch_legacy".to_string()],
            rationale: None,
        };
    }
    if upper.contains("CONSISTENT") {
        return PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
        };
    }

    PlanSemanticLlmOutcome {
        consistent: true,
        violation_codes: Vec::new(),
        rationale: None,
    }
}

/// 侧向 `complete_chat_retrying` 的入参（避免单函数参数过多）。
pub(crate) struct PlanSemanticLlmCtx<'a> {
    pub llm_backend: &'a (dyn ChatCompletionsBackend + Send + Sync),
    pub client: &'a reqwest::Client,
    pub api_key: &'a str,
    pub cfg: &'a AgentConfig,
    pub out: Option<&'a mpsc::Sender<String>>,
    pub no_stream: bool,
    pub cancel: Option<&'a AtomicBool>,
    pub plain_terminal_stream: bool,
    pub request_chrome_trace: Option<Arc<crate::request_chrome_trace::RequestTurnTrace>>,
    pub temperature_override: Option<f32>,
    pub seed_override: LlmSeedOverride,
    pub max_tokens: u32,
}

/// 对 `plan_json` 与 `tool_digest` 做一次无工具侧向调用；`tool_digest` 为空时跳过并视为通过。
/// 解析失败或 API 失败时 **fail-open**（返回 `consistent: true`），避免阻断主循环。
pub(crate) async fn evaluate_plan_consistency_with_recent_tools_llm(
    ctx: PlanSemanticLlmCtx<'_>,
    plan_json: &str,
    tool_digest: Option<&str>,
) -> PlanSemanticLlmOutcome {
    let Some(digest) = tool_digest.map(str::trim).filter(|s| !s.is_empty()) else {
        return PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
        };
    };
    let plan_trim = plan_json.trim();
    if plan_trim.is_empty() {
        return PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
        };
    }

    let user_body = format!(
        "### 最近工具结果摘要（截断）\n{}\n\n### agent_reply_plan JSON\n{}",
        truncate_unicode(digest, 6000),
        truncate_unicode(plan_trim, 8000)
    );
    let side_messages = vec![
        Message::system_only(SIDE_SYSTEM),
        Message::user_only(user_body),
    ];
    let mut req = no_tools_chat_request(
        ctx.cfg,
        &side_messages,
        ctx.temperature_override,
        ctx.seed_override,
    );
    req.max_tokens = ctx.max_tokens.clamp(32, 1024);

    let cc = CompleteChatRetryingParams {
        llm_backend: ctx.llm_backend,
        http: ctx.client,
        api_key: ctx.api_key,
        cfg: ctx.cfg,
        out: ctx.out,
        render_to_terminal: false,
        no_stream: ctx.no_stream,
        cancel: ctx.cancel,
        plain_terminal_stream: ctx.plain_terminal_stream,
        request_chrome_trace: ctx.request_chrome_trace,
    };

    let (reply, finish) = match complete_chat_retrying(&cc, &req).await {
        Ok(x) => x,
        Err(e) => {
            log::warn!(
                target: "crabmate::per",
                "final_plan_semantic_check llm_call_failed error={} (fail-open)",
                e
            );
            return PlanSemanticLlmOutcome {
                consistent: true,
                violation_codes: Vec::new(),
                rationale: None,
            };
        }
    };
    if finish == crate::types::USER_CANCELLED_FINISH_REASON {
        return PlanSemanticLlmOutcome {
            consistent: true,
            violation_codes: Vec::new(),
            rationale: None,
        };
    }
    let text = reply.content.as_deref().unwrap_or("").trim();
    let outcome = parse_plan_semantic_side_reply(text);
    if outcome.consistent {
        log::debug!(target: "crabmate::per", "final_plan_semantic_check outcome=consistent");
    } else {
        log::info!(
            target: "crabmate::per",
            "final_plan_semantic_check outcome=inconsistent codes={:?} preview={}",
            outcome.violation_codes,
            crate::redact::preview_chars(text, 120)
        );
    }
    outcome
}

/// 将合法 v1 规划压成单行 JSON，供侧向调用与日志（失败时返回 `{}`）。
pub(crate) fn agent_reply_plan_json_compact(plan: &plan_artifact::AgentReplyPlanV1) -> String {
    plan_artifact::agent_reply_plan_v1_to_json_string(plan).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_json_consistent_minimal() {
        let o = parse_plan_semantic_side_reply(r#"{"consistent":true}"#);
        assert!(o.consistent);
        assert!(o.violation_codes.is_empty());
        assert!(o.rationale.is_none());
    }

    #[test]
    fn parse_json_inconsistent_with_codes() {
        let o = parse_plan_semantic_side_reply(
            r#"{"consistent":false,"violation_codes":["tool_outcome_contradiction"],"rationale":"工具报错但计划写成功"}"#,
        );
        assert!(!o.consistent);
        assert_eq!(
            o.violation_codes,
            vec!["tool_outcome_contradiction".to_string()]
        );
        assert!(o.rationale.is_some());
    }

    #[test]
    fn parse_json_inconsistent_empty_codes_gets_fallback() {
        let o = parse_plan_semantic_side_reply(r#"{"consistent":false}"#);
        assert!(!o.consistent);
        assert_eq!(
            o.violation_codes,
            vec!["semantic_mismatch_unspecified".to_string()]
        );
    }

    #[test]
    fn parse_embedded_json_object() {
        let o = parse_plan_semantic_side_reply(
            "here is {\"consistent\":false,\"violation_codes\":[\"plan_step_tool_mismatch\"]}",
        );
        assert!(!o.consistent);
        assert_eq!(
            o.violation_codes,
            vec!["plan_step_tool_mismatch".to_string()]
        );
    }

    #[test]
    fn parse_legacy_inconsistent_line() {
        let o = parse_plan_semantic_side_reply("INCONSISTENT 与工具结果矛盾");
        assert!(!o.consistent);
        assert_eq!(
            o.violation_codes,
            vec!["semantic_mismatch_legacy".to_string()]
        );
    }

    #[test]
    fn parse_invalid_code_tokens_dropped() {
        let o = parse_plan_semantic_side_reply(
            r#"{"consistent":false,"violation_codes":["ok_code","Bad-Code","also bad"]}"#,
        );
        assert!(!o.consistent);
        assert_eq!(o.violation_codes, vec!["ok_code".to_string()]);
    }
}
