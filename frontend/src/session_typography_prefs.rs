//! 会话模式 Web UI：界面字体、聊天消息字体与字号（slug / clamp + `--crabmate-*` CSS 变量）。

/// 界面字体（侧栏、顶栏、设置页等）可选 slug。
pub const SESSION_UI_FONT_SLUGS: &[&str] = &["default", "dm_sans", "system", "roboto", "serif"];

/// 聊天列气泡与输入框正文字体可选 slug（代码块等仍用 `--font-mono`）。
pub const SESSION_CHAT_FONT_SLUGS: &[&str] = &[
    "default",
    "dm_sans",
    "system",
    "roboto",
    "serif",
    "jetbrains",
    "mono_system",
];

pub const DEFAULT_SESSION_UI_FONT: &str = "default";
pub const DEFAULT_SESSION_CHAT_FONT: &str = "default";
/// 聊天气泡与输入框正文字号默认（px），与 transcript / composer 历史基准一致。
pub const DEFAULT_SESSION_CHAT_FONT_SIZE: f64 = 14.0;
pub const SESSION_CHAT_FONT_SIZE_MIN: f64 = 10.0;
pub const SESSION_CHAT_FONT_SIZE_MAX: f64 = 28.0;

#[must_use]
pub fn normalize_session_ui_font(raw: &str) -> String {
    let t = raw.trim();
    if SESSION_UI_FONT_SLUGS.contains(&t) {
        t.to_string()
    } else {
        DEFAULT_SESSION_UI_FONT.to_string()
    }
}

#[must_use]
pub fn normalize_session_chat_font(raw: &str) -> String {
    let t = raw.trim();
    if SESSION_CHAT_FONT_SLUGS.contains(&t) {
        t.to_string()
    } else {
        DEFAULT_SESSION_CHAT_FONT.to_string()
    }
}

/// 将聊天区字号夹到允许范围（与设置页 number 输入一致）。
#[must_use]
pub fn clamp_session_chat_font_size(px: f64) -> f64 {
    if !px.is_finite() {
        return DEFAULT_SESSION_CHAT_FONT_SIZE;
    }
    px.round()
        .clamp(SESSION_CHAT_FONT_SIZE_MIN, SESSION_CHAT_FONT_SIZE_MAX)
}

/// `None` 表示使用主题默认（不设置自定义属性）。
#[must_use]
pub fn session_ui_font_stack_css(slug: &str) -> Option<&'static str> {
    Some(match normalize_session_ui_font(slug).as_str() {
        "default" => return None,
        "dm_sans" => {
            "\"DM Sans\", ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif"
        }
        "system" => {
            "ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif"
        }
        "roboto" => {
            "\"Roboto\", \"DM Sans\", ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", \"Helvetica Neue\", Arial, sans-serif"
        }
        "serif" => "Georgia, \"Times New Roman\", \"Noto Serif\", ui-serif, serif",
        _ => return None,
    })
}

#[must_use]
pub fn session_chat_font_stack_css(slug: &str) -> Option<&'static str> {
    Some(match normalize_session_chat_font(slug).as_str() {
        "default" => return None,
        "dm_sans" => {
            "\"DM Sans\", ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif"
        }
        "system" => {
            "ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", Roboto, \"Helvetica Neue\", Arial, sans-serif"
        }
        "roboto" => {
            "\"Roboto\", \"DM Sans\", ui-sans-serif, system-ui, -apple-system, \"Segoe UI\", \"Helvetica Neue\", Arial, sans-serif"
        }
        "serif" => "Georgia, \"Times New Roman\", \"Noto Serif\", ui-serif, serif",
        "jetbrains" => "\"JetBrains Mono\", ui-monospace, \"Cascadia Code\", monospace",
        "mono_system" => "ui-monospace, monospace",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_ui_font_slug_falls_back() {
        assert_eq!(
            normalize_session_ui_font("nope").as_str(),
            DEFAULT_SESSION_UI_FONT
        );
    }

    #[test]
    fn unknown_chat_font_slug_falls_back() {
        assert_eq!(
            normalize_session_chat_font("").as_str(),
            DEFAULT_SESSION_CHAT_FONT
        );
    }

    #[test]
    fn chat_font_size_clamps_and_rounds() {
        assert_eq!(clamp_session_chat_font_size(14.4), 14.0);
        assert_eq!(
            clamp_session_chat_font_size(9.0),
            SESSION_CHAT_FONT_SIZE_MIN
        );
        assert_eq!(
            clamp_session_chat_font_size(40.0),
            SESSION_CHAT_FONT_SIZE_MAX
        );
        assert_eq!(
            clamp_session_chat_font_size(f64::NAN),
            DEFAULT_SESSION_CHAT_FONT_SIZE
        );
    }
}
