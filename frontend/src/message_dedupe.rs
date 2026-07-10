//! 助手正文 fuzzy 比较（normalize 空白后比较）；Phase 7 P1 起写/读路径不再全表 dedupe，本模块供 snapshot 判定与单测保留。

use crate::message_loading::is_finalized_plain_assistant;
use crate::storage::StoredMessage;
use crate::timeline_scan::timeline_ui_snapshot_type;

/// 折叠空白便于比较排版略有差异的重复终答/旁注。
#[must_use]
pub fn normalize_assistant_text_for_dedupe(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 两段助手正文是否语义重复（相等，或较长段包含较短段且短段足够长）。
#[must_use]
pub fn assistant_texts_fuzzy_duplicate(a: &str, b: &str) -> bool {
    let a = normalize_assistant_text_for_dedupe(a.trim());
    let b = normalize_assistant_text_for_dedupe(b.trim());
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a == b {
        return true;
    }
    let (long, short) = if a.len() >= b.len() {
        (a.as_str(), b.as_str())
    } else {
        (b.as_str(), a.as_str())
    };
    short.len() > 40 && long.contains(short)
}

fn is_dedupe_candidate_assistant(m: &StoredMessage) -> bool {
    is_finalized_plain_assistant(m)
}

#[allow(dead_code)]
fn is_ephemeral_final_response_snapshot(m: &StoredMessage) -> bool {
    m.state
        .as_ref()
        .and_then(timeline_ui_snapshot_type)
        .is_some_and(|t| t == "final_response_snapshot")
}

/// 自最后一条 `user` 起，删除 fuzzy 重复的 assistant 行（保留首次出现）。
///
/// Phase 7 P1：已从 `on_done` / 读路径退役；保留供本模块单测回归。
#[allow(dead_code)]
pub fn dedupe_assistant_messages_since_last_user(messages: &mut Vec<StoredMessage>) {
    let Some(last_user) = messages.iter().rposition(|m| m.role == "user") else {
        return;
    };
    let mut kept_bodies: Vec<String> = Vec::new();
    let mut i = last_user + 1;
    while i < messages.len() {
        if !is_dedupe_candidate_assistant(&messages[i]) {
            i += 1;
            continue;
        }
        let body = messages[i].text.clone();
        if is_ephemeral_final_response_snapshot(&messages[i])
            && kept_bodies
                .iter()
                .any(|prior| assistant_texts_fuzzy_duplicate(prior, body.as_str()))
        {
            messages.remove(i);
            continue;
        }
        if kept_bodies
            .iter()
            .any(|prior| assistant_texts_fuzzy_duplicate(prior, body.as_str()))
        {
            messages.remove(i);
            continue;
        }
        kept_bodies.push(body);
        i += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    #[test]
    fn normalize_collapses_whitespace() {
        assert_eq!(normalize_assistant_text_for_dedupe("a\n\nb  c"), "a b c");
    }

    #[test]
    fn fuzzy_duplicate_detects_compact_vs_expanded() {
        let expanded = "当前目录下有三个压缩包：\n\n1. **A** — x\n\n2. **B** — y";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x\n2. **B** — y";
        assert!(assistant_texts_fuzzy_duplicate(expanded, compact));
    }

    #[test]
    fn dedupe_since_last_user_keeps_first_listing() {
        let listing = "当前目录下有三个压缩包：\n\n1. **A** — x\n\n2. **B** — y\n\n3. **C** — z";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x\n2. **B** — y\n3. **C** — z";
        let mut msgs = vec![
            StoredMessage {
                id: "u".into(),
                role: "user".into(),
                text: "分析".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "a1".into(),
                role: "assistant".into(),
                text: listing.into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "a2".into(),
                role: "assistant".into(),
                text: "好的，我来看看。".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "a3".into(),
                role: "assistant".into(),
                text: compact.into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        dedupe_assistant_messages_since_last_user(&mut msgs);
        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[1].id, "a1");
        assert_eq!(msgs[2].id, "a2");
    }

    #[test]
    fn skips_loading_assistant_rows() {
        let body = "重复的长正文内容。".repeat(8);
        let mut msgs = vec![
            StoredMessage {
                id: "u".into(),
                role: "user".into(),
                text: "q".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "a1".into(),
                role: "assistant".into(),
                text: body.clone(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "load".into(),
                role: "assistant".into(),
                text: body,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::Loading),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        dedupe_assistant_messages_since_last_user(&mut msgs);
        assert_eq!(msgs.len(), 3);
    }

    #[test]
    fn dedupe_removes_final_response_snapshot_when_canonical_exists() {
        let body = "当前目录下有三个压缩包：\n\n1. **A** — x";
        let snapshot = body.replace("\n\n", "\n");
        let mut msgs = vec![
            StoredMessage {
                id: "u".into(),
                role: "user".into(),
                text: "分析".into(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "a1".into(),
                role: "assistant".into(),
                text: body.to_string(),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            StoredMessage {
                id: "snap".into(),
                role: "assistant".into(),
                text: snapshot,
                reasoning_text: String::new(),
                image_urls: vec![],
                state: Some(StoredMessageState::TimelineUiJson(
                    r#"{"k":"cm_tl","t":"final_response_snapshot"}"#.into(),
                )),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
        ];
        dedupe_assistant_messages_since_last_user(&mut msgs);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].id, "a1");
    }
}
