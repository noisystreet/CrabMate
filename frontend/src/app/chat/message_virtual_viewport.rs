//! 聊天消息列窗口化渲染：仅挂载视口附近的 [`ChatChunk`]，配合分页加载更早历史。

use leptos::prelude::{RwSignal, Set};

/// 超过该 chunk 数启用虚拟窗口（否则全量 `For`）。
pub const VIRTUAL_CHUNK_THRESHOLD: usize = 48;
/// 估算单行高度（px）；变高 Markdown 行会偏差，靠 overscan 缓冲。
pub const EST_CHUNK_HEIGHT_PX: i32 = 96;
pub const VIRTUAL_OVERSCAN_CHUNKS: usize = 8;
/// 距顶部小于该值时尝试拉取更早一页。
pub const LOAD_OLDER_SCROLL_TOP_PX: i32 = 120;

/// 将滚动容器的 `scrollTop` / `clientHeight` 写回虚拟窗口信号（程序化滚底后须同步）。
pub fn sync_virtual_scroll_signals_from_element(
    el: &web_sys::HtmlElement,
    virtual_scroll_top: RwSignal<i32>,
    virtual_viewport_height: RwSignal<i32>,
) {
    virtual_scroll_top.set(el.scroll_top());
    virtual_viewport_height.set(el.client_height());
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VirtualChunkRange {
    pub start: usize,
    pub end: usize,
}

impl VirtualChunkRange {
    #[must_use]
    pub const fn empty() -> Self {
        Self { start: 0, end: 0 }
    }

    #[must_use]
    pub fn covers_all(chunk_count: usize) -> Self {
        Self {
            start: 0,
            end: chunk_count,
        }
    }
}

#[must_use]
pub fn should_virtualize_chunks(chunk_count: usize) -> bool {
    chunk_count > VIRTUAL_CHUNK_THRESHOLD
}

/// 仅流式跟底时窗口化；用户手动滚动须全量 DOM，否则估算 spacer 与 `scrollHeight` 不同步会导致滚轮失灵。
#[must_use]
pub fn should_virtualize_chunks_for_stream_follow(
    chunk_count: usize,
    auto_scroll_chat: bool,
) -> bool {
    auto_scroll_chat && should_virtualize_chunks(chunk_count)
}

#[must_use]
pub fn virtual_window_visible_chunk_rows(viewport_height: i32) -> usize {
    let est = EST_CHUNK_HEIGHT_PX.max(48);
    (viewport_height / est).max(1) as usize + VIRTUAL_OVERSCAN_CHUNKS * 2 + 4
}

/// 窗口对齐列表尾部（流式跟底或滚过估算高度时）。
#[must_use]
pub fn tail_virtual_chunk_range(chunk_count: usize, viewport_height: i32) -> VirtualChunkRange {
    if chunk_count == 0 {
        return VirtualChunkRange::empty();
    }
    let visible_rows = virtual_window_visible_chunk_rows(viewport_height);
    let start = chunk_count.saturating_sub(visible_rows);
    VirtualChunkRange {
        start,
        end: chunk_count,
    }
}

/// 按 `scrollTop` 估算窗口（当前仅测试保留；手动滚动改走全量 DOM）。
#[allow(dead_code)]
#[must_use]
pub fn compute_virtual_chunk_range(
    scroll_top: i32,
    viewport_height: i32,
    chunk_count: usize,
) -> VirtualChunkRange {
    if chunk_count == 0 {
        return VirtualChunkRange::empty();
    }
    if !should_virtualize_chunks(chunk_count) {
        return VirtualChunkRange::covers_all(chunk_count);
    }
    let est = EST_CHUNK_HEIGHT_PX.max(48);
    let visible_rows = virtual_window_visible_chunk_rows(viewport_height);
    let max_scroll_est = chunk_count
        .saturating_mul(est as usize)
        .saturating_sub(est as usize);
    if scroll_top >= max_scroll_est as i32 {
        return tail_virtual_chunk_range(chunk_count, viewport_height);
    }
    let first_visible = (scroll_top / est).saturating_sub(VIRTUAL_OVERSCAN_CHUNKS as i32) as usize;
    let start = first_visible.min(chunk_count);
    let end = (start + visible_rows).min(chunk_count);
    if end <= start {
        return tail_virtual_chunk_range(chunk_count, viewport_height);
    }
    if end == chunk_count {
        return VirtualChunkRange { start, end };
    }
    VirtualChunkRange { start, end }
}

#[must_use]
pub fn virtual_spacer_heights(range: VirtualChunkRange, chunk_count: usize) -> (i32, i32) {
    let est = EST_CHUNK_HEIGHT_PX.max(48);
    let top = (range.start as i32).saturating_mul(est);
    let bottom = ((chunk_count.saturating_sub(range.end)) as i32).saturating_mul(est);
    (top, bottom)
}

#[must_use]
pub fn should_request_load_older(scroll_top: i32, has_older: bool, loading: bool) -> bool {
    has_older && !loading && scroll_top < LOAD_OLDER_SCROLL_TOP_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_list_not_virtualized() {
        let r = compute_virtual_chunk_range(0, 600, 10);
        assert_eq!(r, VirtualChunkRange::covers_all(10));
    }

    #[test]
    fn large_list_windows() {
        let r = compute_virtual_chunk_range(5000, 800, 200);
        assert!(r.start > 0);
        assert!(r.end <= 200);
        assert!(r.end > r.start);
    }

    #[test]
    fn stream_follow_gate() {
        assert!(!should_virtualize_chunks_for_stream_follow(100, false));
        assert!(should_virtualize_chunks_for_stream_follow(100, true));
        assert!(!should_virtualize_chunks_for_stream_follow(10, true));
    }

    #[test]
    fn past_estimated_end_anchors_tail() {
        let r = compute_virtual_chunk_range(50_000, 800, 60);
        assert_eq!(r.end, 60);
        assert!(r.start < 60);
        assert!(r.end > r.start);
    }
}
