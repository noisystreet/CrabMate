//! TUI 外观主题常量集中定义。
//!
//! 将 `draw.rs` 中散落的颜色、布局比例、图标等视觉常量集中到 `TuiTheme`，
//! 便于后续做成用户可配置的主题文件。当前提供 `default()` 硬编码主题。

use ratatui::style::Color;

/// TUI 布局常量。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct LayoutConst {
    pub chat_width_percent: u16,
    pub right_panel_width_percent: u16,
    pub min_input_rows: u16,
}

/// 面板分隔线颜色。
#[derive(Debug, Clone)]
pub(super) struct SeparatorTheme {
    pub pane_rule_color: Color,
    /// 渐变分隔线灰度色阶（xterm-256 Indexed），从边缘到中心再到边缘。
    pub gradient_shades: Vec<u8>,
}

/// 聊天区消息头样式。
#[derive(Debug, Clone)]
pub(super) struct MessageHeaderTheme {
    pub user_icon: &'static str,
    pub user_color: Color,
    pub assistant_icon: &'static str,
    pub assistant_color: Color,
}

/// 模型阶段颜色映射。
#[derive(Debug, Clone)]
pub(super) struct PhaseColors {
    pub idle: Color,
    pub thinking: Color,
    pub selecting_tools: Color,
    pub answering: Color,
    pub tool_running: Color,
    pub awaiting_approval: Color,
    pub error: Color,
}

/// 右侧面板标签颜色。
#[derive(Debug, Clone)]
pub(super) struct RightTabColors {
    pub workspace: Color,
    pub queue: Color,
    pub tasks: Color,
    pub schedule: Color,
}

/// 聊天区角色内容颜色。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct ChatContentColors {
    pub assistant_gray: Color,
    pub tool_output: Color,
    pub input_text: Color,
}

/// 弹窗主题。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct PopupTheme {
    pub title_color: Color,
    pub border_color: Color,
    pub subtitle_color: Color,
    pub approval_bg: Color,
    pub approval_fg: Color,
    pub approval_option_color: Color,
}

/// 状态栏主题。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct StatusBarTheme {
    pub label_color: Color,
    pub label_color_hc: Color,
    pub model_name_color: Color,
    pub model_name_color_hc: Color,
    pub hc_prefix_color: Color,
    pub hc_prefix_color_hc: Color,
}

/// 完整主题。
/// 部分字段尚未被 `draw.rs` 直接引用（popup、status_bar 迁移在后续 PR），先保留以维持结构完整。
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub(super) struct TuiTheme {
    pub layout: LayoutConst,
    pub separator: SeparatorTheme,
    pub message_header: MessageHeaderTheme,
    pub phase_colors: PhaseColors,
    pub right_tab: RightTabColors,
    pub chat_content: ChatContentColors,
    pub popup: PopupTheme,
    pub status_bar: StatusBarTheme,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            layout: LayoutConst {
                chat_width_percent: 65,
                right_panel_width_percent: 35,
                min_input_rows: 2,
            },
            separator: SeparatorTheme {
                pane_rule_color: Color::DarkGray,
                gradient_shades: vec![248, 246, 244, 243, 242, 243, 244, 246, 248],
            },
            message_header: MessageHeaderTheme {
                user_icon: "▸ ",
                user_color: Color::Cyan,
                assistant_icon: "◆ ",
                assistant_color: Color::Green,
            },
            phase_colors: PhaseColors {
                idle: Color::Green,
                thinking: Color::Cyan,
                selecting_tools: Color::Yellow,
                answering: Color::Blue,
                tool_running: Color::Rgb(255, 165, 0),
                awaiting_approval: Color::Magenta,
                error: Color::Red,
            },
            right_tab: RightTabColors {
                workspace: Color::Green,
                queue: Color::Magenta,
                tasks: Color::Yellow,
                schedule: Color::Cyan,
            },
            chat_content: ChatContentColors {
                assistant_gray: Color::Indexed(245),
                tool_output: Color::Indexed(214),
                input_text: Color::Gray,
            },
            popup: PopupTheme {
                title_color: Color::Cyan,
                border_color: Color::Yellow,
                subtitle_color: Color::DarkGray,
                approval_bg: Color::Rgb(220, 220, 220),
                approval_fg: Color::Black,
                approval_option_color: Color::Yellow,
            },
            status_bar: StatusBarTheme {
                label_color: Color::DarkGray,
                label_color_hc: Color::Gray,
                model_name_color: Color::LightCyan,
                model_name_color_hc: Color::LightYellow,
                hc_prefix_color: Color::Gray,
                hc_prefix_color_hc: Color::White,
            },
        }
    }
}
