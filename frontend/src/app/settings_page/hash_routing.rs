//! 设置页 URL hash 与分区导航（从 `settings_page` 拆出以降低 `SettingsPageView` 的 nloc 棘轮）。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsSection {
    Appearance,
    Llm,
    ExecutorLlm,
    Tools,
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
            "session" => Some(Self::Session),
            "shortcuts" => Some(Self::Shortcuts),
            _ => None,
        }
    }
}

pub(super) fn read_settings_section_from_hash() -> Option<SettingsSection> {
    let win = web_sys::window()?;
    let hash = win.location().hash().ok()?;
    let slug = hash
        .strip_prefix("#settings/")
        .or_else(|| hash.strip_prefix("#settings="))?;
    SettingsSection::from_slug(slug)
}

pub(super) fn write_settings_section_to_hash(section: SettingsSection) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let _ = win
        .location()
        .set_hash(&format!("settings/{}", section.slug()));
}

pub(super) fn clear_settings_hash_if_present() {
    let Some(win) = web_sys::window() else {
        return;
    };
    let Ok(hash) = win.location().hash() else {
        return;
    };
    if hash.starts_with("#settings/") || hash.starts_with("#settings=") {
        let _ = win.location().set_hash("");
    }
}

pub(super) fn settings_page_install_hashchange_listener(active_section: RwSignal<SettingsSection>) {
    Effect::new(move |_| {
        let h = window_event_listener(
            leptos::ev::hashchange,
            move |_ev: web_sys::HashChangeEvent| {
                let Some(section) = read_settings_section_from_hash() else {
                    return;
                };
                if active_section.get_untracked() != section {
                    active_section.set(section);
                }
            },
        );
        on_cleanup(move || h.remove());
    });
}
