//! Web 内置 `/btw` 命令：**旁路提问**（off-the-record side question）。
//!
//! 用当前会话上下文回答，但：
//! - **不**写入会话历史、不增长上下文（短路返回，不落盘）；
//! - **不**给模型任何工具（`no_tools_chat_request`，`tool_choice=none`）。

use std::sync::Arc;

use crate::chat_job_queue::WebChatLlmOverride;
use crate::clarification_questionnaire::{
    ClarifyAnswersNormalized, merge_user_text_with_clarification_answers,
};
use crate::types::LlmSeedOverride;
use crate::web::app_state_facets::WebChatTurnAppFacet;

const BTW_USAGE: &str = "用法：`/btw <问题>`\n\n旁路提问：结合当前会话上下文回答，但该问答**不写入会话历史**、不增长上下文，模型也**不会**调用任何工具。";

/// 展示给用户的标记：明确本次回答为不落盘的旁路提问。
const BTW_REPLY_MARKER: &str = "〔旁路提问 · 未写入会话历史〕\n\n";

/// 送给模型的补充说明：明确这是一次不落盘的旁路提问。
///
/// 置于问题**之前**（前缀），使送入 `build_messages_for_turn` 的字符串不以 `/` 开头，
/// 从而不会命中 `prepare_user_message_for_skills` 的 `/<skill-id>` 强制技能解析
/// （否则问题以 `/<skill-id>` 开头会被剥离/误注入，甚至因技能不存在而整体报错）。
const BTW_MODEL_HINT: &str =
    "（旁路提问：请结合上文上下文直接回答；本条问答不会写入会话历史，也无需调用任何工具。）\n\n";

/// 识别 `/btw` 前缀并返回其后的**问题文本**（大小写不敏感；`/btw` 后须为空白或行尾）。
///
/// - `/btw 问题` → `Some("问题")`
/// - `/btw`（无问题）→ `Some("")`（由调用方给出用法）
/// - 其它 → `None`
pub(super) fn classify_btw_command(input: &str) -> Option<&str> {
    let trimmed = input.trim();
    let rest = trimmed.strip_prefix('/')?;
    let rest = rest.trim_start();
    let mut parts = rest.splitn(2, char::is_whitespace);
    let head = parts.next().unwrap_or("");
    if !head.eq_ignore_ascii_case("btw") {
        return None;
    }
    Some(parts.next().unwrap_or("").trim())
}

/// 是否为 `/btw` 命令（含无问题时的用法提示形态）。
pub(super) fn is_btw_command(input: &str) -> bool {
    classify_btw_command(input).is_some()
}

/// `/btw` 短路命令的请求视图（由两种 handler 的解析结果构造，避免超长参数列表）。
pub(super) struct BtwCommandRequest<'a> {
    pub(super) conversation_id: &'a str,
    pub(super) user_trim: &'a str,
    pub(super) image_urls: &'a [String],
    pub(super) clarify: Option<&'a ClarifyAnswersNormalized>,
    pub(super) llm_override: Option<&'a WebChatLlmOverride>,
    pub(super) temperature_override: Option<f32>,
    pub(super) seed_override: LlmSeedOverride,
}

/// 执行 `/btw` 短路命令；非 `/btw` 返回 `None`。
///
/// 结果直接返回给调用方（JSON 走 `ChatResponseBody`，SSE 走单条内置事件），
/// **绝不**经由回合队列，因此不会落盘。
pub(super) async fn run_btw_command(
    state: &WebChatTurnAppFacet,
    req: BtwCommandRequest<'_>,
) -> Option<String> {
    let question = classify_btw_command(req.user_trim)?;
    if question.is_empty() {
        return Some(BTW_USAGE.to_string());
    }
    Some(match answer_btw(state, &req, question).await {
        Ok(text) if !text.trim().is_empty() => format!("{BTW_REPLY_MARKER}{text}"),
        Ok(_) => format!("{BTW_REPLY_MARKER}（模型返回了空回复）"),
        Err(msg) => format!("旁路提问失败：{msg}"),
    })
}

/// 组装上下文并无工具调用一次模型。
///
/// 为复用 `chat_queue_max_concurrent` 上限，调用模型前先从队列的**共用**信号量取 permit
/// （队列饱和时在此等待），避免短路路径绕过队列放大并发。
async fn answer_btw(
    state: &WebChatTurnAppFacet,
    req: &BtwCommandRequest<'_>,
    question: &str,
) -> Result<String, String> {
    let deps = state.chat.chat_queue_job_deps.clone();
    let cfg_snap = {
        let g = state.cfg.read().await;
        Arc::new(g.clone())
    };
    let (eff_cfg, api_key) =
        crate::chat_job_queue::resolve_web_llm_for_job(deps.as_ref(), cfg_snap, req.llm_override);
    let llm_backend = deps
        .llm_backend
        .unwrap_or_else(crate::llm::default_chat_completions_backend);

    // 复用普通回合的上下文组装（含 system / 工作区画像 / 历史），但**不落盘**：
    // `build_messages_for_turn` 只操作 `load_conversation_seed` 的克隆。
    // hint 前置（见 `BTW_MODEL_HINT`）可规避 `/<skill-id>` 强制技能误判。
    let user_for_model = format!("{BTW_MODEL_HINT}{question}");
    let user_for_model =
        merge_user_text_with_clarification_answers(user_for_model, req.clarify.cloned());
    let seed = super::turn_build::build_messages_for_turn(
        state,
        req.conversation_id,
        user_for_model.as_str(),
        req.image_urls,
        None,
        None,
    )
    .await?;
    let messages = seed.messages;
    let model_override = req.llm_override.and_then(|o| o.model.as_deref());

    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: eff_cfg.llm.clone(),
        sampling: eff_cfg.llm_sampling.clone(),
        vendor_flags: eff_cfg.llm_vendor_flags.clone(),
        http_retry: eff_cfg.llm_http_retry.clone(),
    };
    let chat_req = crate::llm::no_tools_chat_request(
        &llm_cfg,
        &messages,
        req.temperature_override,
        model_override,
        req.seed_override,
    );
    // 复用队列信号量限流；客户端断开时 handler future 被 drop，会连带中止在途请求。
    let _permit = state
        .chat
        .chat_queue
        .turn_semaphore()
        .acquire_owned()
        .await
        .map_err(|_| "会话队列已关闭".to_string())?;
    let cc = crate::llm::CompleteChatRetryingParams::new(
        llm_backend,
        &state.client,
        &api_key,
        eff_cfg.as_ref(),
        crate::llm::LlmRetryingTransportOpts {
            out: None,
            no_stream: true,
            cancel: None,
        },
        None,
        model_override,
    );
    let (msg, _finish_reason) = crate::llm::complete_chat_retrying(&cc, &chat_req)
        .await
        .map_err(|e| e.to_string())?;
    Ok(crate::types::message_content_as_str(&msg.content)
        .unwrap_or("")
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_btw_prefix() {
        assert_eq!(classify_btw_command("/btw 为什么"), Some("为什么"));
        assert_eq!(classify_btw_command("/BTW   a b "), Some("a b"));
        assert_eq!(classify_btw_command("/btw"), Some(""));
        assert_eq!(classify_btw_command("/btw   "), Some(""));
        assert_eq!(classify_btw_command("  /btw hi"), Some("hi"));
    }

    #[test]
    fn rejects_non_btw() {
        assert_eq!(classify_btw_command("/btwx hi"), None);
        assert_eq!(classify_btw_command("/skills"), None);
        assert_eq!(classify_btw_command("btw hi"), None);
        assert_eq!(classify_btw_command(""), None);
    }

    #[test]
    fn model_hint_prefix_avoids_skill_slash_detection() {
        // 问题以 `/<skill-id>` 开头时，前置 hint 使送入模型组装的字符串不以 `/` 开头，
        // 从而不会命中 `prepare_user_message_for_skills` 的强制技能解析。
        let user_for_model = format!("{BTW_MODEL_HINT}/some-skill 任务");
        assert!(!user_for_model.starts_with('/'));
        assert!(user_for_model.ends_with("/some-skill 任务"));
    }
}
