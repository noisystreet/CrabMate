//! 将连续「分阶段时间线」`system` 消息聚合为待办式步骤列表（解析 `cm_tl` 与旧式前缀旁注）。

use std::collections::BTreeMap;

use crate::i18n::Locale;
use crate::message_format::message_text_for_display_ex;
use crate::storage::StoredMessage;
use crate::timeline_scan::{
    TimelineEntry, TimelineKind, timeline_entry_for_message, timeline_entry_is_failed,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StagedStepPhase {
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

/// 从一组连续分阶段时间线消息生成有序步骤行；`legacy_lines` 为无 `cm_tl` 的旧式旁注纯文本。
pub(crate) fn build_staged_plan_todo_steps(
    locale: Locale,
    apply_filters: bool,
    items: &[(usize, StoredMessage)],
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

    let steps = by_step
        .into_iter()
        .map(
            |(
                step_index,
                StepAcc {
                    total_steps: _,
                    title,
                    phase,
                    anchor_message_id,
                },
            )| StagedPlanTodoStep {
                ordinal: step_index.max(1),
                title,
                phase,
                anchor_message_id,
            },
        )
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
