//! 将连续「分阶段时间线」`system` 消息聚合为待办式步骤列表（解析 `cm_tl` 与旧式前缀旁注）。
//! 若会话中本簇之前有「规划轮」`agent_reply_plan`，则**预先展开全部步骤**；尚未有时间线事件的步骤为 `Pending`，仅随完成态更新勾选。

use std::collections::{BTreeMap, BTreeSet};

use crate::i18n::Locale;
use crate::message_format::{
    agent_reply_plan_step_descriptions_from_assistant, message_text_for_display_ex,
};
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    TimelineEntry, TimelineKind, timeline_entry_for_message, timeline_entry_is_failed,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StagedStepPhase {
    /// 规划 JSON 已列出该步，尚无 `staged_plan_step_*` 时间线事件。
    Pending,
    InProgress,
    Done,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug)]
pub(crate) struct StagedPlanTodoStep {
    /// 与 SSE `step_index` 一致，**从 1 起**；用于列表行 `「{ordinal}. …」`。
    pub ordinal: usize,
    /// 已去掉前导 `^\d+\.\s*`，避免与 `ordinal` 重复拼接。
    pub title: String,
    pub phase: StagedStepPhase,
    pub anchor_message_id: String,
}

/// 去掉旁注正文前导的 `「12. 」` 式步骤号，供聚合列表统一加序号。
pub(crate) fn strip_leading_step_ordinal(s: &str) -> &str {
    let s = s.trim_start();
    let mut end_digits = 0usize;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() {
            end_digits = i + c.len_utf8();
        } else {
            break;
        }
    }
    if end_digits == 0 {
        return s;
    }
    let after = &s[end_digits..];
    let after = match after.strip_prefix('.') {
        Some(r) => r,
        None => return s,
    };
    after.trim_start()
}

fn phase_from_status(status: &str) -> StagedStepPhase {
    let t = status.trim();
    let end_kind = TimelineKind::StagedEnd {
        step_index: 0,
        total_steps: 0,
        status: t.to_string(),
    };
    match t {
        "ok" => StagedStepPhase::Done,
        "cancelled" => StagedStepPhase::Cancelled,
        _ if timeline_entry_is_failed(&end_kind) => StagedStepPhase::Failed,
        _ => StagedStepPhase::Done,
    }
}

struct StepAcc {
    total_steps: usize,
    title: String,
    phase: StagedStepPhase,
    anchor_message_id: String,
}

/// 在本簇首条时间线消息**之前**的会话中，回溯最近一条含可解析 `agent_reply_plan.steps` 的助手消息。
pub(crate) fn plan_step_prefill_from_session(
    session: &[StoredMessage],
    before_msg_idx: usize,
) -> Option<Vec<String>> {
    for i in (0..before_msg_idx).rev() {
        if let Some(v) = agent_reply_plan_step_descriptions_from_assistant(&session[i]) {
            if !v.is_empty() {
                return Some(v);
            }
        }
    }
    None
}

/// 从一组连续分阶段时间线消息生成有序步骤行；`legacy_lines` 为无 `cm_tl` 的旧式旁注纯文本。
pub(crate) fn build_staged_plan_todo_steps(
    locale: Locale,
    apply_filters: bool,
    items: &[(usize, StoredMessage)],
    session_messages: &[StoredMessage],
) -> (Vec<StagedPlanTodoStep>, Vec<String>) {
    let mut by_step: BTreeMap<usize, StepAcc> = BTreeMap::new();
    let mut legacy_lines: Vec<String> = Vec::new();

    for (_idx, m) in items {
        let disp = message_text_for_display_ex(m, locale, apply_filters);
        let disp_norm = strip_leading_step_ordinal(&disp).to_string();
        let Some(TimelineEntry { message_id, kind }) = timeline_entry_for_message(m) else {
            legacy_lines.push(disp);
            continue;
        };

        match kind {
            TimelineKind::StagedStart {
                step_index,
                total_steps,
            } => {
                by_step
                    .entry(step_index)
                    .and_modify(|e| {
                        e.total_steps = e.total_steps.max(total_steps);
                        if e.title.is_empty() {
                            e.title = disp_norm.clone();
                        }
                        if !matches!(
                            e.phase,
                            StagedStepPhase::Done
                                | StagedStepPhase::Failed
                                | StagedStepPhase::Cancelled
                        ) {
                            e.phase = StagedStepPhase::InProgress;
                        }
                    })
                    .or_insert(StepAcc {
                        total_steps,
                        title: disp_norm,
                        phase: StagedStepPhase::InProgress,
                        anchor_message_id: message_id,
                    });
            }
            TimelineKind::StagedEnd {
                step_index,
                total_steps,
                status,
            } => {
                let ph = phase_from_status(&status);
                by_step
                    .entry(step_index)
                    .and_modify(|e| {
                        e.total_steps = e.total_steps.max(total_steps);
                        e.phase = ph;
                        if e.title.is_empty() {
                            e.title = disp_norm.clone();
                        }
                    })
                    .or_insert(StepAcc {
                        total_steps,
                        title: disp_norm,
                        phase: ph,
                        anchor_message_id: message_id,
                    });
            }
            TimelineKind::LegacyStaged => {
                legacy_lines.push(disp);
            }
            _ => {
                legacy_lines.push(disp);
            }
        }
    }

    let before_idx = items.first().map(|(i, _)| *i).unwrap_or(0);
    let prefill = plan_step_prefill_from_session(session_messages, before_idx);
    let head_anchor = items.first().map(|(_, m)| m.id.clone()).unwrap_or_default();

    if by_step.is_empty() && prefill.is_none() {
        return (Vec::new(), legacy_lines);
    }

    let mut ord_set: BTreeSet<usize> = by_step.keys().copied().collect();
    let n_pref = prefill.as_ref().map(|p| p.len()).unwrap_or(0);
    for i in 1..=n_pref {
        ord_set.insert(i);
    }
    let max_total = by_step.values().map(|e| e.total_steps).max().unwrap_or(0);
    let max_seen = ord_set.iter().next_back().copied().unwrap_or(0);
    let cap = n_pref.max(max_seen).max(max_total);
    for k in 1..=cap {
        ord_set.insert(k);
    }

    let steps: Vec<StagedPlanTodoStep> = ord_set
        .into_iter()
        .map(|ord| {
            let acc = by_step.get(&ord);
            let from_plan = prefill.as_ref().and_then(|p| p.get(ord.saturating_sub(1)));

            // 规划 JSON 中的 `description` 作稳定标题；时间线旁注仅作补全（避免「start」覆盖「步一」）。
            let mut title = from_plan.cloned().unwrap_or_default();
            if title.is_empty() {
                title = acc
                    .filter(|e| !e.title.is_empty())
                    .map(|e| e.title.clone())
                    .unwrap_or_default();
            }
            if title.is_empty() {
                title = crate::i18n::plan_step_no_desc(locale).to_string();
            }

            let phase = acc.map(|e| e.phase).unwrap_or(StagedStepPhase::Pending);

            let anchor_message_id = acc
                .map(|e| e.anchor_message_id.clone())
                .unwrap_or_else(|| head_anchor.clone());

            StagedPlanTodoStep {
                ordinal: ord.max(1),
                title,
                phase,
                anchor_message_id,
            }
        })
        .collect();

    (steps, legacy_lines)
}

#[cfg(test)]
mod strip_tests {
    use super::strip_leading_step_ordinal;

    #[test]
    fn strip_removes_simple_ordinal() {
        assert_eq!(strip_leading_step_ordinal("1. hello"), "hello");
        assert_eq!(strip_leading_step_ordinal("12.  x"), "x");
    }

    #[test]
    fn strip_no_match_leaves_string() {
        assert_eq!(strip_leading_step_ordinal("no digit"), "no digit");
        assert_eq!(strip_leading_step_ordinal("1x. y"), "1x. y");
    }
}

#[cfg(test)]
mod merge_tests {
    use super::StagedStepPhase;
    use super::build_staged_plan_todo_steps;
    use crate::i18n::Locale;
    use crate::message_format::staged_timeline_system_message_body;
    use crate::storage::StoredMessage;
    use crate::timeline_scan::{timeline_state_staged_end, timeline_state_staged_start};

    fn assistant_with_plan(id: &str, json: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: format!("```json\n{json}\n```"),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool: false,
            created_at: 0,
        }
    }

    fn staged_start(id: &str, step: usize, total: usize, body: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "system".into(),
            text: staged_timeline_system_message_body(body),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_staged_start(id, step, total)),
            is_tool: false,
            created_at: 1,
        }
    }

    fn staged_end(id: &str, step: usize, total: usize, status: &str) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "system".into(),
            text: staged_timeline_system_message_body(&format!("{step}. {status}")),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: Some(timeline_state_staged_end(id, step, total, status)),
            is_tool: false,
            created_at: 2,
        }
    }

    #[test]
    fn prefill_shows_all_steps_before_timeline() {
        let plan = r#"{"type":"agent_reply_plan","version":1,"no_task":false,"steps":[{"id":"a","description":"步一"},{"id":"b","description":"步二"}]}"#;
        let session = vec![
            assistant_with_plan("a0", plan),
            staged_start("s1", 1, 2, "1. start"),
        ];
        let items = vec![(1usize, session[1].clone())];
        let (steps, _) = build_staged_plan_todo_steps(Locale::ZhHans, true, &items, &session);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].ordinal, 1);
        assert_eq!(steps[0].title, "步一");
        assert_eq!(steps[0].phase, StagedStepPhase::InProgress);
        assert_eq!(steps[1].phase, StagedStepPhase::Pending);
    }

    #[test]
    fn timeline_done_updates_check() {
        let plan = r#"{"type":"agent_reply_plan","version":1,"no_task":false,"steps":[{"id":"a","description":"A"},{"id":"b","description":"B"}]}"#;
        let session = vec![
            assistant_with_plan("a0", plan),
            staged_start("s1", 1, 2, "1. x"),
            staged_end("s2", 1, 2, "ok"),
            staged_start("s3", 2, 2, "2. y"),
        ];
        let items: Vec<_> = (1..4).map(|i| (i, session[i].clone())).collect();
        let (steps, _) = build_staged_plan_todo_steps(Locale::ZhHans, true, &items, &session);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].phase, StagedStepPhase::Done);
        assert_eq!(steps[1].phase, StagedStepPhase::InProgress);
    }
}
