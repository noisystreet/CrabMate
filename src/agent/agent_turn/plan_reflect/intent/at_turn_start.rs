//! 回合起点：**执行约束的语义判定**（#1401 #12）。
//!
//! 历史实现是关键词启发式——三组子串表（`no_write` / `no_command_execution` / `analysis_only`）
//! 再加一道双条件收窄。它两个方向都会错：英文 `explain` 命中 `explainer`、`how to` 命中
//! `show token`；而「别动代码」「不改动」这类**明确禁令**反而不在表里，于是用户说了别改、
//! agent 照改。补词表永远补不完，故改为**一次极短的无工具 LLM 判定**：只回答
//! 「用户是否要求本轮只读」，命中则挂 `ReviewReadonly`。
//!
//! 失败语义是 **fail-open**：开关关闭 / 调用失败 / 被取消 / 解析不出布尔 → 一律**不挂约束**。
//! 这与原先「漏命中 = 不收窄」同向：宁可少限制，也不误锁死可写任务。
//!
//! Ask/Plan 跳过本判定——它们的只读由 `session_mode` 在 `run_dispatch` 里挂载。

use crate::agent::agent_turn::params::RunLoopParams;
use crate::agent::agent_turn::policy::TurnStartSnapshot;
use crate::agent::plan_artifact::PlanStepExecutorKind;
use crate::cm_types::SessionMode;
use crate::types::Message;

/// 判定器系统提示：只让模型回答一个布尔。
///
/// 关键规则是最后两条——**「分析」本身不等于只读**（「分析并修复」仍算授权），
/// 以及「别跑题」这类与执行无关的口语不算禁令。这两条正是关键词表最容易错的地方。
const JUDGE_SYSTEM_PROMPT: &str = "\
你是 CrabMate 的**回合执行约束判定员**。只判断一件事：用户本轮任务是否**要求本轮只读**\
（不得改动文件、不得执行命令/构建/测试），还是**授权改动或执行**。\n\
只输出一个 JSON 对象（不要代码围栏、不要任何说明文字）：\n\
- 只读：{\"review_readonly\":true}\n\
- 其它（含无法确定）：{\"review_readonly\":false}\n\
判断要点：\n\
- 用户**明确禁止**修改文件、或明确禁止运行命令/构建/测试 → true。例：「别动代码，先分析」「不要运行测试，先解释原因」「只看看，不要改」。\n\
- 用户要求分析/解释，但**没有**禁止改动或执行 → **false**。「分析并修复」「解释这个报错然后改掉」仍有授权。\n\
- 「别跑题」「别动手打字」这类与改动/执行无关的口语 → false。\n\
- 无法确定时一律 false。";

/// 判定器输出上限：只需要一个布尔。
const JUDGE_MAX_TOKENS: u32 = 128;

/// 任务文本送判定器前的截断上限（避免超长任务占满 token 预算）。
const JUDGE_TASK_MAX_CHARS: usize = 4000;

/// 判定为只读后注入模型的本轮短约束（勿复述全局只读叙事）。
const EXECUTION_CONSTRAINT_HINT_ZH: &str =
    "【执行约束】本轮：只读诊断，不得改文件/patch，不得跑构建/测试/执行类命令。勿越过。";

/// Ask/Plan 由 mode 决定只读档，跳过 Act 句判定。
#[must_use]
fn should_skip_act_turn_start_judge(session_mode: SessionMode) -> bool {
    session_mode.requires_readonly_tools()
}

/// 最新真实用户任务句（跳过编排注入）；供判定器使用。
fn latest_user_task_for_judge(messages: &[crate::cm_types::Message]) -> String {
    crate::cm_types::last_real_user_task_content(messages, false)
        .unwrap_or_default()
        .to_string()
}

/// 回合起点：判定本轮是否只读，并据此挂 `ReviewReadonly`。
pub(crate) async fn run_act_turn_start_judge(p: &mut RunLoopParams<'_>) {
    let task = latest_user_task_for_judge(p.turn.messages());
    if task.trim().is_empty() {
        p.turn.turn_planner_hints.turn_start_snapshot = Some(TurnStartSnapshot::EmptyTask);
        return;
    }

    if should_skip_act_turn_start_judge(p.ctx.attach.session_mode) {
        p.turn.turn_planner_hints.turn_start_snapshot = Some(TurnStartSnapshot::Disabled);
        return;
    }

    apply_turn_start_judgement(p, &task).await;
}

async fn apply_turn_start_judgement(p: &mut RunLoopParams<'_>, task: &str) {
    let review_readonly = judge_review_readonly(p, task).await.unwrap_or(false);
    if review_readonly {
        p.turn.turn_planner_hints.step_executor_constraint =
            Some(PlanStepExecutorKind::ReviewReadonly);
        p.turn.turn_planner_hints.execution_constraint_hint =
            Some(EXECUTION_CONSTRAINT_HINT_ZH.to_string());
    }
    p.turn.turn_planner_hints.turn_start_snapshot =
        Some(TurnStartSnapshot::ActHeuristics { review_readonly });
}

/// 从判定器正文里取布尔 `review_readonly`；取不到返回 `None`（调用方 fail-open）。
fn parse_judge_reply(text: &str) -> Option<bool> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let slice = json_object_slice(trimmed).unwrap_or(trimmed);
    serde_json::from_str::<serde_json::Value>(slice)
        .ok()?
        .get("review_readonly")?
        .as_bool()
}

/// 取第一个 `{` 到最后一个 `}` 的片段：模型偶尔仍会裹代码围栏或加前后缀。
fn json_object_slice(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    (end > start).then_some(&s[start..=end])
}

/// 一次无工具侧向调用判定「是否只读」。
///
/// 返回 `None` 表示**没有结论**（关闭 / 失败 / 取消 / 解析不出），调用方据此不挂约束。
async fn judge_review_readonly(p: &RunLoopParams<'_>, task: &str) -> Option<bool> {
    let cfg = p.ctx.core.cfg.as_ref();
    if !cfg.per_plan_policy.turn_execution_constraint_judge_enabled {
        return None;
    }
    let task_trim = task.trim();
    if task_trim.is_empty() {
        return None;
    }

    let messages = vec![
        Message::system_only(JUDGE_SYSTEM_PROMPT),
        Message::user_only(format!(
            "### 用户本轮任务\n{}",
            crate::cm_types::text_utils::preview_chars(task_trim, JUDGE_TASK_MAX_CHARS)
        )),
    ];
    let model_override = p
        .turn
        .model_override
        .as_deref()
        .or(cfg.llm.planner_model.as_deref());
    let llm_cfg = crate::cm_types::llm_config::LlmConfig {
        llm: cfg.llm.clone(),
        sampling: cfg.llm_sampling.clone(),
        vendor_flags: cfg.llm_vendor_flags.clone(),
        http_retry: cfg.llm_http_retry.clone(),
    };
    let mut req = crate::llm::no_tools_chat_request(
        &llm_cfg,
        &messages,
        p.turn.temperature_override,
        model_override,
        p.turn.seed_override,
    );
    req.max_tokens = JUDGE_MAX_TOKENS;

    // 刻意**不**挂 `with_turn_budget`：本判定是回合基建，不该从任务本身的
    // `max_llm_calls` 配额里拿走一次——配得紧的用户会因此在任务开始前就少一次调用机会。
    let cc = crate::llm::CompleteChatRetryingParams::new(
        p.ctx.core.llm_backend,
        p.ctx.core.client,
        p.ctx.core.api_key,
        cfg,
        crate::llm::LlmRetryingTransportOpts {
            out: p.ctx.io.control.out,
            no_stream: p.ctx.io.no_stream,
            cancel: p.ctx.io.cancel.as_deref(),
        },
        p.ctx.obs.request_chrome_trace.clone(),
        model_override,
    );

    let (reply, finish) = match crate::llm::complete_chat_retrying(&cc, &req).await {
        Ok(x) => x,
        Err(e) => {
            // fail-open：判定失败不该阻断回合，也不该误锁成只读。
            tracing::warn!(
                target: "crabmate::agent_turn",
                "turn_execution_constraint_judge llm_call_failed error={e} (fail-open)"
            );
            return None;
        }
    };
    if finish == crate::types::USER_CANCELLED_FINISH_REASON {
        return None;
    }

    let text = crate::types::message_content_as_str(&reply.content)
        .unwrap_or("")
        .trim()
        .to_string();
    let verdict = parse_judge_reply(&text);
    tracing::debug!(
        target: "crabmate::agent_turn",
        verdict = ?verdict,
        preview = %crate::redact::preview_chars(&text, 120),
        "turn_execution_constraint_judge"
    );
    verdict
}

#[cfg(test)]
mod skip_judge_tests {
    use super::should_skip_act_turn_start_judge;
    use crate::cm_types::SessionMode;

    #[test]
    fn skips_ask_plan() {
        assert!(should_skip_act_turn_start_judge(SessionMode::Ask));
        assert!(should_skip_act_turn_start_judge(SessionMode::Plan));
    }

    #[test]
    fn runs_for_act() {
        assert!(!should_skip_act_turn_start_judge(SessionMode::Act));
    }
}

#[cfg(test)]
mod tests {
    use super::{EXECUTION_CONSTRAINT_HINT_ZH, JUDGE_SYSTEM_PROMPT, parse_judge_reply};

    #[test]
    fn parses_plain_verdict() {
        assert_eq!(parse_judge_reply(r#"{"review_readonly":true}"#), Some(true));
        assert_eq!(
            parse_judge_reply(r#"{"review_readonly":false}"#),
            Some(false)
        );
    }

    #[test]
    fn parses_verdict_wrapped_in_prose_or_fence() {
        assert_eq!(
            parse_judge_reply("结论如下：\n```json\n{\"review_readonly\":true}\n```\n以上。"),
            Some(true)
        );
    }

    #[test]
    fn unparsable_reply_yields_none_for_fail_open() {
        assert_eq!(parse_judge_reply(""), None);
        assert_eq!(parse_judge_reply("   "), None);
        assert_eq!(parse_judge_reply("我觉得是只读"), None);
        // 布尔字段缺失或类型不对同样没有结论。
        assert_eq!(parse_judge_reply(r#"{"review_readonly":"true"}"#), None);
        assert_eq!(parse_judge_reply(r#"{"other":true}"#), None);
    }

    /// 提示词必须守住两条最容易错的边界：分析 ≠ 只读、无关口语不算禁令。
    #[test]
    fn judge_prompt_pins_the_hard_cases() {
        assert!(JUDGE_SYSTEM_PROMPT.contains("分析并修复"));
        assert!(JUDGE_SYSTEM_PROMPT.contains("别跑题"));
        assert!(JUDGE_SYSTEM_PROMPT.contains("无法确定时一律 false"));
    }

    #[test]
    fn execution_constraint_hint_stays_short() {
        assert!(EXECUTION_CONSTRAINT_HINT_ZH.starts_with("【执行约束】本轮："));
        assert!(EXECUTION_CONSTRAINT_HINT_ZH.ends_with("勿越过。"));
        assert!(
            EXECUTION_CONSTRAINT_HINT_ZH.chars().count() <= 120,
            "hint should stay short, got {} chars",
            EXECUTION_CONSTRAINT_HINT_ZH.chars().count()
        );
    }

    #[test]
    fn latest_user_task_skips_orchestration_injection() {
        use super::latest_user_task_for_judge;
        let messages = vec![
            crate::cm_types::Message::user_only("编译 hpcg"),
            crate::cm_types::Message::user_only("【编排纠偏】继续构建"),
        ];
        assert_eq!(latest_user_task_for_judge(&messages), "编译 hpcg");
    }
}
