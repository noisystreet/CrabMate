//! 将 `StoredMessage` 切片折叠为连续工具组，供聊天列迭代渲染。
//!
//! 分阶段时间线旁注（`### CrabMate·staged_timeline`）与工具输出按时间顺序穿插展示：
//! **每条**旁注单独一条消息气泡（与分层子目标一致）；仅**连续工具**折叠为工具组。

use crate::message_format::is_staged_timeline_bubble;
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
    is_staged_timeline_bubble(m) || m.is_tool
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
        } else if is_staged_timeline_bubble(&msgs[i]) {
            // 分阶段簇内按时间顺序交替输出：
            // staged 段 -> tool 段 -> staged 段 ...
            let j = staged_cluster_end_exclusive(msgs, i);
            let mut k = i;
            while k < j {
                if is_staged_timeline_bubble(&msgs[k]) {
                    let s_start = k;
                    while k < j && is_staged_timeline_bubble(&msgs[k]) {
                        k += 1;
                    }
                    for (off, msg) in msgs[s_start..k].iter().cloned().enumerate() {
                        out.push(ChatChunk::Single {
                            idx: s_start + off,
                            msg,
                        });
                    }
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
}
