//! 状态栏文案。

use super::state::TuiState;

pub(super) fn build_normal_status_line(model: &str) -> String {
    format!("模型：{}", model)
}

pub(super) fn set_normal_status_line(state: &mut TuiState, model: &str) {
    state.status_line = build_normal_status_line(model);
}

pub(super) fn set_high_contrast_status_line(state: &mut TuiState, model: &str) {
    state.status_line = format!(
        "高对比度：{} | {}",
        if state.high_contrast { "开" } else { "关" },
        build_normal_status_line(model)
    );
}
