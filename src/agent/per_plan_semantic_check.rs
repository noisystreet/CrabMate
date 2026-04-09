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

const SIDE_SYSTEM: &str = "你是 CrabMate 的**规划一致性审计员**。只根据给定的「最近工具结果摘要」与「模型输出的 agent_reply_plan JSON」判断是否明显矛盾（例如计划声称成功但工具报错、计划步骤与工具状态冲突）。\n\
若**无法判断**或**无明显矛盾**，一律回答 CONSISTENT。\n\
若**明确矛盾**，回答 INCONSISTENT 并在同一行后附 1 句中文理由（不超过 80 字）。\n\
**不要**输出其它段落或 Markdown。";

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
/// 解析失败或 API 失败时 **fail-open**（返回 `true`），避免阻断主循环。
pub(crate) async fn plan_consistent_with_recent_tools_llm(
    ctx: PlanSemanticLlmCtx<'_>,
    plan_json: &str,
    tool_digest: Option<&str>,
) -> bool {
    let Some(digest) = tool_digest.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    let plan_trim = plan_json.trim();
    if plan_trim.is_empty() {
        return true;
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
            return true;
        }
    };
    if finish == crate::types::USER_CANCELLED_FINISH_REASON {
        return true;
    }
    let text = reply.content.as_deref().unwrap_or("").trim();
    if text.is_empty() {
        return true;
    }
    let upper = text.to_uppercase();
    if upper.contains("INCONSISTENT") {
        log::info!(
            target: "crabmate::per",
            "final_plan_semantic_check outcome=inconsistent preview={}",
            crate::redact::preview_chars(text, 120)
        );
        return false;
    }
    if upper.contains("CONSISTENT") {
        log::debug!(target: "crabmate::per", "final_plan_semantic_check outcome=consistent");
        return true;
    }
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(text)
        && let Some(b) = v.get("consistent").and_then(|x| x.as_bool())
    {
        return b;
    }
    true
}

/// 将合法 v1 规划压成单行 JSON，供侧向调用与日志（失败时返回 `{}`）。
pub(crate) fn agent_reply_plan_json_compact(plan: &plan_artifact::AgentReplyPlanV1) -> String {
    plan_artifact::agent_reply_plan_v1_to_json_string(plan).unwrap_or_else(|_| "{}".to_string())
}
