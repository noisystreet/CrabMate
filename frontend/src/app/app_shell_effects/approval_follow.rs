//! 审批 UI：待审批会话 id 变化时复位展开态。

use leptos::prelude::*;

/// 新待审批会话 id 变化时收起审批条展开态。
pub fn wire_approval_expanded_follows_pending(
    pending_approval: RwSignal<Option<(String, String, String)>>,
    last_approval_sid: RwSignal<String>,
    approval_expanded: RwSignal<bool>,
) {
    Effect::new(move |_| {
        if let Some((sid, _, _)) = pending_approval.get() {
            if last_approval_sid.get_untracked() != sid {
                last_approval_sid.set(sid);
                approval_expanded.set(false);
            }
        } else {
            last_approval_sid.set(String::new());
        }
    });
}
