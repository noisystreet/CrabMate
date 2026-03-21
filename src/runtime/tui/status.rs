//! 状态栏文案。

use super::state::TuiState;

pub(super) fn build_normal_status_line(model: &str) -> String {
    format!(
        "模型：{}  |  Ctrl+C 退出  |  F1 键位表  |  F2 焦点  |  Tab 右栏  |  F4 Markdown",
        model
    )
}

pub(super) fn set_normal_status_line(state: &mut TuiState, model: &str) {
    state.status_line = build_normal_status_line(model);
}

pub(super) fn set_high_contrast_status_line(state: &mut TuiState, model: &str) {
    state.status_line = format!(
        "高对比度：{}（F5 切换）  |  {}",
        if state.high_contrast { "开" } else { "关" },
        build_normal_status_line(model)
    );
}
