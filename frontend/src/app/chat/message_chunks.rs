//! 将 `StoredMessage` 切片折叠为连续工具组，供聊天列迭代渲染。
//!
//! 可见性筛选与 fuzzy dedupe 见 [`crate::visible_messages`]（单一读路径）；本模块只负责 **chunk 折叠**。

use crate::storage::StoredMessage;
use crate::visible_messages::VisibleMessageScope;

#[derive(Clone)]
pub(crate) enum ChatChunk {
    Single {
        idx: usize,
        msg: StoredMessage,
    },
    ToolGroup {
        head_id: String,
        items: Vec<(usize, StoredMessage)>,
    },
}

/// 供 [`leptos::prelude::For`] 使用的稳定键：单条用消息 id；工具组用首条 id 保持稳定。
pub(crate) fn chat_chunk_stable_key(chunk: &ChatChunk) -> String {
    match chunk {
        ChatChunk::Single { msg, .. } => format!("s:{}", msg.id),
        ChatChunk::ToolGroup { head_id, .. } => format!("tg:{head_id}"),
    }
}

fn push_tool_run_chunk(out: &mut Vec<ChatChunk>, slice: Vec<(usize, StoredMessage)>) {
    match slice.len() {
        0 => {}
        1 => {
            let (idx, msg) = slice.into_iter().next().expect("len 1");
            out.push(ChatChunk::Single { idx, msg });
        }
        _ => {
            let head_id = slice.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
            out.push(ChatChunk::ToolGroup {
                head_id,
                items: slice,
            });
        }
    }
}

fn chunk_tool_run(msgs: &[StoredMessage], start: usize, end: usize, out: &mut Vec<ChatChunk>) {
    let slice: Vec<_> = (start..end).map(|j| (j, msgs[j].clone())).collect();
    push_tool_run_chunk(out, slice);
}

pub(crate) fn chunk_messages(msgs: &[StoredMessage]) -> Vec<ChatChunk> {
    let visible =
        crate::visible_messages::visible_message_indices(msgs, VisibleMessageScope::ChatColumn);
    let mut out = Vec::new();
    let mut vi = 0usize;
    while vi < visible.len() {
        let i = visible[vi];
        if msgs[i].is_tool {
            let start = i;
            let mut end = i + 1;
            while vi + 1 < visible.len() && msgs[visible[vi + 1]].is_tool {
                vi += 1;
                end = visible[vi] + 1;
            }
            chunk_tool_run(msgs, start, end, &mut out);
        } else {
            out.push(ChatChunk::Single {
                idx: i,
                msg: msgs[i].clone(),
            });
        }
        vi += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_msg(id: &str, role: &str, text: &str, is_tool: bool) -> StoredMessage {
        StoredMessage {
            id: id.into(),
            role: role.into(),
            text: text.into(),
            reasoning_text: String::new(),
            image_urls: vec![],
            state: None,
            is_tool,
            tool_call_id: None,
            tool_name: None,
            created_at: 0,
        }
    }

    #[test]
    fn commentary_before_tools_skipped_in_chunks() {
        let msgs = vec![
            empty_msg("u", "user", "hi", false),
            StoredMessage {
                id: "c1".into(),
                role: "assistant".into(),
                text: String::new(),
                reasoning_text: "旁注".into(),
                image_urls: vec![],
                state: Some(crate::storage::StoredMessageState::CommentaryBeforeTools),
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            empty_msg("a", "assistant", "真正终答", false),
        ];
        let chunks = chunk_messages(&msgs);
        assert_eq!(chunks.len(), 2);
    }

    #[test]
    fn fuzzy_duplicate_assistant_rows_all_visible_in_chunks() {
        let listing = "当前目录下有三个压缩包：\n\n1. **A** — x";
        let compact = "当前目录下有三个压缩包：\n1. **A** — x";
        let msgs = vec![
            empty_msg("u", "user", "分析", false),
            empty_msg("a1", "assistant", listing, false),
            empty_msg("a2", "assistant", compact, false),
        ];
        let chunks = chunk_messages(&msgs);
        assert_eq!(chunks.len(), 3);
    }
}
