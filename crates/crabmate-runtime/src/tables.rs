//! Unicode 等宽排版辅助函数（从 `cli_repl_ui/tables.rs` 提取）。
//!
//! 主要用于终端表格的列宽对齐计算。

/// 计算字符串在终端中的显示宽度（中文字符等算 2，ASCII 算 1）。
#[cfg(feature = "repl")]
pub fn unicode_display_width(s: &str) -> usize {
    use unicode_width::UnicodeWidthStr;
    s.width()
}
