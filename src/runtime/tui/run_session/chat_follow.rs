//! 聊天区跟底意图（对齐 Web `scroll_shell` / `scroll_follow`）。

use super::TuiModel;

/// 对齐 Web `STICK_NEAR_BOTTOM_GAP_PX`：距底 ≤ 此行数视为近底，可 re-pin。
pub(super) const STICK_NEAR_BOTTOM_ROWS: u16 = 2;
/// 对齐 Web `STICK_UNPIN_GAP_PX`：拖滚动条离底超过此行数则 unpin。
pub(super) const STICK_UNPIN_GAP_ROWS: u16 = 4;

#[must_use]
pub(super) fn chat_scroll_gap_from_bottom(scroll_y: u16, max_scroll: u16) -> u16 {
    max_scroll.saturating_sub(scroll_y)
}

#[must_use]
pub(super) fn is_near_chat_bottom(gap_rows: u16) -> bool {
    gap_rows <= STICK_NEAR_BOTTOM_ROWS
}

/// 拖滚动条：近底 re-pin；离底超过 unpin 阈值则 unpin（对齐 Web pointer 离底）。
pub(super) fn apply_chat_scrollbar_follow_intent(
    model: &mut TuiModel,
    scroll_y: u16,
    max_scroll: u16,
) {
    model.chat_scroll_y = scroll_y;
    model.chat_user_scroll_down = false;
    let gap = chat_scroll_gap_from_bottom(scroll_y, max_scroll);
    if is_near_chat_bottom(gap) {
        model.chat_follow_bottom = true;
    } else if gap > STICK_UNPIN_GAP_ROWS {
        model.chat_follow_bottom = false;
    }
}

/// 用户滚轮↑ / PgUp / Home：unpin（对齐 Web wheel↑ / Home）。
pub(super) fn note_chat_user_scroll_up(model: &mut TuiModel) {
    model.chat_follow_bottom = false;
    model.chat_user_scroll_down = false;
}

/// 用户滚轮↓ / PgDn：标记下滑，下一帧按 gap 决定是否 re-pin（对齐 Web `scrolled_down`）。
pub(super) fn note_chat_user_scroll_down(model: &mut TuiModel) {
    model.chat_user_scroll_down = true;
}

/// 是否应在主动下滑后 re-pin（对齐 Web：`near || (scrolled_down && gap ≤ UNPIN)`）。
#[must_use]
pub(super) fn should_repin_after_scroll_down(gap_rows: u16) -> bool {
    gap_rows <= STICK_UNPIN_GAP_ROWS
}

/// 消费 [`TuiModel::chat_user_scroll_down`] 与近底静止 re-pin（在算 stick_mode 之前调用）。
pub(super) fn resolve_chat_follow_after_user_scroll(model: &mut TuiModel, max_scroll: u16) {
    let y = model.chat_scroll_y.min(max_scroll);
    model.chat_scroll_y = y;
    let gap = chat_scroll_gap_from_bottom(y, max_scroll);
    if model.chat_user_scroll_down {
        model.chat_user_scroll_down = false;
        if should_repin_after_scroll_down(gap) {
            model.chat_follow_bottom = true;
        }
    } else if !model.chat_follow_bottom && is_near_chat_bottom(gap) {
        // 已在底部（无下滑标记）仍可 re-pin，对齐 Web 近底 / 哨兵
        model.chat_follow_bottom = true;
    }
}

#[cfg(test)]
mod tests {
    use super::super::turn_project::TuiTurnProjection;
    use super::super::{TuiFocus, TuiModel};
    use super::*;

    fn empty_model() -> TuiModel {
        TuiModel {
            header_line: String::new(),
            nav_summary: String::new(),
            right_summary: String::new(),
            transcript: String::new(),
            chat_scroll_y: 0,
            chat_snap_bottom_next_draw: false,
            chat_follow_bottom: true,
            chat_user_scroll_down: false,
            chat_scrollbar_dragging: false,
            input: String::new(),
            status_chips: String::new(),
            status_run: "就绪".into(),
            focus: TuiFocus::default(),
            approval_modal: None,
            approval_backlog: Default::default(),
            clarification_modal: None,
            clarification_backlog: Default::default(),
            workspace_path_buf: std::path::PathBuf::from("."),
            workspace_modal: None,
            sqlite_conversation_id: None,
            recent_conversation_ids: Vec::new(),
            control_plane_tail: String::new(),
            turn_projection: TuiTurnProjection::default(),
            committed_turns: Default::default(),
        }
    }

    #[test]
    fn near_bottom_gap_matches_web_rows() {
        assert!(is_near_chat_bottom(0));
        assert!(is_near_chat_bottom(STICK_NEAR_BOTTOM_ROWS));
        assert!(!is_near_chat_bottom(STICK_NEAR_BOTTOM_ROWS + 1));
    }

    #[test]
    fn scrollbar_follow_intent_pins_and_unpins() {
        let mut model = empty_model();
        apply_chat_scrollbar_follow_intent(&mut model, 0, 20);
        assert!(!model.chat_follow_bottom);
        assert_eq!(model.chat_scroll_y, 0);

        apply_chat_scrollbar_follow_intent(&mut model, 19, 20);
        assert!(model.chat_follow_bottom);
        assert_eq!(model.chat_scroll_y, 19);
    }

    #[test]
    fn scroll_down_repins_within_unpin_gap_like_web() {
        assert!(should_repin_after_scroll_down(0));
        assert!(should_repin_after_scroll_down(STICK_UNPIN_GAP_ROWS));
        assert!(!should_repin_after_scroll_down(STICK_UNPIN_GAP_ROWS + 1));

        let mut model = empty_model();
        model.chat_scroll_y = 16;
        model.chat_follow_bottom = false;
        model.chat_user_scroll_down = true;
        // gap = 20-16 = 4 == UNPIN → re-pin
        resolve_chat_follow_after_user_scroll(&mut model, 20);
        assert!(model.chat_follow_bottom);
        assert!(!model.chat_user_scroll_down);

        model.chat_follow_bottom = false;
        model.chat_user_scroll_down = true;
        model.chat_scroll_y = 10; // gap = 10 > UNPIN
        resolve_chat_follow_after_user_scroll(&mut model, 20);
        assert!(!model.chat_follow_bottom);
        assert!(!model.chat_user_scroll_down);
    }

    #[test]
    fn scroll_up_unpins_and_clears_scroll_down_flag() {
        let mut model = empty_model();
        model.chat_scroll_y = 5;
        model.chat_follow_bottom = true;
        model.chat_user_scroll_down = true;
        note_chat_user_scroll_up(&mut model);
        assert!(!model.chat_follow_bottom);
        assert!(!model.chat_user_scroll_down);
    }
}
