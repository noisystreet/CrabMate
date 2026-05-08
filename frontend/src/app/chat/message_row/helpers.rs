//! 消息行样式与分层子目标（hierarchical subgoal）相关的纯辅助逻辑。

use crate::i18n::{self, Locale};
use crate::storage::{ChatSession, StoredMessage, StoredMessageState};

pub(super) fn stored_message_by_id<'a>(
    sessions: &'a [ChatSession],
    active_session_id: &str,
    message_id: &str,
) -> Option<&'a StoredMessage> {
    sessions
        .iter()
        .find(|s| s.id == active_session_id)
        .and_then(|s| s.messages.iter().find(|m| m.id == message_id))
}

pub(super) fn is_hierarchical_subgoal_state(state: Option<&StoredMessageState>) -> bool {
    state.is_some_and(|s| s.looks_like_hierarchical_subgoal())
}

pub(super) fn tool_bubble_emoji(m: &StoredMessage) -> &'static str {
    let name = m
        .tool_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            m.reasoning_text.lines().next().and_then(|line| {
                line.trim()
                    .strip_prefix("tool:")
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
            })
        });
    name.map(i18n::tool_kind_emoji).unwrap_or("🔧")
}

pub(super) fn extract_hierarchical_phase_chip(
    msg: &StoredMessage,
    loc: Locale,
) -> Option<(String, String)> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    i18n::hierarchical_phase_chip_view(loc, msg.text.as_str())
}

pub(super) fn extract_hierarchical_metrics(msg: &StoredMessage, loc: Locale) -> Option<String> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    let mut error_count: Option<String> = None;
    let mut stagnant_rounds: Option<String> = None;
    for line in msg.text.lines().map(str::trim) {
        if error_count.is_none()
            && let Some(v) = i18n::hierarchical_error_count_raw(line)
        {
            let v = v.trim();
            if !v.is_empty() {
                error_count = Some(v.to_string());
            }
        }
        if stagnant_rounds.is_none()
            && let Some(v) = i18n::hierarchical_stagnant_rounds_raw(line)
        {
            let v = v.trim();
            if !v.is_empty() {
                stagnant_rounds = Some(v.to_string());
            }
        }
    }
    i18n::hierarchical_metrics_line(loc, error_count.as_deref(), stagnant_rounds.as_deref())
}

pub(super) fn extract_hierarchical_goal_target(msg: &StoredMessage) -> Option<String> {
    if !is_hierarchical_subgoal_state(msg.state.as_ref()) {
        return None;
    }
    msg.text.lines().map(str::trim).find_map(|line| {
        i18n::hierarchical_goal_target_raw(line)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

pub(super) fn build_subgoal_exec_banner_text(
    loc: Locale,
    phase: Option<&str>,
    target: Option<&str>,
) -> Option<String> {
    let key = i18n::hierarchical_subgoal_phase_key(phase)?;
    let verb = i18n::hierarchical_subgoal_exec_verb(loc, key);
    if verb.is_empty() {
        return None;
    }
    let suffix = i18n::hierarchical_subgoal_running_suffix(loc);
    match (loc, target.filter(|t| !t.trim().is_empty())) {
        (Locale::ZhHans, Some(t)) => Some(format!("{verb}：{}…", t.trim())),
        (Locale::ZhHans, None) => Some(format!("{verb}{suffix}")),
        (Locale::En, Some(t)) => Some(format!("{verb}: {}…", t.trim())),
        (Locale::En, None) => Some(format!("{verb} {suffix}")),
    }
}

pub(super) fn build_subgoal_exec_banner_icon_key(
    _loc: Locale,
    phase: Option<&str>,
) -> Option<&'static str> {
    i18n::hierarchical_subgoal_phase_key(phase)
}

pub(super) fn is_running_subgoal_phase(loc: Locale, phase: Option<&str>) -> bool {
    let _ = loc;
    i18n::hierarchical_subgoal_phase_key(phase).is_some()
}

pub(super) fn message_row_shell_class(is_staged_timeline: bool, m: &StoredMessage) -> &'static str {
    if is_staged_timeline {
        "msg msg-staged-timeline"
    } else {
        match m.role.as_str() {
            "user" => "msg msg-user",
            "assistant" if m.is_tool => "msg msg-tool",
            "assistant" => "msg msg-assistant",
            _ if m.is_tool => "msg msg-tool",
            _ => "msg msg-system",
        }
    }
}

pub(super) fn message_row_loading_and_error(
    is_tool: bool,
    role: &str,
    state: Option<&StoredMessageState>,
) -> (bool, bool) {
    let loading = state.is_some_and(|s| s.is_loading()) && (role == "assistant" || is_tool);
    let err = state.is_some_and(|s| s.is_error());
    (loading, err)
}

pub(super) fn message_row_prefixed_class(cls: &str, err: bool, loading: bool) -> String {
    if err {
        format!("{cls} msg-error")
    } else if loading {
        format!("{cls} msg-loading")
    } else {
        cls.to_string()
    }
}

pub(super) fn hierarchical_subgoal_banner_is_active(
    sessions: &[ChatSession],
    active_session_id: &str,
    current_msg_id: &str,
    subgoal_exec_banner: Option<&String>,
    phase_for_run_check: Option<&str>,
    loc: Locale,
) -> bool {
    if subgoal_exec_banner.is_none() || !is_running_subgoal_phase(loc, phase_for_run_check) {
        return false;
    }
    sessions
        .iter()
        .find(|s| s.id == active_session_id)
        .and_then(|sess| {
            sess.messages
                .iter()
                .rev()
                .find(|msg| is_hierarchical_subgoal_state(msg.state.as_ref()))
        })
        .map(|msg| msg.id == current_msg_id)
        .unwrap_or(false)
}
