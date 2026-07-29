//! 流式助手**正文所有权**约定（Phase A–D 收口后的 P0）。
//!
//! # 允许的写入点
//!
//! | 载体 | 谁写 | 合法内容 |
//! |------|------|----------|
//! | `turn-commentary-{tcid}` | [`super::projection_reconciler`] | 锚定旁白（定稿或 open 锚定 upsert） |
//! | `turn-final-answer` | reconciler / `ensure_final_*` | 终答 |
//! | `StreamTextOverlay` answer | delta 热路径 / `sync_stream_preview` | **仅** active 预览：无锚点 open 旁白或未落盘终答增量 |
//! | loading `StoredMessage.text` | **禁止**承载旁白/终答；仅 abort/错误诊断等兼容文案 | 句柄态应为空 |
//! | 普通 assistant（rotate finalize） | 多轮无工具升格 / detach 后的历史行 | 定稿后不再作 active |
//!
//! # 不变量
//!
//! 1. **同一投影键至多一条行**：`turn-commentary-{tcid}` / `turn-final-answer` 禁止第二行。
//! 2. **overlay ≡ active 渲染缓存**：不得与已定稿同文并行长期持有（定稿后清 overlay）。
//! 3. **`loading_handoff` 仅为兼容**：主路径旁白不写 `loading.text`；仅当残留非空 `loading.text`
//!    与定稿行同文时清空，并计入 `commentary_handoff`。

use crate::storage::StoredMessage;

use super::bubble_queue::{FINAL_ANSWER_ROW_ID, is_commentary_row_id};

/// 同一 `tool_call_id` 不得出现两条 commentary 行。
#[must_use]
pub(super) fn duplicate_commentary_row_ids(messages: &[StoredMessage]) -> Vec<String> {
    let mut seen = Vec::new();
    let mut dups = Vec::new();
    for m in messages {
        if !is_commentary_row_id(m.id.as_str()) {
            continue;
        }
        if seen.iter().any(|id: &String| id == &m.id) {
            dups.push(m.id.clone());
        } else {
            seen.push(m.id.clone());
        }
    }
    dups
}

/// `turn-final-answer` 至多一条。
#[must_use]
pub(super) fn final_answer_row_count(messages: &[StoredMessage]) -> usize {
    messages
        .iter()
        .filter(|m| m.id == FINAL_ANSWER_ROW_ID)
        .count()
}

/// loading 句柄是否仍持有与某条定稿 commentary/终答同文的正文（主路径不应发生）。
#[must_use]
pub(super) fn loading_holds_duplicate_of_persisted(
    messages: &[StoredMessage],
    loading_id: &str,
) -> bool {
    let Some(load) = messages.iter().find(|m| m.id == loading_id) else {
        return false;
    };
    let live = load.text.trim();
    if live.is_empty() {
        return false;
    }
    messages.iter().any(|m| {
        m.id != loading_id
            && m.role == "assistant"
            && !m.is_tool
            && (is_commentary_row_id(m.id.as_str()) || m.id == FINAL_ANSWER_ROW_ID)
            && m.text.trim() == live
    })
}

/// 构造 commentary 行 id（测试与断言用）。
#[cfg(test)]
#[must_use]
pub(super) fn expected_commentary_id(tool_call_id: &str) -> String {
    super::bubble_queue::commentary_row_id(tool_call_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::StoredMessageState;

    fn msg(id: &str, text: &str, loading: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: "assistant".into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: loading.then_some(StoredMessageState::Loading),
            is_tool: false,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn detects_duplicate_commentary_ids() {
        let messages = vec![
            msg("turn-commentary-tc_a", "A。", false),
            msg("turn-commentary-tc_a", "A 重复。", false),
        ];
        assert_eq!(
            duplicate_commentary_row_ids(&messages),
            vec!["turn-commentary-tc_a".to_string()]
        );
    }

    #[test]
    fn loading_duplicate_of_commentary_is_flagged() {
        let messages = vec![
            msg("turn-commentary-tc1", "旁白。", false),
            msg("load", "旁白。", true),
        ];
        assert!(loading_holds_duplicate_of_persisted(&messages, "load"));
    }

    #[test]
    fn empty_loading_is_not_duplicate() {
        let messages = vec![
            msg("turn-commentary-tc1", "旁白。", false),
            msg("load", "", true),
        ];
        assert!(!loading_holds_duplicate_of_persisted(&messages, "load"));
    }

    #[test]
    fn single_final_answer_ok() {
        let messages = vec![msg(FINAL_ANSWER_ROW_ID, "完。", false)];
        assert_eq!(final_answer_row_count(&messages), 1);
    }
}
