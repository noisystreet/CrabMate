//! 回合结束 `sync` 时合并列表：Agent 侧 `messages` 经 `prepare_messages_for_model` 可能删掉较早 user/assistant，
//! 若直接替换 TUI `state.messages`，聊天框会像「旧消息全没了」；此处保留 UI 上被裁掉的前缀，再接上 Agent 尾部（含 `tool` 行）。
//!
//! TUI 仅经 SSE 文本 delta 更新 assistant **正文**，通常**没有** `tool_calls`；Agent 侧同一条 assistant 可能已带 `tool_calls`，
//! 若用 `Message` 全量 `==` 做后缀对齐会永远对不上，合并失败后又整表替换为裁剪后的 `fin`，表现为旧消息全丢。

use crate::types::Message;

fn non_tool_enumerate(msgs: &[Message]) -> Vec<(usize, &Message)> {
    msgs.iter()
        .enumerate()
        .filter(|(_, m)| m.role != "tool")
        .collect()
}

/// TUI 侧缺 `tool_calls` 时仍视为与 Agent 同一条 assistant 对齐。
fn assistant_tool_calls_align_for_merge(ui: &Message, fin: &Message) -> bool {
    match (&ui.tool_calls, &fin.tool_calls) {
        (Some(a), Some(b)) => a == b,
        (None, Some(_)) => true,
        (None, None) => true,
        (Some(_), None) => false,
    }
}

fn assistant_content_align(ui: Option<&str>, fin: Option<&str>, is_last: bool) -> bool {
    match (ui, fin) {
        (Some(cu), Some(cf)) => {
            let cu = cu.trim_end();
            let cf = cf.trim_end();
            if is_last {
                cf.starts_with(cu) || cu == cf || cu.is_empty()
            } else {
                cf.starts_with(cu) || cu.starts_with(cf) || cu == cf
            }
        }
        (None, Some(cf)) => !cf.trim().is_empty(),
        (None, None) => true,
        (Some(cu), None) => cu.trim().is_empty(),
    }
}

/// 非 tool 消息在后缀对齐时是否「同一条」：assistant 放宽正文与 tool_calls。
fn non_tool_pair_aligns(ui: &Message, fin: &Message, is_last: bool) -> bool {
    if ui.role != fin.role {
        return false;
    }
    if ui.role == "assistant" {
        if !assistant_tool_calls_align_for_merge(ui, fin) {
            return false;
        }
        return assistant_content_align(ui.content.as_deref(), fin.content.as_deref(), is_last);
    }
    ui == fin
}

fn non_tool_tail_matches(ui_tail: &[(usize, &Message)], fin_tail: &[(usize, &Message)]) -> bool {
    if ui_tail.len() != fin_tail.len() {
        return false;
    }
    let n = ui_tail.len();
    for i in 0..n {
        let (_, a) = ui_tail[i];
        let (_, b) = fin_tail[i];
        if !non_tool_pair_aligns(a, b, i + 1 == n) {
            return false;
        }
    }
    true
}

/// 去掉**相邻**且 `content` 与 `tool_calls` 完全相同的助手（无 `tool_calls`）。
/// 分阶段规划下全量同步后，滞后 SSE 可能再推一条与规划轮全文相同的助手，表现为双开场白；Agent 侧仅一条。
fn dedupe_adjacent_identical_assistants(msgs: Vec<Message>) -> Vec<Message> {
    let mut out: Vec<Message> = Vec::with_capacity(msgs.len());
    for m in msgs {
        let dup = m.role == "assistant"
            && m.tool_calls.is_none()
            && out.last().is_some_and(|p| {
                p.role == "assistant" && p.tool_calls.is_none() && p.content == m.content
            });
        if dup {
            continue;
        }
        out.push(m);
    }
    out
}

/// `ui`：回合过程中 TUI 侧流式积累的完整气泡；`fin`：Agent 返回的 `messages`（可能更短、含 `tool`）。
pub(super) fn merge_tui_messages_after_agent_sync(
    ui: Vec<Message>,
    fin: Vec<Message>,
) -> Vec<Message> {
    let merged = if fin.is_empty() {
        ui
    } else if let Some(m) = merge_by_non_tool_suffix(&ui, &fin) {
        m
    } else {
        fin
    };
    dedupe_adjacent_identical_assistants(merged)
}

fn merge_by_non_tool_suffix(ui: &[Message], fin: &[Message]) -> Option<Vec<Message>> {
    let ui_nt = non_tool_enumerate(ui);
    let fin_nt = non_tool_enumerate(fin);
    let max_k = ui_nt.len().min(fin_nt.len());
    for k in (1..=max_k).rev() {
        let ui_tail = &ui_nt[ui_nt.len() - k..];
        let fin_tail = &fin_nt[fin_nt.len() - k..];
        if !non_tool_tail_matches(ui_tail, fin_tail) {
            continue;
        }
        let ui_start_idx = ui_tail[0].0;
        let fin_start_idx = fin_tail[0].0;
        let mut out = Vec::with_capacity(ui_start_idx + fin.len().saturating_sub(fin_start_idx));
        out.extend_from_slice(&ui[..ui_start_idx]);
        out.extend_from_slice(&fin[fin_start_idx..]);
        return Some(out);
    }
    if fin.len() > ui.len() {
        let extra_count = fin.len() - ui.len();
        let fin_extra_start = fin.len() - extra_count;
        let mut out = Vec::with_capacity(fin.len());
        out.extend_from_slice(ui);
        for item in fin.iter().skip(fin_extra_start) {
            out.push(item.clone());
        }
        return Some(out);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{FunctionCall, Message, ToolCall};

    fn u(s: &str) -> Message {
        Message::user_only(s.to_string())
    }
    fn a(s: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(s.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    fn assistant_with_tools(content: &str) -> Message {
        Message {
            role: "assistant".to_string(),
            content: Some(content.to_string()),
            tool_calls: Some(vec![ToolCall {
                id: "c1".to_string(),
                typ: "function".to_string(),
                function: FunctionCall {
                    name: "run_command".to_string(),
                    arguments: "{}".to_string(),
                },
            }]),
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn merge_keeps_prefix_when_agent_trimmed_middle() {
        let ui = vec![
            Message::system_only("sys"),
            u("old"),
            a("a1"),
            u("new"),
            a("tail"),
        ];
        let fin = vec![Message::system_only("sys"), u("new"), a("tail")];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 5);
        assert_eq!(m[1].content.as_deref(), Some("old"));
        assert_eq!(m[4].content.as_deref(), Some("tail"));
    }

    #[test]
    fn merge_prefers_fin_when_fin_longer() {
        let ui = vec![Message::system_only("s"), u("x")];
        let fin = vec![Message::system_only("s"), u("x"), a("y")];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 3);
    }

    #[test]
    fn merge_equal_len_when_ui_lacks_tool_calls_on_assistant() {
        let ui = vec![
            Message::system_only("sys"),
            u("q"),
            a("calling"),
            a("result"),
        ];
        let fin = vec![
            Message::system_only("sys"),
            u("q"),
            assistant_with_tools("calling"),
            a("result"),
        ];
        assert_eq!(ui.len(), fin.len());
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 4);
        assert!(m[2].tool_calls.is_some());
        assert_eq!(m[2].content.as_deref(), Some("calling"));
    }

    #[test]
    fn merge_skips_tools_in_fin_for_suffix_alignment() {
        let ui = vec![
            Message::system_only("sys"),
            u("q"),
            a("plan"),
            u("step"),
            a("done"),
        ];
        let tool = Message {
            role: "tool".to_string(),
            content: Some(r#"{"ok":true}"#.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: Some("call-1".to_string()),
        };
        let fin = vec![Message::system_only("sys"), u("step"), tool, a("done")];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 6);
        assert_eq!(m[1].content.as_deref(), Some("q"));
        assert!(m.iter().any(|x| x.role == "tool"));
    }

    #[test]
    fn merge_tolerant_last_assistant_placeholder() {
        let ui = vec![
            Message::system_only("sys"),
            u("q"),
            Message {
                role: "assistant".to_string(),
                content: Some(String::new()),
                tool_calls: None,
                name: None,
                tool_call_id: None,
            },
        ];
        let fin = vec![Message::system_only("sys"), u("q"), a("full reply")];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 3);
        assert_eq!(m[2].content.as_deref(), Some("full reply"));
    }

    #[test]
    fn dedupe_drops_consecutive_identical_assistant_bodies() {
        let ui = vec![Message::system_only("sys"), u("q"), a("same"), a("same")];
        let fin = vec![Message::system_only("sys"), u("q"), a("same")];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(
            m.iter().filter(|x| x.role == "assistant").count(),
            1,
            "{m:?}"
        );
    }

    #[test]
    fn merge_staged_plan_preserves_streaming_assistant() {
        let ui = vec![
            Message::system_only("sys"),
            u("q"),
            a("plan"),
            u("step user content"),
            a("streaming..."),
        ];
        let tool = Message {
            role: "tool".to_string(),
            content: Some(r#"{"ok":true}"#.to_string()),
            tool_calls: None,
            name: None,
            tool_call_id: Some("call-1".to_string()),
        };
        let fin = vec![
            Message::system_only("sys"),
            u("q"),
            a("plan"),
            u("step user content"),
            a("done"),
            tool,
        ];
        let m = merge_tui_messages_after_agent_sync(ui, fin);
        assert_eq!(m.len(), 6);
        assert_eq!(m[4].content.as_deref(), Some("streaming..."));
        assert!(m.last().is_some_and(|msg| msg.role == "tool"));
    }
}
