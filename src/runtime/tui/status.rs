//! 状态栏文案。

use super::state::{focus_name, Focus, TuiState};

pub(super) fn build_normal_status_line(model: &str, focus: Focus) -> String {
    format!(
        "模型：{}  |  Ctrl+C 退出  |  F2 切焦点（当前：{}）  |  Tab 切右侧面板  |  F3 代码主题  |  F4 Markdown样式",
        model,
        focus_name(focus)
    )
}

pub(super) fn set_normal_status_line(state: &mut TuiState, model: &str) {
    state.status_line = build_normal_status_line(model, state.focus);
}

pub(super) fn set_high_contrast_status_line(state: &mut TuiState, model: &str) {
    state.status_line = format!(
        "高对比度：{}（F5 切换）  |  {}",
        if state.high_contrast { "开" } else { "关" },
        build_normal_status_line(model, state.focus)
    );
}
