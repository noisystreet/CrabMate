//! 回合级「目标完成 / 早停 / 冗余工具抑制 / 终答纠偏」共用判定。
//!
//! 纯判定核（[`TurnCompletionDecision`] / evaluate_*）已下沉 **`crate::agent::agent_turn::policy`**；本文件保留
//! 外循环纠偏文案与依赖 `tool_result` 信封的终答空答检测。

#[cfg(test)]
#[path = "turn_completion_golden.rs"]
mod turn_completion_golden;

pub(crate) use crate::agent::agent_turn::policy::turn_completion_decision::{
    TurnCompletionDecision, evaluate_turn_redundant_tools, log_turn_completion_decision,
};

pub(crate) use crate::agent::agent_turn::policy::completion_suppression::redundant_tool_names_for_log;

pub(crate) use crate::cm_display_rules::OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX;

use crate::types::{Message, ToolCall, last_real_user_message_index, message_content_as_str};

use crate::agent::agent_turn::params::RunLoopParams;
use crate::agent::agent_turn::policy::task_level_evidence::EvidenceSet;
use crate::agent::completion_judge_llm::LlmCompletionJudge;
use crate::cm_agent::completion_judge::{
    CompletionJudgeInput, CompletionStatus, JudgedEvidence, judge_and_validate,
};

/// 最多注入次数，避免与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX: u32 = 2;

/// 低于该字符数（Unicode scalar）视为「无可见终答」。
pub(crate) const OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS: usize = 24;

/// 截断纠偏最多注入次数，避免长输出反复触发与外层迭代上限死磕。
pub(crate) const OUTER_LOOP_TRUNCATED_FEEDBACK_MAX: u32 = 2;

/// 早停门的**唯一判定**：问第 ② 层 LLM 语义判定（#1387 S0.5 / #1415）。
///
/// 第 ① 层（结构化证据 + 领域检测器）已删除：检测器只有 C++ 一个、且它本身就是文本启发式，
/// 保留它意味着同一事实存在两条判定路径。现在结构化证据只作为 **judge 的输入**，不自下结论。
///
/// - 判 `satisfied` 且锚点校验通过 → 放行，并把结论缓存到 `PerCoordinator`（同步门复用）；
/// - `satisfied` 但锚点不过 → `judge_and_validate` 已降级为 `unsatisfied`，放行不了；
/// - 判定层不可用（`completion_judge_enabled = false` / LLM 失败 / 回复无法解析 / 无可用窗口）
///   → **不早停**（fail-open，绝不因判定失败而早停）。
///
/// **代价（有意接受）**：早停能力现在**完全依赖 LLM 可用性**。关掉 `completion_judge_enabled`
/// 或缺 `API_KEY` 时，任何任务都不会提前停轮，回合会跑到迭代上限。
pub(crate) async fn turn_early_stop_allowed_with_judge(
    p: &RunLoopParams<'_>,
    per_coord: &mut crate::agent::per_coord::PerCoordinator,
) -> bool {
    let messages = p.turn.messages();
    // 证据必须按「最后一个真实 user」窗口裁剪：全量历史会把上一任务跑通的 `cargo_test`
    // 带进本轮判定输入，而锚点校验只问「有没有 verify/write 成功」、不问属不属于本窗口。
    let Some(window) = crate::cm_types::messages_slice_since_last_real_user(messages, false) else {
        return false;
    };
    let ev = EvidenceSet::collect(window);
    // 先构造判定器：开关关时直接返回 `None`，此时**不应消耗配额**也不该写指纹。
    // 顺序颠倒会让「关掉 judge」的部署在每个工具批白涨 `completion_judge_calls`，
    // 把那个计数器变成假指标。
    let Some(judge) = LlmCompletionJudge::from_config(
        p.ctx.core.cfg,
        p.ctx.core.llm_backend,
        p.ctx.core.client,
        p.ctx.core.api_key,
        p.ctx.io.cancel.as_deref(),
        Some(&p.turn.turn_budget),
    ) else {
        return false;
    };
    // 去重：本门在**每个工具批之后**都会走一次。指纹不变 = 没有新事实，重复问纯属浪费。
    if !per_coord.take_completion_judge_slot(evidence_fingerprint(&ev)) {
        return false;
    }
    let verdict = judge_and_validate(&judge, build_judge_input_from(window, &ev)).await;
    let satisfied = match &verdict {
        Some(v) => v.status == CompletionStatus::Satisfied,
        None => false,
    };
    // #1472：把**结构化结论**（`status` + `missing`）留给收尾层，供 `stream_ended.reason`
    // 区分「正常完成」与「有终答但未完成」。此前这一行只留 `== Satisfied` 一个 bool，
    // `Partial` 与 `missing` 直接丢失。写入只影响终态标注，不改变本门的 stop / continue。
    if let Some(v) = verdict {
        per_coord.record_completion_verdict(v);
    }
    // 缓存结论：冗余探针抑制等同步门拿不到 judge，靠它对齐口径。
    if satisfied {
        per_coord.mark_completion_confirmed();
    }
    satisfied
}

/// 证据指纹：条目数 + 最后一条的 id/成败。
///
/// 只求「是否有新事实」的粗判，不做内容哈希——判定输入本就截断，逐字节比对既贵又无意义。
fn evidence_fingerprint(ev: &EvidenceSet) -> u64 {
    let mut acc: u64 = ev.items().len() as u64;
    if let Some(last) = ev.items().last() {
        acc = acc
            .wrapping_mul(31)
            .wrapping_add(last.id.len() as u64)
            .wrapping_mul(31)
            .wrapping_add(u64::from(last.ok));
    }
    acc
}

/// 把第 ① 层证据投影成判定输入。
///
/// 只传任务原文 + 结构化证据，**不传 assistant 自述**——这是第 ② 层锚点机制成立的前提。
fn build_judge_input_from(messages: &[Message], ev: &EvidenceSet) -> CompletionJudgeInput {
    CompletionJudgeInput {
        task: crate::cm_types::last_real_user_task_content(messages, false)
            .unwrap_or_default()
            .to_string(),
        evidence: ev
            .items()
            .iter()
            .map(|i| JudgedEvidence {
                id: i.id.clone(),
                kind: i.kind.as_str().to_string(),
                strength: i.strength.as_str().to_string(),
                tool: i.tool.clone(),
                ok: i.ok,
                exit_code: i.exit_code,
                detail: i.detail.clone(),
            })
            .collect(),
    }
}

#[cfg(test)]
fn build_completion_judge_input(messages: &[Message]) -> CompletionJudgeInput {
    build_judge_input_from(messages, &EvidenceSet::collect(messages))
}

/// 活跃目标已有完成证据且允许早停时，是否应静默丢弃本轮探针类 / 重复 `run_command` 工具调用。
///
/// `completion_confirmed` 来自 `PerCoordinator`：语义判定的结论由早停门写入，本门只读，
/// 使两道门对同一批证据给出一致结论（#1415）。
pub(crate) fn turn_redundant_tools_after_completion_allowed(
    tool_calls: &[ToolCall],
    messages: &[Message],
    completion_confirmed: bool,
) -> bool {
    evaluate_turn_redundant_tools(tool_calls, messages, completion_confirmed).is_allow()
}

/// 旧式文本输出里 `退出码：N` / `exit code: N` 的 N 是否为 0。
///
/// 不能写成 `contains("退出码：0")`：`退出码：01`、`exit code: 05` 都以该子串开头，
/// 会把非零退出判成成功（#1478）。这里取标签后的连续数字再比数值。
///
/// **已知残余**（不在本函数内解决）：
/// - 只看「文本里出现过零退出码」，不区分它是不是**本条结果自己的**退出码；说明性文本
///   （如 `expected exit code: 0, got 1`）仍会被判成功。
/// - 标签表只含全角冒号 `退出码：` 与带空格的 `exit code: `；`退出码: 0`（半角）不识别。
/// - 同一段里既有 0 又有非 0 时取「任一为 0」。
///
/// 真正的收敛要等结构化 `error_code` 覆盖到这条路径（#1478 主线）。
fn text_reports_zero_exit_code(raw: &str) -> bool {
    const EXIT_CODE_LABELS: &[&str] = &["退出码：", "exit code: "];
    let lower = raw.to_lowercase();
    EXIT_CODE_LABELS.iter().any(|label| {
        lower.match_indices(label).any(|(start, matched)| {
            let digits: String = lower[start + matched.len()..]
                .chars()
                .take_while(|c: &char| c.is_ascii_digit())
                .collect();
            digits.parse::<u32>().is_ok_and(|n| n == 0)
        })
    })
}

fn outer_loop_window_has_any_successful_tool(messages: &[Message]) -> bool {
    let Some(user_idx) = last_real_user_message_index(messages, false) else {
        return false;
    };
    messages[user_idx.saturating_add(1)..].iter().any(|m| {
        if m.role != "tool" {
            return false;
        }
        let Some(raw) = message_content_as_str(&m.content) else {
            return false;
        };
        if let Some(env) = crate::tool_result::normalize_tool_message_content(raw) {
            return env.ok || env.exit_code == Some(0);
        }
        text_reports_zero_exit_code(raw)
    })
}

/// 自本轮 user 起，是否已有一条**可见终答**的 assistant 正文（#1414）。
///
/// 早停门的必要条件：**有完成证据 ≠ 消费者拿到了答案**。工具轮的 assistant 通常只带
/// `tool_calls`、正文为空，此时直接停轮会让回合静默结束（收尾层只能回退思维链片段）。
///
/// 判据只认**纯文本轮**：带 `tool_calls` 的 assistant 即便写了正文，也只是「打算做什么」
/// 的过程叙述（#1414 复盘：模型先写「我先看实现、再跑测试」，随后又发一批工具调用；把
/// 这条算成终答，早停门照样会在用户拿到结论之前放行）。不看 `tool_calls` 的写法见
/// [`outer_loop_assistant_lacks_visible_final_answer`]——那是 stop-turn 分支的反向语义，
/// 两者不可互换。
pub(crate) fn outer_loop_has_visible_final_answer_since_last_user(messages: &[Message]) -> bool {
    let Some(user_idx) = last_real_user_message_index(messages, false) else {
        return false;
    };
    messages[user_idx.saturating_add(1)..].iter().any(|m| {
        m.role == "assistant"
            && m.tool_calls.as_ref().is_none_or(|c| c.is_empty())
            && message_content_as_str(&m.content).is_some_and(|s| {
                s.trim().chars().count() >= OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS
            })
    })
}

pub(crate) fn outer_loop_assistant_lacks_visible_final_answer(msg: &Message) -> bool {
    if msg.tool_calls.as_ref().is_some_and(|c| !c.is_empty()) {
        return false;
    }
    let text = message_content_as_str(&msg.content).unwrap_or("").trim();
    text.chars().count() < OUTER_LOOP_MISSING_FINAL_ANSWER_MIN_CHARS
}

pub(crate) fn outer_loop_missing_final_answer_feedback_body() -> String {
    format!(
        "{prefix}本轮工具已执行并产生结果，但助手终答为空或过短。请基于**当前对话与工具输出**，用自然语言向用户给出完整终答：\
         说明已完成什么、关键结果/路径/命令输出摘要，以及若仍有未完成项请明确列出。**禁止**再发起无必要的 tool_calls。",
        prefix = OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX
    )
}

/// 若应注入纠偏 user 并继续外循环，返回 `Some(feedback)`。
pub(crate) fn outer_loop_missing_final_answer_feedback_if_needed(
    messages: &[Message],
    assistant: &Message,
    feedback_injected_count: u32,
) -> Option<String> {
    let decision = if feedback_injected_count >= OUTER_LOOP_MISSING_FINAL_ANSWER_FEEDBACK_MAX {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "feedback_budget_exhausted",
        }
    } else if !outer_loop_assistant_lacks_visible_final_answer(assistant) {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "assistant_has_visible_answer",
        }
    } else if !outer_loop_window_has_any_successful_tool(messages) {
        TurnCompletionDecision::DenyMissingFinalAnswerFeedback {
            reason: "no_successful_tool_in_window",
        }
    } else {
        TurnCompletionDecision::AllowMissingFinalAnswerFeedback
    };
    log_turn_completion_decision(decision, "missing_final_answer_feedback");
    if decision.is_allow() {
        Some(outer_loop_missing_final_answer_feedback_body())
    } else {
        None
    }
}

pub(crate) fn outer_loop_truncated_feedback_body() -> String {
    format!(
        "{prefix}上一轮助手输出因达到模型长度上限或流式中断而被**截断**，终答不完整。\
         请基于已有上下文**继续**并给出完整、可验收的终答：若任务已完成，直接输出最终结论；\
         若仍需执行，请通过 `tool_calls` 继续；避免重复已输出内容，必要时分段或精炼。",
        prefix = OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX
    )
}

/// 若本轮 assistant 输出被上游截断且仍有纠偏预算，返回 `Some(feedback)`。
///
/// 触发仅依赖层 1 的截断信号（[`crate::per_turn_flight::PerTurnFlight::last_assistant_finish_truncated`]）；
/// 与正文长度无关——截断常伴随**已有较长但未完成**的正文，故不复用终答缺失的字符数启发式。
pub(crate) fn outer_loop_truncated_feedback_if_needed(
    truncated: bool,
    feedback_injected_count: u32,
) -> Option<String> {
    if !truncated || feedback_injected_count >= OUTER_LOOP_TRUNCATED_FEEDBACK_MAX {
        return None;
    }
    Some(outer_loop_truncated_feedback_body())
}

#[cfg(test)]
mod tests {

    use super::*;
    use crate::types::Message;

    use crate::test_fixtures::{msg, tool_env};

    /// #1414：工具轮的 assistant 只有 `tool_calls`、正文为空 → 视为「尚无可见终答」。
    ///
    /// 这是早停门的必要条件：有完成证据不等于消费者拿到了答案。
    #[test]
    fn visible_final_answer_absent_for_tool_only_assistant() {
        let messages = vec![
            msg("user", "分析这个 issue 怎么修"),
            Message {
                role: "assistant".into(),
                content: None,
                reasoning_content: Some("先跑一下编译".into()),
                reasoning_details: None,
                tool_calls: Some(vec![ToolCall {
                    id: "call_1".into(),
                    typ: "function".into(),
                    function: crate::types::FunctionCall {
                        name: "cargo_check".into(),
                        arguments: "{}".into(),
                    },
                }]),
                name: None,
                tool_call_id: None,
            },
            tool_env("cargo_check", "cargo check", "退出码：0\n标准输出：\nFinished"),
        ];
        assert!(
            !outer_loop_has_visible_final_answer_since_last_user(&messages),
            "只有 tool_calls 的 assistant 不应算作可见终答"
        );
    }

    /// 同一判据在 assistant 给出足够长的正文时必须放行（否则会一直要求终答）。
    #[test]
    fn visible_final_answer_detected_from_assistant_body() {
        let messages = vec![
            msg("user", "分析这个 issue 怎么修"),
            msg("assistant", "结论：早停门缺终答条件，按方案 A 落地并补测试即可。"),
        ];
        assert!(outer_loop_has_visible_final_answer_since_last_user(&messages));
    }

    /// #1414 复盘：夹带 `tool_calls` 的长正文只是「打算做什么」的过程叙述，**不算**可见终答。
    ///
    /// 模型常见形态是先写「我先看实现、再跑测试」再发工具调用。若把这条算成终答，早停门
    /// 依旧会在用户拿到结论之前放行——这正是首轮修复（只看正文长度）漏掉的那一半。
    #[test]
    fn visible_final_answer_ignores_narration_that_carries_tool_calls() {
        let mut narration = msg(
            "assistant",
            "我先看下相关实现，再跑一遍编译与测试，最后把完整结论写给你，请稍等。",
        );
        narration.tool_calls = Some(vec![ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: crate::types::FunctionCall {
                name: "cargo_check".into(),
                arguments: "{}".into(),
            },
        }]);
        let messages = vec![
            msg("user", "分析这个 issue 怎么修"),
            narration,
            tool_env("cargo_check", "cargo check", "退出码：0"),
        ];
        assert!(
            !outer_loop_has_visible_final_answer_since_last_user(&messages),
            "夹带 tool_calls 的叙述不得当作可见终答"
        );
    }

    /// 第 ② 层输入只含「任务原文 + 结构化证据」，**不得含 assistant 自述**。
    ///
    /// 这条是锚点机制成立的前提：一旦把自述喂给判定器，它就会顺着执行者自己的
    /// 结论点头，`evidence_refs` 校验也就失去意义。
    #[test]
    fn judge_input_excludes_assistant_claim() {
        let messages = vec![
            msg("user", "跑通全部测试"),
            tool_env(
                "cargo_test",
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 100 passed; 0 failed",
            ),
            msg("assistant", "我已经全部改完并通过了所有测试"),
        ];
        let input = build_completion_judge_input(&messages);
        assert_eq!(input.task, "跑通全部测试");
        assert_eq!(input.evidence.len(), 1);
        assert_eq!(input.evidence[0].id, "ev_1");
        assert_eq!(input.evidence[0].kind, "verify");
        assert_eq!(input.evidence[0].strength, "strong");
        assert!(input.evidence[0].ok);
        // 自述文本不得出现在判定输入的任何字段里。
        let dumped = format!("{input:?}");
        assert!(!dumped.contains("我已经全部改完"));
    }

    /// 无 user 任务 / 无工具消息时仍返回结构完整的空输入（不 panic）。
    #[test]
    fn judge_input_is_empty_safe() {
        let messages = vec![msg("assistant", "你好")];
        let input = build_completion_judge_input(&messages);
        assert!(input.task.is_empty());
        assert!(input.evidence.is_empty());
    }

    /// 证据增长时指纹必须变化——否则「有新事实」永远检测不到。
    #[test]
    fn evidence_fingerprint_changes_with_new_evidence() {
        let one = vec![
            msg("user", "分析项目"),
            tool_env("list_tree", "list tree: .", "list tree: ."),
        ];
        let two = vec![
            msg("user", "分析项目"),
            tool_env("list_tree", "list tree: .", "list tree: ."),
            tool_env("read_file", "read file: a", "read file: a"),
        ];
        let fp1 = evidence_fingerprint(&EvidenceSet::collect(&one));
        let fp2 = evidence_fingerprint(&EvidenceSet::collect(&two));
        assert_ne!(fp1, fp2);
    }

    fn list_tree_probe() -> ToolCall {
        ToolCall {
            id: "call_1".into(),
            typ: "function".into(),
            function: crate::types::FunctionCall {
                name: crate::cm_names::LIST_TREE.into(),
                arguments: r#"{"path":"."}"#.into(),
            },
        }
    }

    /// #1415：早停门（可问 judge）写入的 `completion_confirmed` 必须被**同步**的冗余探针抑制读到。
    ///
    /// 只读分析任务现在第 ① 层恒为 `NotApplicable`，同步门单独判会拒绝；早停门用语义判定
    /// 确认后，抑制门必须给出同一结论——否则同一批证据两道门相反，探针照跑、轮次照烧。
    #[test]
    fn completion_confirmed_lets_sync_gate_agree_with_judge() {
        let messages = vec![
            msg("user", "分析当前目录"),
            tool_env(
                "read_file",
                "read file: README.md",
                "读取文件\n退出码：0\n标准输出：\n# Project\n",
            ),
            msg("assistant", "当前目录包含 README 与源码目录，分析结果如下。"),
        ];
        let tools = vec![list_tree_probe()];
        // 未确认：同步路径拿不到 judge，只有第 ① 层 → 拒绝。
        assert!(!evaluate_turn_redundant_tools(&tools, &messages, false).is_allow());
        // 已确认（早停门写入）：同一份证据下同步门放行。
        assert!(evaluate_turn_redundant_tools(&tools, &messages, true).is_allow());
    }

    /// #1415：judge 的输入证据必须与第 ① 层**同一窗口**。
    ///
    /// 否则多轮会话里上一任务跑通的 `cargo_test` 会进入本轮判定输入，而锚点校验只要求
    /// 「有一条 verify/write 成功」、不问它属不属于本窗口 → judge 会拿旧证据判当前任务完成。
    #[test]
    fn judge_evidence_window_excludes_previous_task() {
        let messages = vec![
            msg("user", "上一轮：跑通全部测试"),
            tool_env(
                "cargo_test",
                "cargo test",
                "退出码：0\n标准输出：\ntest result: ok. 1 passed",
            ),
            msg("assistant", "上一轮已完成。"),
            msg("user", "本轮：分析目录结构"),
            tool_env("list_tree", "list tree", "list tree: ."),
        ];
        let window = crate::cm_types::messages_slice_since_last_real_user(&messages, false)
            .expect("应能从最后一条真实 user 切开");
        let ev = EvidenceSet::collect(window);
        assert_eq!(ev.items().len(), 1, "只应采集本轮证据");
        assert!(
            ev.items()
                .iter()
                .all(|i| i.kind != crate::agent::agent_turn::policy::task_level_evidence::EvidenceKind::Verify),
            "上一任务的 cargo_test 不得进入本轮判定输入"
        );
    }

    #[test]
    fn injects_when_tools_succeeded_but_assistant_empty() {
        let msgs = vec![
            Message::user_only("编译 hello"),
            tool_env(
                "run_command",
                "make",
                "命令：make\n退出码：0\n标准输出：\nBuilt target hello",
            ),
            Message::assistant_only(""),
        ];
        let fb = outer_loop_missing_final_answer_feedback_if_needed(&msgs, &msgs[2], 0);
        assert!(fb.is_some());
    }

    #[test]
    fn injects_truncated_feedback_when_truncated_within_budget() {
        let fb = outer_loop_truncated_feedback_if_needed(true, 0);
        let fb = fb.expect("截断且预算内应注入纠偏");
        assert!(fb.contains("截断"));
        assert!(fb.contains(OUTER_LOOP_ORCHESTRATION_FEEDBACK_PREFIX));
    }

    #[test]
    fn skips_truncated_feedback_when_not_truncated_or_budget_exhausted() {
        assert!(outer_loop_truncated_feedback_if_needed(false, 0).is_none());
        assert!(
            outer_loop_truncated_feedback_if_needed(true, OUTER_LOOP_TRUNCATED_FEEDBACK_MAX)
                .is_none()
        );
    }

    /// #1478：`退出码：01` 以 `退出码：0` 开头，旧实现 `contains("退出码：0")` 会判成成功。
    #[test]
    fn successful_tool_ignores_nonzero_exit_code_with_zero_prefix() {
        let messages = vec![
            msg("user", "跑一下测试"),
            msg("tool", "退出码：01\n标准输出：\nfailed"),
        ];
        assert!(
            !outer_loop_window_has_any_successful_tool(&messages),
            "退出码 1 不得判成功"
        );
    }

    /// 英文等价形态：`exit code: 01` 同样不得判成功。
    #[test]
    fn successful_tool_ignores_nonzero_exit_code_with_zero_prefix_en() {
        let messages = vec![
            msg("user", "run the tests"),
            msg("tool", "exit code: 01\nstdout:\nfailed"),
        ];
        assert!(
            !outer_loop_window_has_any_successful_tool(&messages),
            "exit code 1 不得判成功"
        );
    }

    /// 真 0 仍要认，防止判据被改成永假。
    #[test]
    fn successful_tool_detects_real_zero_exit_code() {
        let messages = vec![
            msg("user", "跑一下测试"),
            msg("tool", "退出码：0\n标准输出：\nok"),
        ];
        assert!(outer_loop_window_has_any_successful_tool(&messages));
    }

    /// #1478 已知残余（pin 现状，不是期望行为）：说明性文本里的零退出码仍会被采信。
    ///
    /// 判据是「文本里出现过零退出码」而非「本条结果自己的退出码」。这条测试挡住两种误改：
    /// 把 helper 换回裸 `contains` 不会变红因而无人察觉；以及有人改严后忘了同步这里记录的
    /// 已知边界。真正的收敛等结构化 `error_code` 覆盖该路径。
    #[test]
    fn zero_exit_code_in_prose_is_a_known_false_positive() {
        assert!(text_reports_zero_exit_code(
            "cargo: expected exit code: 0, got 1"
        ));
    }

    /// 标签表只认全角冒号与带空格的英文形态；半角 `退出码: 0` 不识别（已知边界）。
    #[test]
    fn half_width_colon_is_not_recognized() {
        assert!(!text_reports_zero_exit_code("退出码: 0"));
    }

    /// 数字按数值比较：`01` / `10` 拒绝，`00` 语义即 0。
    #[test]
    fn exit_code_digits_are_compared_numerically() {
        assert!(!text_reports_zero_exit_code("退出码：01"));
        assert!(!text_reports_zero_exit_code("退出码：10"));
        assert!(text_reports_zero_exit_code("退出码：00"));
    }

    /// 同一段里既有 0 又有非 0：取「任一为 0」（已知边界，pin 现状）。
    #[test]
    fn any_zero_in_text_wins_over_nonzero() {
        assert!(text_reports_zero_exit_code("退出码：1\n退出码：0"));
    }
}
