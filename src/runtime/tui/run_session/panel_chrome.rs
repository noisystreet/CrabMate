//! TUI 面板「纯色块」分区：无边框线，靠底色区分区域；顶栏 / 底栏亦为实心条，与色块齐平无缝。
//!
//! 默认实色底；**`CM_TUI_PANEL_BG=transparent`** 可退回透明（透出终端底色）。未来可接 TOML 主题。

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders};

/// 四区面板角色（决定默认底色档位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TuiPanelKind {
    NavLeft,
    Chat,
    Composer,
    SideRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FillMode {
    /// 四区均有实色（相邻轻微明度差）。
    Solid(PanelFills),
    /// 仅聚焦面板铺底。
    FocusOnly { focused: Color },
}

/// 四区实色（相邻区用轻微明度差，代替分割线）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PanelFills {
    nav: Color,
    chat: Color,
    composer: Color,
    side: Color,
    focused: Color,
}

impl PanelFills {
    fn for_kind(self, kind: TuiPanelKind) -> Color {
        match kind {
            TuiPanelKind::NavLeft => self.nav,
            TuiPanelKind::Chat => self.chat,
            TuiPanelKind::Composer => self.composer,
            TuiPanelKind::SideRight => self.side,
        }
    }

    fn solid_default() -> Self {
        Self {
            nav: Color::Rgb(30, 32, 38),
            chat: Color::Rgb(22, 24, 28),
            composer: Color::Rgb(26, 28, 34),
            side: Color::Rgb(30, 32, 38),
            focused: Color::Rgb(38, 44, 54),
        }
    }

    fn solid_dim() -> Self {
        Self {
            nav: Color::Rgb(18, 20, 24),
            chat: Color::Rgb(14, 16, 18),
            composer: Color::Rgb(16, 18, 22),
            side: Color::Rgb(18, 20, 24),
            focused: Color::Rgb(28, 32, 40),
        }
    }
}

/// 面板色块主题（无线框）。
#[derive(Clone, Debug)]
pub(super) struct TuiChromeTheme {
    fill: Option<FillMode>,
    pub title: Color,
    pub title_focused: Color,
}

impl TuiChromeTheme {
    /// `color=false`（含 `NO_COLOR`）时无底色。
    pub(super) fn for_color_mode(color: bool) -> Self {
        if !color {
            return Self {
                fill: None,
                title: Color::Reset,
                title_focused: Color::Reset,
            };
        }
        let mut theme = Self {
            fill: Some(FillMode::Solid(PanelFills::solid_default())),
            title: Color::Gray,
            title_focused: Color::Cyan,
        };
        theme.apply_panel_bg_env();
        theme
    }

    /// **`CM_TUI_PANEL_BG`**：
    /// - 未设置 / `solid` → 默认纯色块；
    /// - `transparent` / `0` / `off` / `none` → 透明；
    /// - `dim` → 更深色块；
    /// - `focus` → 仅聚焦面板铺底。
    fn apply_panel_bg_env(&mut self) {
        let Ok(raw) = std::env::var("CM_TUI_PANEL_BG") else {
            return;
        };
        let v = raw.trim().to_ascii_lowercase();
        if v.is_empty() || v == "solid" {
            self.fills_solid(PanelFills::solid_default());
            return;
        }
        if matches!(v.as_str(), "transparent" | "0" | "off" | "none") {
            self.fill = None;
            return;
        }
        if v == "dim" {
            self.fills_solid(PanelFills::solid_dim());
            return;
        }
        if v == "focus" {
            self.fill = Some(FillMode::FocusOnly {
                focused: Color::Rgb(28, 32, 38),
            });
        }
    }

    fn fills_solid(&mut self, fills: PanelFills) {
        self.fill = Some(FillMode::Solid(fills));
    }

    pub(super) fn panel_bg(&self, kind: TuiPanelKind, focused: bool) -> Option<Color> {
        match self.fill? {
            FillMode::Solid(fills) => Some(if focused {
                fills.focused
            } else {
                fills.for_kind(kind)
            }),
            FillMode::FocusOnly { focused: fg } => focused.then_some(fg),
        }
    }
}

/// 纯色块 `Block`：无 borders，标题落在色块顶行。
pub(super) fn panel_block<'a>(
    kind: TuiPanelKind,
    title: &'a str,
    theme: &TuiChromeTheme,
    color: bool,
    focused: bool,
) -> Block<'a> {
    let title_style = if color {
        let mut s = Style::default().fg(if focused {
            theme.title_focused
        } else {
            theme.title
        });
        if let Some(bg) = theme.panel_bg(kind, focused) {
            s = s.bg(bg);
        }
        s
    } else if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    };
    let mut block = Block::default()
        .borders(Borders::NONE)
        .title(Line::from(title))
        .title_style(title_style);
    if let Some(bg) = theme.panel_bg(kind, focused) {
        block = block.style(Style::default().bg(bg));
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solid_blocks_have_no_border_inset_only_title() {
        let theme = TuiChromeTheme::for_color_mode(true);
        let area = ratatui::layout::Rect::new(0, 0, 10, 5);
        for kind in [
            TuiPanelKind::NavLeft,
            TuiPanelKind::Chat,
            TuiPanelKind::Composer,
            TuiPanelKind::SideRight,
        ] {
            let inner = panel_block(kind, " t ", &theme, true, false).inner(area);
            // 无边框，仅标题占顶行
            assert_eq!(inner, ratatui::layout::Rect::new(0, 1, 10, 4));
            assert!(theme.panel_bg(kind, false).is_some());
        }
    }

    #[test]
    fn default_theme_uses_solid_fills() {
        let t = TuiChromeTheme::for_color_mode(true);
        assert!(t.fill.is_some());
        assert_ne!(
            t.panel_bg(TuiPanelKind::Chat, false),
            t.panel_bg(TuiPanelKind::NavLeft, false)
        );
    }

    #[test]
    fn no_color_mode_has_no_fill() {
        let t = TuiChromeTheme::for_color_mode(false);
        assert!(t.fill.is_none());
        assert!(t.panel_bg(TuiPanelKind::Chat, true).is_none());
    }
}
