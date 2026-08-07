//! 设置页 URL hash 与分区导航（`#/settings` / `#/settings/<section>`）。
//!
//! 兼容旧式 `#settings/<section>`；关闭设置时回到 `#/`。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Appearance,
    Llm,
    ExecutorLlm,
    Tools,
    Github,
    Mcp,
    Session,
    Shortcuts,
}

impl SettingsSection {
    pub(super) fn slug(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Llm => "llm",
            Self::ExecutorLlm => "executor-llm",
            Self::Tools => "tools",
            Self::Github => "github",
            Self::Mcp => "mcp",
            Self::Session => "session",
            Self::Shortcuts => "shortcuts",
        }
    }

    pub(super) fn from_slug(s: &str) -> Option<Self> {
        match s {
            "appearance" => Some(Self::Appearance),
            "llm" => Some(Self::Llm),
            "executor-llm" => Some(Self::ExecutorLlm),
            "tools" => Some(Self::Tools),
            "github" => Some(Self::Github),
            "mcp" => Some(Self::Mcp),
            "session" => Some(Self::Session),
            "shortcuts" => Some(Self::Shortcuts),
            _ => None,
        }
    }
}

/// 去掉开头的 `#`，得到 hash 路径主体。
fn hash_body(hash: &str) -> &str {
    hash.strip_prefix('#').unwrap_or(hash)
}

/// 是否为设置页路由（含旧式 `#settings/...`）。
pub(crate) fn is_settings_hash(hash: &str) -> bool {
    parse_settings_route(hash).is_some()
}

/// 解析设置路由：`Some(section)`；纯 `#/settings` 时 section 默认 Appearance。
/// 非设置路由返回 `None`。
pub(crate) fn parse_settings_route(hash: &str) -> Option<SettingsSection> {
    let body = hash_body(hash).trim();
    if body.is_empty() {
        return None;
    }
    // `#/settings` | `#/settings/` | `#/settings/appearance`
    if let Some(rest) = body.strip_prefix("/settings") {
        return Some(section_from_settings_rest(rest));
    }
    // 旧式 `#settings` | `#settings/` | `#settings/appearance` | `#settings=appearance`
    if let Some(rest) = body.strip_prefix("settings") {
        if let Some(slug) = rest.strip_prefix('=') {
            return Some(SettingsSection::from_slug(slug).unwrap_or(SettingsSection::Appearance));
        }
        return Some(section_from_settings_rest(rest));
    }
    None
}

fn section_from_settings_rest(rest: &str) -> SettingsSection {
    let slug = rest.trim_start_matches('/').trim();
    if slug.is_empty() {
        SettingsSection::Appearance
    } else {
        SettingsSection::from_slug(slug).unwrap_or(SettingsSection::Appearance)
    }
}

pub(super) fn read_settings_section_from_hash() -> Option<SettingsSection> {
    let win = web_sys::window()?;
    let hash = win.location().hash().ok()?;
    parse_settings_route(&hash)
}

pub(super) fn write_settings_section_to_hash(section: SettingsSection) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let target = format!("/settings/{}", section.slug());
    let Ok(cur) = win.location().hash() else {
        return;
    };
    if hash_body(&cur) == target.trim_start_matches('#') || hash_body(&cur) == target.as_str() {
        return;
    }
    // `set_hash` 会自动加 `#`；传入 `/settings/...` → `#/settings/...`
    let _ = win.location().set_hash(&target);
}

/// 打开设置页并写入 `#/settings/<section>`（工具栏等入口）。
pub(crate) fn navigate_to_settings(settings_page: RwSignal<bool>, section: SettingsSection) {
    settings_page.set(true);
    write_settings_section_to_hash(section);
}

/// 关闭设置页并回到聊天路由 `#/`。
pub(crate) fn navigate_to_chat(settings_page: RwSignal<bool>) {
    settings_page.set(false);
    clear_settings_hash_if_present();
}

pub(super) fn clear_settings_hash_if_present() {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(hash) = win.location().hash() else {
        return;
    };
    if !is_settings_hash(&hash) {
        return;
    }
    let _ = win.location().set_hash("/");
}

/// 监听 hash：进入/离开 `#/settings` 时同步 `settings_page`，并更新分区。
pub(super) fn settings_page_install_hashchange_listener(
    settings_page: RwSignal<bool>,
    active_section: RwSignal<SettingsSection>,
) {
    // 首屏：若 URL 已是设置路由则打开页面
    Effect::new(move |_| {
        if let Some(section) = read_settings_section_from_hash() {
            active_section.set(section);
            if !settings_page.get_untracked() {
                settings_page.set(true);
            }
        }
    });

    Effect::new(move |_| {
        let h = window_event_listener(
            leptos::ev::hashchange,
            move |_ev: web_sys::HashChangeEvent| {
                let Some(win) = web_sys::window() else {
                    return;
                };
                let Ok(hash) = win.location().hash() else {
                    return;
                };
                if let Some(section) = parse_settings_route(&hash) {
                    if active_section.get_untracked() != section {
                        active_section.set(section);
                    }
                    if !settings_page.get_untracked() {
                        settings_page.set(true);
                    }
                } else if settings_page.get_untracked() {
                    settings_page.set(false);
                }
            },
        );
        on_cleanup(move || h.remove());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_hash_slash_settings_routes() {
        assert_eq!(
            parse_settings_route("#/settings"),
            Some(SettingsSection::Appearance)
        );
        assert_eq!(
            parse_settings_route("#/settings/"),
            Some(SettingsSection::Appearance)
        );
        assert_eq!(
            parse_settings_route("#/settings/mcp"),
            Some(SettingsSection::Mcp)
        );
        assert_eq!(
            parse_settings_route("#/settings/executor-llm"),
            Some(SettingsSection::ExecutorLlm)
        );
        assert_eq!(
            parse_settings_route("#/settings/github"),
            Some(SettingsSection::Github)
        );
        assert!(parse_settings_route("#/").is_none());
        assert!(parse_settings_route("").is_none());
        assert!(parse_settings_route("#/chat").is_none());
    }

    #[test]
    fn parse_legacy_settings_hashes() {
        assert_eq!(
            parse_settings_route("#settings/appearance"),
            Some(SettingsSection::Appearance)
        );
        assert_eq!(
            parse_settings_route("#settings=mcp"),
            Some(SettingsSection::Mcp)
        );
    }
}
