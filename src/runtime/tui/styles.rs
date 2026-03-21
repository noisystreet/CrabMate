//! tui-markdown 样式表与代码高亮主题名。

use ratatui::style::{Color, Modifier, Style};
use tui_markdown::StyleSheet;

#[derive(Debug, Clone)]
pub(super) struct DarkStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct LightStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct HighContrastDarkStyleSheet;

#[derive(Debug, Clone)]
pub(super) struct HighContrastLightStyleSheet;

impl StyleSheet for DarkStyleSheet {
    fn heading(&self, _level: u8) -> Style {
        Style::new().bold()
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
    fn heading(&self, _level: u8) -> Style {
        Style::new().bold()
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black)
    }

    fn link(&self) -> Style {
        Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED)
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
    fn heading(&self, _level: u8) -> Style {
        Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::White).bg(Color::Black)
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
    fn heading(&self, _level: u8) -> Style {
        Style::new().fg(Color::Black).add_modifier(Modifier::BOLD)
    }

    fn code(&self) -> Style {
        Style::new().fg(Color::Black).bg(Color::White)
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
