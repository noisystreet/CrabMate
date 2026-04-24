//! 将 `StoredMessage` 切片折叠为连续工具组 / 分阶段时间线组，供聊天列迭代渲染。
//!
//! 分阶段时间线旁注（`### CrabMate·staged_timeline`）与工具输出需按时间顺序穿插展示：
//! 同类连续旁注会聚合为一张待办卡；工具输出连续段折叠为工具组。

use crate::message_format::is_staged_timeline_stored_message;
use crate::storage::StoredMessage;

pub(crate) enum ChatChunk {
    Single {
        idx: usize,
        msg: StoredMessage,
    },
    ToolGroup {
        head_id: String,
        items: Vec<(usize, StoredMessage)>,
    },
    StagedTimelineGroup {
        head_id: String,
        items: Vec<(usize, StoredMessage)>,
    },
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
    is_staged_timeline_stored_message(m) || m.is_tool
}

/// 从 `start`（须为分阶段旁注）起，向后扩展直到遇到非（旁注|工具）消息。
fn staged_cluster_end_exclusive(msgs: &[StoredMessage], start: usize) -> usize {
    let mut j = start + 1;
    while j < msgs.len() && is_staged_or_tool(&msgs[j]) {
        j += 1;
    }
    j
}

pub(crate) fn chunk_messages(msgs: &[StoredMessage]) -> Vec<ChatChunk> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < msgs.len() {
        if msgs[i].is_tool {
            let start = i;
            while i < msgs.len() && msgs[i].is_tool {
                i += 1;
            }
            let slice: Vec<_> = (start..i).map(|j| (j, msgs[j].clone())).collect();
            push_tool_run_chunk(&mut out, slice);
        } else if is_staged_timeline_stored_message(&msgs[i]) {
            // 分阶段簇内按时间顺序交替输出：
            // staged 段 -> tool 段 -> staged 段 ...
            let j = staged_cluster_end_exclusive(msgs, i);
            let mut k = i;
            while k < j {
                if is_staged_timeline_stored_message(&msgs[k]) {
                    let s_start = k;
                    while k < j && is_staged_timeline_stored_message(&msgs[k]) {
                        k += 1;
                    }
                    let items: Vec<_> = (s_start..k).map(|idx| (idx, msgs[idx].clone())).collect();
                    let head_id = items.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
                    out.push(ChatChunk::StagedTimelineGroup { head_id, items });
                } else if msgs[k].is_tool {
                    let t_start = k;
                    while k < j && msgs[k].is_tool {
                        k += 1;
                    }
                    let slice: Vec<_> = (t_start..k).map(|idx| (idx, msgs[idx].clone())).collect();
                    push_tool_run_chunk(&mut out, slice);
                } else {
                    k += 1;
                }
            }
            i = j;
        } else {
            out.push(ChatChunk::Single {
                idx: i,
                msg: msgs[i].clone(),
            });
            i += 1;
        }
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
    fn staged_bridged_across_tools_interleaves_in_order() {
        let msgs = vec![
            staged_line("s0", "cm_tl:0:start"),
            empty_msg("t0", "system", "tool0", true),
            staged_line("s1", "cm_tl:0:end"),
        ];
        let chunks = chunk_messages(&msgs);
        assert!(
            matches!(chunks[0], ChatChunk::StagedTimelineGroup { .. }),
            "expected staged group first"
        );
        if let ChatChunk::StagedTimelineGroup { ref items, .. } = chunks[0] {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].0, 0);
        } else {
            panic!("chunk 0");
        }
        assert!(
            matches!(&chunks[1], ChatChunk::Single { idx: 1, .. }),
            "expected single tool"
        );
        assert!(
            matches!(chunks[2], ChatChunk::StagedTimelineGroup { .. }),
            "expected trailing staged group"
        );
        if let ChatChunk::StagedTimelineGroup { ref items, .. } = chunks[2] {
            assert_eq!(items.len(), 1);
            assert_eq!(items[0].0, 2);
        } else {
            panic!("chunk 2");
        }
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
        assert!(matches!(chunks[0], ChatChunk::StagedTimelineGroup { .. }));
        assert!(matches!(&chunks[1], ChatChunk::Single { .. }));
        assert!(matches!(chunks[2], ChatChunk::StagedTimelineGroup { .. }));
    }
}
