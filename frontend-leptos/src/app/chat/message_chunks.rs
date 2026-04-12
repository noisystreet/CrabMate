//! 将 `StoredMessage` 切片折叠为连续工具组 / 分阶段时间线组，供聊天列迭代渲染。

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
            if slice.len() == 1 {
                let (idx, msg) = slice.into_iter().next().expect("len 1");
                out.push(ChatChunk::Single { idx, msg });
            } else {
                let head_id = slice.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
                out.push(ChatChunk::ToolGroup {
                    head_id,
                    items: slice,
                });
            }
        } else if is_staged_timeline_stored_message(&msgs[i]) {
            let start = i;
            while i < msgs.len() && is_staged_timeline_stored_message(&msgs[i]) {
                i += 1;
            }
            let slice: Vec<_> = (start..i).map(|j| (j, msgs[j].clone())).collect();
            if slice.len() == 1 {
                let (idx, msg) = slice.into_iter().next().expect("len 1");
                out.push(ChatChunk::Single { idx, msg });
            } else {
                let head_id = slice.first().map(|(_, m)| m.id.clone()).unwrap_or_default();
                out.push(ChatChunk::StagedTimelineGroup {
                    head_id,
                    items: slice,
                });
            }
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
