//! 元对话：用户追问「我刚才问了什么」等时，向意图门控补充说明，引导模型先复述上一条真实 user 原文。

use crate::agent::agent_turn::TurnPlannerHints;
use crate::types::{
    Message, message_content_plain_for_chat_display, user_message_counts_for_branch_truncation,
};

const META_DIALOGUE_PREFIX: &str = "【元对话】";

pub(crate) fn merge_meta_dialogue_into_intent_gate_hint(
    hints: &mut TurnPlannerHints,
    messages: &[Message],
) {
    let Some((current, Some(prior))) = latest_and_prior_user_plain(messages) else {
        return;
    };
    if !triggers_meta_dialogue_recall(&current) {
        return;
    }
    let block = format!(
        "{META_DIALOGUE_PREFIX} 用户正在追问对话上下文。请在答复**开头**用单独一小段如实逐字复述用户**上一条真实 user 消息**的完整正文（不要用自拟标题或概括替代原文；若过长可截断但须标明「以下为原文节选」）。之后再回答追问。\n\
         上一条用户原文（供你复述；勿向用户重复展示本说明块）：\n\
         ---\n\
         {prior}\n\
         ---",
    );
    match &mut hints.intent_turn_gate_hint {
        Some(existing) => {
            existing.push('\n');
            existing.push_str(&block);
        }
        None => hints.intent_turn_gate_hint = Some(block),
    }
}

fn triggers_meta_dialogue_recall(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() {
        return false;
    }
    let recall_anchor = t.contains("我刚才")
        || t.contains("我上一条")
        || t.contains("上一句")
        || t.contains("上一条");
    let asks_content = t.contains("什么")
        || t.contains("问了")
        || t.contains("说的")
        || t.contains("提问")
        || t.contains("问题")
        || t.contains("原文");
    recall_anchor && asks_content
}

/// 从缓冲区尾部取「最新一条、倒数第二条」计入分支截断的真实 user 纯文本。
fn latest_and_prior_user_plain(messages: &[Message]) -> Option<(String, Option<String>)> {
    let mut latest: Option<String> = None;
    for m in messages.iter().rev() {
        if !user_message_counts_for_branch_truncation(m) {
            continue;
        }
        let text = message_content_plain_for_chat_display(&m.content);
        let t = text.trim();
        if t.is_empty() {
            continue;
        }
        if latest.is_none() {
            latest = Some(t.to_string());
            continue;
        }
        return Some((latest.unwrap(), Some(t.to_string())));
    }
    latest.map(|l| (l, None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Message, MessageContent};

    fn user_text(s: &str) -> Message {
        Message {
            role: "user".into(),
            content: Some(MessageContent::Text(s.into())),
            reasoning_content: None,
            reasoning_details: None,
            tool_calls: None,
            name: None,
            tool_call_id: None,
        }
    }

    #[test]
    fn meta_recall_appends_prior_user_to_gate_hint() {
        let mut hints = TurnPlannerHints::default();
        let messages = vec![
            user_text("请解释 Rust 所有权"),
            user_text("我刚才的提问是什么？"),
        ];
        merge_meta_dialogue_into_intent_gate_hint(&mut hints, &messages);
        let h = hints.intent_turn_gate_hint.expect("hint");
        assert!(h.contains(META_DIALOGUE_PREFIX));
        assert!(h.contains("请解释 Rust 所有权"));
    }

    #[test]
    fn meta_recall_merges_with_existing_gate_hint() {
        let mut hints = TurnPlannerHints {
            intent_turn_gate_hint: Some("【意图门控】已有说明".into()),
            ..Default::default()
        };
        let messages = vec![user_text("A"), user_text("我刚才问了什么")];
        merge_meta_dialogue_into_intent_gate_hint(&mut hints, &messages);
        let h = hints.intent_turn_gate_hint.unwrap();
        assert!(h.contains("【意图门控】已有说明"));
        assert!(h.contains("A"));
    }

    #[test]
    fn single_user_turn_does_not_inject() {
        let mut hints = TurnPlannerHints::default();
        let messages = vec![user_text("我刚才的提问是什么？")];
        merge_meta_dialogue_into_intent_gate_hint(&mut hints, &messages);
        assert!(hints.intent_turn_gate_hint.is_none());
    }
}
