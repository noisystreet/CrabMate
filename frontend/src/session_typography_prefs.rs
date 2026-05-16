//! 会话模式 Web UI：界面字体与聊天消息字体的本机偏好（`localStorage` + `--crabmate-*` CSS 变量）。

use crate::app_prefs::{SESSION_CHAT_FONT_KEY, SESSION_UI_FONT_KEY, local_storage};

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

#[must_use]
pub fn read_session_ui_font_initial() -> String {
    local_storage()
        .and_then(|st| st.get_item(SESSION_UI_FONT_KEY).ok().flatten())
        .map(|s| normalize_session_ui_font(&s))
        .unwrap_or_else(|| DEFAULT_SESSION_UI_FONT.to_string())
}

#[must_use]
pub fn read_session_chat_font_initial() -> String {
    local_storage()
        .and_then(|st| st.get_item(SESSION_CHAT_FONT_KEY).ok().flatten())
        .map(|s| normalize_session_chat_font(&s))
        .unwrap_or_else(|| DEFAULT_SESSION_CHAT_FONT.to_string())
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
}
