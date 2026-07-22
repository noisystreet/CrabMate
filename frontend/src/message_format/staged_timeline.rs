//! 分阶段实施时间线：`system` 消息固定前缀与聚合判断。

/// `role: system` 时间线旁注（分阶段步进）；前缀仅供 UI 分类，展示时剥去。
pub const STAGED_TIMELINE_SYSTEM_PREFIX: &str = "### CrabMate·staged_timeline\n";

pub fn staged_timeline_system_message_body(body: &str) -> String {
    format!("{STAGED_TIMELINE_SYSTEM_PREFIX}{body}")
}
