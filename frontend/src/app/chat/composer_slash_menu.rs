//! Web composer：`/` skill 浮层状态与菜单组件。

use leptos::html::Textarea;
use leptos::prelude::*;

use crate::api::{SkillListItem, SkillsListData, fetch_skills};
use crate::i18n::{self, Locale};

/// 草稿是否处于「仅 `/` + 可选 id 前缀、尚无空白」的 skill 浮层态。
pub(super) fn slash_skill_prefix(draft: &str) -> Option<&str> {
    let s = draft.trim_start();
    let rest = s.strip_prefix('/')?;
    if rest.contains(|c: char| c.is_whitespace()) || rest.contains('/') {
        return None;
    }
    Some(rest)
}

fn filter_skills(skills: &[SkillListItem], prefix: &str) -> Vec<SkillListItem> {
    let p = prefix.to_ascii_lowercase();
    let mut out: Vec<SkillListItem> = skills
        .iter()
        .filter(|s| p.is_empty() || s.id.to_ascii_lowercase().starts_with(&p))
        .cloned()
        .collect();
    out.sort_by_key(|a| a.id.to_ascii_lowercase());
    out.truncate(24);
    out
}

pub(super) fn apply_skill_id(
    draft: RwSignal<String>,
    selected_idx: RwSignal<usize>,
    menu_dismissed: RwSignal<bool>,
    composer_input_ref: NodeRef<Textarea>,
    id: &str,
) {
    menu_dismissed.set(false);
    let next = format!("/{id} ");
    draft.set(next.clone());
    selected_idx.set(0);
    if let Some(ta) = composer_input_ref.get() {
        let _ = ta.focus();
        let len = next.chars().count() as u32;
        let _ = ta.set_selection_range(len, len);
    }
}

#[derive(Clone, Copy)]
pub(super) struct SlashMenuSignals {
    pub menu_open: Memo<bool>,
    pub filtered: Memo<Vec<SkillListItem>>,
    pub skills_cache: RwSignal<Option<SkillsListData>>,
    pub skills_loading: RwSignal<bool>,
    pub skills_err: RwSignal<Option<String>>,
    pub selected_idx: RwSignal<usize>,
    pub menu_dismissed: RwSignal<bool>,
}

/// 挂载 slash 菜单相关信号与 Effect（工作区失效、拉取目录、选中索引钳制）。
pub(super) fn install_slash_menu_effects(
    draft: RwSignal<String>,
    locale: RwSignal<Locale>,
    workspace_path: Memo<String>,
) -> SlashMenuSignals {
    let skills_cache = RwSignal::new(Option::<SkillsListData>::None);
    let skills_loading = RwSignal::new(false);
    let skills_err = RwSignal::new(Option::<String>::None);
    let selected_idx = RwSignal::new(0usize);
    let menu_dismissed = RwSignal::new(false);
    let bare_slash_fetched = RwSignal::new(false);

    let menu_open =
        Memo::new(move |_| slash_skill_prefix(&draft.get()).is_some() && !menu_dismissed.get());
    let filtered = Memo::new(move |_| {
        let draft_now = draft.get();
        let Some(prefix) = slash_skill_prefix(&draft_now) else {
            return Vec::<SkillListItem>::new();
        };
        let Some(cache) = skills_cache.get() else {
            return Vec::new();
        };
        if !cache.enabled {
            return Vec::new();
        }
        filter_skills(&cache.skills, prefix)
    });

    Effect::new(move |_| {
        let d = draft.get();
        if slash_skill_prefix(&d).is_none() {
            menu_dismissed.set(false);
        }
        if d.trim() != "/" {
            bare_slash_fetched.set(false);
        }
    });

    Effect::new(move |_| {
        let _ = workspace_path.get();
        skills_cache.set(None);
        skills_err.set(None);
        bare_slash_fetched.set(false);
    });

    Effect::new(move |_| {
        let n = filtered.get().len();
        let i = selected_idx.get_untracked();
        if n == 0 {
            selected_idx.set(0);
        } else if i >= n {
            selected_idx.set(n - 1);
        }
    });

    Effect::new(move |_| {
        if !menu_open.get() || skills_loading.get_untracked() {
            return;
        }
        let bare = draft.get().trim() == "/";
        let cache_empty = skills_cache.get_untracked().is_none();
        let refresh_bare = bare && !bare_slash_fetched.get_untracked();
        if !cache_empty && !refresh_bare {
            return;
        }
        if bare {
            bare_slash_fetched.set(true);
        }
        skills_loading.set(true);
        skills_err.set(None);
        let loc = locale.get_untracked();
        leptos::task::spawn_local(async move {
            match fetch_skills(loc).await {
                Ok(data) => {
                    if let Some(ref e) = data.error {
                        skills_err.set(Some(e.clone()));
                    } else {
                        skills_err.set(None);
                    }
                    skills_cache.set(Some(data));
                }
                Err(e) => skills_err.set(Some(e)),
            }
            skills_loading.set(false);
        });
    });

    SlashMenuSignals {
        menu_open,
        filtered,
        skills_cache,
        skills_loading,
        skills_err,
        selected_idx,
        menu_dismissed,
    }
}

/// 处理浮层打开时的键盘；返回 `true` 表示已消费事件（含空列表时的 Enter/Tab 防误发）。
pub(super) fn handle_slash_menu_keydown(
    ev: &web_sys::KeyboardEvent,
    slash: SlashMenuSignals,
    draft: RwSignal<String>,
    composer_input_ref: NodeRef<Textarea>,
) -> bool {
    if !slash.menu_open.get_untracked() {
        return false;
    }
    let key = ev.key();
    let items = slash.filtered.get_untracked();
    if key == "Escape" {
        ev.prevent_default();
        slash.menu_dismissed.set(true);
        return true;
    }
    if key == "ArrowDown" {
        ev.prevent_default();
        if !items.is_empty() {
            let n = items.len();
            slash.selected_idx.update(|i| *i = (*i + 1) % n);
        }
        return true;
    }
    if key == "ArrowUp" {
        ev.prevent_default();
        if !items.is_empty() {
            let n = items.len();
            slash
                .selected_idx
                .update(|i| *i = if *i == 0 { n - 1 } else { *i - 1 });
        }
        return true;
    }
    if key == "Tab" || (key == "Enter" && !ev.shift_key()) {
        ev.prevent_default();
        if let Some(item) = items.get(slash.selected_idx.get_untracked()) {
            apply_skill_id(
                draft,
                slash.selected_idx,
                slash.menu_dismissed,
                composer_input_ref,
                &item.id,
            );
        }
        return true;
    }
    false
}

#[component]
pub(super) fn ComposerSlashMenu(
    locale: RwSignal<Locale>,
    slash: SlashMenuSignals,
    draft: RwSignal<String>,
    composer_input_ref: NodeRef<Textarea>,
) -> impl IntoView {
    let menu_open = slash.menu_open;
    view! {
        <Show when=move || menu_open.get()>
            <div
                class="composer-slash-menu"
                role="listbox"
                prop:aria-label=move || i18n::composer_slash_menu_aria(locale.get())
            >
                {move || slash_menu_body(locale, slash, draft, composer_input_ref)}
            </div>
        </Show>
    }
}

fn slash_menu_body(
    locale: RwSignal<Locale>,
    slash: SlashMenuSignals,
    draft: RwSignal<String>,
    composer_input_ref: NodeRef<Textarea>,
) -> AnyView {
    let skills_loading = slash.skills_loading;
    let skills_cache = slash.skills_cache;
    let skills_err = slash.skills_err;
    let filtered = slash.filtered;
    let selected_idx = slash.selected_idx;
    let menu_dismissed = slash.menu_dismissed;

    if skills_loading.get() && skills_cache.get().is_none() {
        return view! {
            <div class="composer-slash-menu-empty">
                {move || i18n::composer_slash_menu_loading(locale.get())}
            </div>
        }
        .into_any();
    }
    if let Some(err) = skills_err.get() {
        return view! {
            <div class="composer-slash-menu-empty">{err}</div>
        }
        .into_any();
    }
    let items = filtered.get();
    if items.is_empty() {
        return view! {
            <div class="composer-slash-menu-empty">
                {move || i18n::composer_slash_menu_empty(locale.get())}
            </div>
        }
        .into_any();
    }
    let sel = selected_idx.get();
    items
        .into_iter()
        .enumerate()
        .map(|(i, item)| {
            let id = item.id.clone();
            let id_btn = id.clone();
            let desc = item.description.clone();
            let path = item.path.clone();
            let active = i == sel;
            view! {
                <button
                    type="button"
                    class="composer-slash-menu-item"
                    class:composer-slash-menu-item--active=active
                    role="option"
                    prop:aria-selected=active
                    on:mousedown=move |ev| {
                        ev.prevent_default();
                        apply_skill_id(
                            draft,
                            selected_idx,
                            menu_dismissed,
                            composer_input_ref,
                            &id_btn,
                        );
                    }
                >
                    <span class="composer-slash-menu-id">{format!("/{id}")}</span>
                    <span class="composer-slash-menu-desc">
                        {if desc.is_empty() { path } else { desc }}
                    </span>
                </button>
            }
        })
        .collect_view()
        .into_any()
}
