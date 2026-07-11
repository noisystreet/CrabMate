//! 流式跟读：距底在阈值内时用 `scrollTop += ΔscrollHeight`，避免反复 `scrollTop = scrollHeight` 台阶感。

use web_sys::HtmlElement;

use crate::app_prefs::STICKY_BOTTOM_THRESHOLD_PX;

/// 跨次 `wire_content_follow_scroll` 调用记住上一帧 `scrollHeight`。
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ScrollAnchorState {
    pub last_scroll_height: i32,
}

impl ScrollAnchorState {
    fn apply_delta_follow_at_threshold(&mut self, el: &HtmlElement, threshold: i32) -> bool {
        apply_delta_follow_if_at_edge(el, self, threshold)
    }
}

/// 距底像素（≤0 表示已贴底或超出）。
#[must_use]
pub(crate) fn scroll_gap_from_bottom(
    scroll_height: i32,
    scroll_top: i32,
    client_height: i32,
) -> i32 {
    scroll_height - scroll_top - client_height
}

/// 在 live edge 时用高度差追底；否则只更新 `last_scroll_height`。
#[must_use]
pub(crate) fn apply_delta_follow_if_at_edge(
    el: &HtmlElement,
    state: &mut ScrollAnchorState,
    threshold: i32,
) -> bool {
    let scroll_height = el.scroll_height();
    let gap = scroll_gap_from_bottom(scroll_height, el.scroll_top(), el.client_height());
    let at_edge = gap <= threshold;
    if at_edge {
        let delta = scroll_height.saturating_sub(state.last_scroll_height);
        if delta > 0 {
            el.set_scroll_top(el.scroll_top().saturating_add(delta));
        } else if gap > 0 {
            el.set_scroll_top(scroll_height);
        }
        state.last_scroll_height = el.scroll_height();
        true
    } else {
        state.last_scroll_height = scroll_height;
        false
    }
}

/// 共享 baseline 的 delta 追底（供 [`crate::app::chat::scroll_follow`] 流式路径）。
pub(crate) fn follow_content_growth_by_delta(
    el: &HtmlElement,
    state: &mut ScrollAnchorState,
) -> bool {
    state.apply_delta_follow_at_threshold(el, STICKY_BOTTOM_THRESHOLD_PX)
}

#[cfg(test)]
mod tests {
    use super::{ScrollAnchorState, scroll_gap_from_bottom};

    #[test]
    fn gap_zero_when_flush_to_bottom() {
        assert_eq!(scroll_gap_from_bottom(1000, 900, 100), 0);
    }

    #[test]
    fn delta_follow_adds_height_delta_when_at_edge() {
        let mut state = ScrollAnchorState {
            last_scroll_height: 900,
        };
        let (top, last) = delta_follow_plan(850, 1000, 100, &mut state, 80);
        assert_eq!(top, 950);
        assert_eq!(last, 1000);
    }

    #[test]
    fn delta_follow_skips_when_far_from_bottom() {
        let mut state = ScrollAnchorState {
            last_scroll_height: 900,
        };
        let (top, last) = delta_follow_plan(400, 1000, 100, &mut state, 80);
        assert_eq!(top, 400);
        assert_eq!(last, 1000);
    }

    /// 纯算术模拟 DOM（无 `HtmlElement`）。
    fn delta_follow_plan(
        scroll_top: i32,
        scroll_height: i32,
        client_height: i32,
        state: &mut ScrollAnchorState,
        threshold: i32,
    ) -> (i32, i32) {
        let gap = scroll_gap_from_bottom(scroll_height, scroll_top, client_height);
        let at_edge = gap <= threshold;
        let mut top = scroll_top;
        if at_edge {
            let delta = scroll_height.saturating_sub(state.last_scroll_height);
            if delta > 0 {
                top = scroll_top.saturating_add(delta);
            } else if gap > 0 {
                top = scroll_height;
            }
            state.last_scroll_height = scroll_height;
        } else {
            state.last_scroll_height = scroll_height;
        }
        (top, state.last_scroll_height)
    }
}
