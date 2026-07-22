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

#[inline]
fn is_staged_or_tool(m: &StoredMessage) -> bool {
    m.is_tool
}

/// 从 `start`（须为分阶段旁注）起，向后扩展直到遇到非（旁注|工具）消息。
fn staged_cluster_end_exclusive(msgs: &[StoredMessage], start: usize) -> usize {
    let mut j = start + 1;
    while j < msgs.len() && is_staged_or_tool(&msgs[j]) {
        j += 1;
    }
    j
}

fn chunk_tool_run(msgs: &[StoredMessage], start: usize, end: usize, out: &mut Vec<ChatChunk>) {
    let slice: Vec<_> = (start..end).map(|j| (j, msgs[j].clone())).collect();
    push_tool_run_chunk(out, slice);
}

fn chunk_staged_cluster(
    msgs: &[StoredMessage],
    cluster_start: usize,
    cluster_end: usize,
    out: &mut Vec<ChatChunk>,
) {
    let mut k = cluster_start;
    while k < cluster_end {
        if msgs[k].is_tool {
            let t_start = k;
            while k < cluster_end && msgs[k].is_tool {
                k += 1;
            }
            chunk_tool_run(msgs, t_start, k, out);
        } else {
            k += 1;
        }
    }
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
        } else if false {
            let j = staged_cluster_end_exclusive(msgs, i);
            chunk_staged_cluster(msgs, i, j, &mut out);
            while vi < visible.len() && visible[vi] < j {
                vi += 1;
            }
            continue;
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
    use crate::message_format::staged_timeline_system_message_body;

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

    fn staged_line(id: &str, body_line: &str) -> StoredMessage {
        empty_msg(
            id,
            "system",
            &staged_timeline_system_message_body(body_line),
            false,
        )
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
    fn orchestration_route_timeline_skipped_in_chunks() {
        use crate::message_format::staged_timeline_system_message_body;

        let msgs = vec![
            empty_msg("u", "user", "hi", false),
            StoredMessage {
                id: "route".into(),
                role: "assistant".into(),
                text: staged_timeline_system_message_body("编排路由：freeform\n{}"),
                reasoning_text: String::new(),
                image_urls: vec![],
                state: None,
                is_tool: false,
                tool_call_id: None,
                tool_name: None,
                created_at: 0,
            },
            empty_msg("a", "assistant", "reply", false),
        ];
        let chunks = chunk_messages(&msgs);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], ChatChunk::Single { idx: 0, .. }));
        assert!(matches!(&chunks[1], ChatChunk::Single { idx: 2, .. }));
    }

    #[test]
    fn staged_bridged_across_tools_interleaves_in_order() {
        let msgs = vec![
            staged_line("s0", "cm_tl:0:start"),
            empty_msg("t0", "system", "tool0", true),
            staged_line("s1", "cm_tl:0:end"),
        ];
        let chunks = chunk_messages(&msgs);
        assert!(matches!(&chunks[0], ChatChunk::Single { idx: 0, .. }));
        assert!(
            matches!(&chunks[1], ChatChunk::Single { idx: 1, .. }),
            "expected single tool"
        );
        assert!(matches!(&chunks[2], ChatChunk::Single { idx: 2, .. }));
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn two_staged_clusters_split_by_user() {
        let msgs = vec![
            staged_line("a0", "cm_tl:0:start"),
            empty_msg("u", "user", "hi", false),
            staged_line("b0", "cm_tl:0:start"),
        ];
        let chunks = chunk_messages(&msgs);
        assert_eq!(chunks.len(), 3);
        assert!(matches!(&chunks[0], ChatChunk::Single { .. }));
        assert!(matches!(&chunks[1], ChatChunk::Single { .. }));
        assert!(matches!(&chunks[2], ChatChunk::Single { .. }));
    }

    #[test]
    fn consecutive_staged_lines_are_separate_singles() {
        let msgs = vec![staged_line("s0", "1. a"), staged_line("s1", "2. b")];
        let chunks = chunk_messages(&msgs);
        assert_eq!(chunks.len(), 2);
        assert!(matches!(&chunks[0], ChatChunk::Single { idx: 0, .. }));
        assert!(matches!(&chunks[1], ChatChunk::Single { idx: 1, .. }));
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
