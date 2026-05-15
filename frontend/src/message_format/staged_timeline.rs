//! 分阶段实施时间线：`system` 消息固定前缀与聚合判断。

use crate::storage::StoredMessage;

/// `role: system` 时间线旁注（分阶段步进）；前缀仅供 UI 分类，展示时剥去。
pub const STAGED_TIMELINE_SYSTEM_PREFIX: &str = "### CrabMate·staged_timeline\n";

pub fn staged_timeline_system_message_body(body: &str) -> String {
    format!("{STAGED_TIMELINE_SYSTEM_PREFIX}{body}")
}

/// Web 聊天列：`on_staged_plan_step_*` 落盘的 `system` 旁注（固定前缀）。
#[inline]
pub(crate) fn is_staged_timeline_stored_message(m: &StoredMessage) -> bool {
    m.role == "system" && m.text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX)
}

/// 分步工具时间线气泡（`on_staged_plan_step_*` 的 `system` 或 `on_timeline_log` 写入的 `assistant` + 同前缀）。
/// 外壳仍用普通 `msg-system` / `msg-assistant`，靠元信息文案与行内「分步执行」横幅区分，避免多一种气泡样式。
#[inline]
pub fn is_staged_timeline_bubble(m: &StoredMessage) -> bool {
    is_staged_timeline_stored_message(m)
        || (m.role == "assistant"
            && !m.is_tool
            && m.text.starts_with(STAGED_TIMELINE_SYSTEM_PREFIX))
}
