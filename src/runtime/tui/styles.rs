//! tui-markdown 样式表与代码高亮主题名。
//!
//! 标题样式按 Markdown 层级（`#`…`######`，`level` 为 1–6）区分，便于在终端里扫读结构。

use ratatui::style::{Color, Modifier, Style};
use tui_markdown::StyleSheet;

/// `tui_markdown::StyleSheet::heading` 的 `level` 为从 1 起的标题级；异常值按 H6 处理。
fn heading_level_clamp(level: u8) -> u8 {
    if level == 0 { 1 } else { level.min(6) }
}

/// 暗色聊天区：上级标题更亮，下级略弱并增加斜体。
fn heading_style_dark(level: u8) -> Style {
    match heading_level_clamp(level) {
        1 => Style::new()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::new()
            .fg(Color::LightCyan)
            .add_modifier(Modifier::BOLD),
        3 => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        4 => Style::new()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        5 => Style::new()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        _ => Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    }
}

/// 亮色聊天区：深前景 + 色相递进，避免与正文灰度糊在一起。
fn heading_style_light(level: u8) -> Style {
    match heading_level_clamp(level) {
        1 => Style::new()
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD),
        3 => Style::new()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
        4 => Style::new()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
        5 => Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
        _ => Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::ITALIC),
    }
}

/// 高对比暗底：用多种高可读色相区分层级。
fn heading_style_high_contrast_dark(level: u8) -> Style {
    match heading_level_clamp(level) {
        1 => Style::new()
            .fg(Color::White)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        3 => Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        4 => Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
        5 => Style::new()
            .fg(Color::LightMagenta)
            .add_modifier(Modifier::BOLD),
        _ => Style::new()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    }
}

/// 高对比亮底：深黑/深灰与少量饱和色，保证轮廓清晰。
fn heading_style_high_contrast_light(level: u8) -> Style {
    match heading_level_clamp(level) {
        1 => Style::new()
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
        2 => Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        3 => Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD),
        4 => Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        5 => Style::new()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
        _ => Style::new()
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD | Modifier::ITALIC),
    }
}

#[derive(Debug, Clone)]
pub(super) struct DarkStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct LightStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct HighContrastDarkStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct HighContrastLightStyleSheet;

impl StyleSheet for DarkStyleSheet {
    fn heading(&self, level: u8) -> Style {
        heading_style_dark(level)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::White)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::LightBlue)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Yellow)
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::LightYellow)
    }
}

impl StyleSheet for LightStyleSheet {
    fn heading(&self, level: u8) -> Style {
        heading_style_light(level)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Blue)
            .add_modifier(Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Magenta)
    }

    fn heading_meta(&self) -> Style {
        Style::new().dim()
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }
}

impl StyleSheet for HighContrastDarkStyleSheet {
    fn heading(&self, level: u8) -> Style {
        heading_style_high_contrast_dark(level)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::White)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::LightYellow)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::LightCyan)
    }
}

impl StyleSheet for HighContrastLightStyleSheet {
    fn heading(&self, level: u8) -> Style {
        heading_style_high_contrast_light(level)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black)
    }

    fn link(&self) -> Style {
        Style::new()
            .fg(Color::Blue)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    }

    fn blockquote(&self) -> Style {
        Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD)
    }

    fn heading_meta(&self) -> Style {
        Style::new().fg(Color::DarkGray)
    }

    fn metadata_block(&self) -> Style {
        Style::new().fg(Color::Black)
    }
}

pub(super) fn code_themes() -> [&'static str; 5] {
    [
        "base16-ocean.dark",
        "base16-ocean.light",
        "Solarized (dark)",
        "Solarized (light)",
        "InspiredGitHub",
    ]
}
