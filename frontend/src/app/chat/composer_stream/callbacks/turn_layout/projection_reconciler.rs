//! 将 [`crabmate_turn_layout::TurnProjection`] 落到 `StoredMessage`。
//!
//! Phase D：定稿旁白只经本模块 upsert；锚定 active 旁白也经此入口写 `turn-commentary-*`。
//! Loading 句柄不承载这些正文（见 Phase B/C）。

use crabmate_turn_layout::{ASSISTANT_COMMENTARY, TurnProjection};

use super::bubble_queue::BubbleOutputQueue;

/// 将投影中的已关闭 commentary 行 upsert 到锚定工具前。
pub(super) fn reconcile_finalized_commentary(
    messages: &mut Vec<crate::storage::StoredMessage>,
    projection: &TurnProjection,
) {
    for row in &projection.finalized_rows {
        if row.kind != ASSISTANT_COMMENTARY {
            continue;
        }
        let Some(tool_call_id) = row.tool_call_id.as_deref() else {
            continue;
        };
        let _ = BubbleOutputQueue::upsert_commentary_before_tool(
            messages,
            tool_call_id,
            row.text.clone(),
        );
    }
}

/// 锚定 open 旁白：写入 `turn-commentary-*`（工具可尚未存在）。
///
/// 无锚点 / 非 commentary active → `false`（由 overlay preview 路径处理）。
pub(super) fn try_reconcile_active_anchored_commentary(
    messages: &mut Vec<crate::storage::StoredMessage>,
    projection: &TurnProjection,
    loading_tail_id: Option<&str>,
) -> bool {
    let Some(active) = projection.active_row.as_ref() else {
        return false;
    };
    if active.kind != ASSISTANT_COMMENTARY {
        return false;
    }
    let Some(tcid) = active.before_tool_call_id.as_deref() else {
        return false;
    };
    BubbleOutputQueue::upsert_streaming_anchored_commentary(
        messages,
        tcid,
        active.text.clone(),
        loading_tail_id,
    )
}
