//! Web composer：`/` 浮层（内建命令 + skill）状态与菜单组件。

use leptos::html::Textarea;
use leptos::prelude::*;

use crate::api::{SkillListItem, SkillsListData, fetch_skills};
use crate::i18n::{self, Locale};

/// 草稿是否处于「仅 `/` + 可选前缀、尚无空白」的浮层态。
pub(super) fn slash_skill_prefix(draft: &str) -> Option<&str> {
    let s = draft.trim_start();
    let rest = s.strip_prefix('/')?;
    if rest.contains(|c: char| c.is_whitespace()) || rest.contains('/') {
        return None;
    }
    Some(rest)
}

/// IME 组字中：勿拦截方向键/Enter，也勿触发发送。
pub(super) fn keydown_is_ime_composing(ev: &web_sys::KeyboardEvent) -> bool {
    if ev.is_composing() {
        return true;
    }
    // 旧引擎在 composition 期间常报 keyCode 229。
    ev.key_code() == 229
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SlashItemKind {
    Builtin,
    Skill,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SlashMenuItem {
    pub kind: SlashItemKind,
    /// 前缀匹配键（不含 `/`）。
    pub match_key: String,
    /// 写入草稿的全文。
    pub insert: String,
    /// 左侧展示（含 `/`）。
    pub label: String,
    pub description: String,
}

fn web_builtin_slash_items(locale: Locale) -> Vec<SlashMenuItem> {
    vec![
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "help".to_string(),
            insert: "/help".to_string(),
            label: "/help".to_string(),
            description: i18n::composer_slash_builtin_help(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "workspace".to_string(),
            insert: "/workspace ".to_string(),
            label: "/workspace".to_string(),
            description: i18n::composer_slash_builtin_workspace(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "agent".to_string(),
            insert: "/agent ".to_string(),
            label: "/agent".to_string(),
            description: i18n::composer_slash_builtin_agent(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "model".to_string(),
            insert: "/model ".to_string(),
            label: "/model".to_string(),
            description: i18n::composer_slash_builtin_model(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "api-base".to_string(),
            insert: "/api-base ".to_string(),
            label: "/api-base".to_string(),
            description: i18n::composer_slash_builtin_api_base(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "export".to_string(),
            insert: "/export ".to_string(),
            label: "/export".to_string(),
            description: i18n::composer_slash_builtin_export(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "config".to_string(),
            insert: "/config".to_string(),
            label: "/config".to_string(),
            description: i18n::composer_slash_builtin_config(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "context".to_string(),
            insert: "/context".to_string(),
            label: "/context".to_string(),
            description: i18n::composer_slash_builtin_context(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "clear".to_string(),
            insert: "/clear".to_string(),
            label: "/clear".to_string(),
            description: i18n::composer_slash_builtin_clear(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "api-key".to_string(),
            insert: "/api-key ".to_string(),
            label: "/api-key".to_string(),
            description: i18n::composer_slash_builtin_api_key(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "skills".to_string(),
            insert: "/skills".to_string(),
            label: "/skills".to_string(),
            description: i18n::composer_slash_builtin_skills(locale).to_string(),
        },
        SlashMenuItem {
            kind: SlashItemKind::Builtin,
            match_key: "skills list".to_string(),
            insert: "/skills list".to_string(),
            label: "/skills list".to_string(),
            description: i18n::composer_slash_builtin_skills_list(locale).to_string(),
        },
    ]
}

fn item_matches_prefix(item: &SlashMenuItem, prefix: &str) -> bool {
    if prefix.is_empty() {
        return true;
    }
    let p = prefix.to_ascii_lowercase();
    item.match_key.to_ascii_lowercase().starts_with(&p)
        || item
            .label
            .trim_start_matches('/')
            .to_ascii_lowercase()
            .starts_with(&p)
}

fn skill_to_menu_item(s: &SkillListItem) -> SlashMenuItem {
    let desc = if s.description.trim().is_empty() {
        s.path.clone()
    } else {
        s.description.clone()
    };
    SlashMenuItem {
        kind: SlashItemKind::Skill,
        match_key: s.id.clone(),
        insert: format!("/{} ", s.id),
        label: format!("/{}", s.id),
        description: desc,
    }
}

fn build_filtered_menu(
    prefix: &str,
    locale: Locale,
    cache: Option<&SkillsListData>,
) -> Vec<SlashMenuItem> {
    let mut out: Vec<SlashMenuItem> = web_builtin_slash_items(locale)
        .into_iter()
        .filter(|it| item_matches_prefix(it, prefix))
        .collect();
    if let Some(cache) = cache
        && cache.enabled
    {
        let mut skills: Vec<SlashMenuItem> = cache
            .skills
            .iter()
            .filter(|s| {
                prefix.is_empty()
                    || s.id
                        .to_ascii_lowercase()
                        .starts_with(&prefix.to_ascii_lowercase())
            })
            .map(skill_to_menu_item)
            .collect();
        skills.sort_by(|a, b| {
            a.match_key
                .to_ascii_lowercase()
                .cmp(&b.match_key.to_ascii_lowercase())
        });
        skills.truncate(24);
        out.extend(skills);
    }
    out
}

pub(super) fn apply_slash_item(
    draft: RwSignal<String>,
    selected_idx: RwSignal<usize>,
    menu_dismissed: RwSignal<bool>,
    composer_input_ref: NodeRef<Textarea>,
    item: &SlashMenuItem,
) {
    let next = item.insert.clone();
    draft.set(next.clone());
    selected_idx.set(0);
    menu_dismissed.set(true);
    if let Some(ta) = composer_input_ref.get() {
        let _ = ta.focus();
        let len = next.chars().count() as u32;
        let _ = ta.set_selection_range(len, len);
    }
}

#[derive(Clone, Copy)]
pub(super) struct SlashMenuSignals {
    pub menu_open: Memo<bool>,
    pub filtered: Memo<Vec<SlashMenuItem>>,
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
            return Vec::<SlashMenuItem>::new();
        };
        let loc = locale.get();
        build_filtered_menu(prefix, loc, skills_cache.get().as_ref())
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
    if keydown_is_ime_composing(ev) || !slash.menu_open.get_untracked() {
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
            apply_slash_item(
                draft,
                slash.selected_idx,
                slash.menu_dismissed,
                composer_input_ref,
                item,
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

fn slash_empty_hint(locale: Locale, cache: Option<&SkillsListData>, loading: bool) -> &'static str {
    if loading && cache.is_none() {
        return i18n::composer_slash_menu_loading(locale);
    }
    if let Some(c) = cache
        && !c.enabled
    {
        return i18n::composer_slash_menu_skills_disabled(locale);
    }
    i18n::composer_slash_menu_empty(locale)
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

    let loc = locale.get();
    let cache = skills_cache.get();
    let loading = skills_loading.get();
    let err = skills_err.get();
    let items = filtered.get();

    // 内建命令可立刻展示；仅在「尚无任何可选项」时显示空态（加载中也先等一帧内建列表）。
    if items.is_empty() {
        let hint = slash_empty_hint(loc, cache.as_ref(), loading);
        return view! {
            <div class="composer-slash-menu-empty">{hint}</div>
        }
        .into_any();
    }

    let mut views: Vec<AnyView> = Vec::new();
    if let Some(err) = err {
        views.push(
            view! { <div class="composer-slash-menu-banner composer-slash-menu-banner--err">{err}</div> }
                .into_any(),
        );
    } else if loading && cache.is_none() {
        views.push(
            view! {
                <div class="composer-slash-menu-banner">
                    {i18n::composer_slash_menu_loading(loc)}
                </div>
            }
            .into_any(),
        );
    } else if let Some(ref c) = cache
        && !c.enabled
    {
        views.push(
            view! {
                <div class="composer-slash-menu-banner">
                    {i18n::composer_slash_menu_skills_disabled(loc)}
                </div>
            }
            .into_any(),
        );
    }

    let has_builtin = items.iter().any(|i| i.kind == SlashItemKind::Builtin);
    let has_skill = items.iter().any(|i| i.kind == SlashItemKind::Skill);
    let show_sections = has_builtin && has_skill;
    let sel = selected_idx.get();
    let mut last_kind: Option<SlashItemKind> = None;

    for (i, item) in items.into_iter().enumerate() {
        if show_sections && last_kind != Some(item.kind) {
            let title = match item.kind {
                SlashItemKind::Builtin => i18n::composer_slash_section_commands(loc),
                SlashItemKind::Skill => i18n::composer_slash_section_skills(loc),
            };
            views.push(
                view! { <div class="composer-slash-menu-section" role="presentation">{title}</div> }
                    .into_any(),
            );
            last_kind = Some(item.kind);
        }
        let active = i == sel;
        let item_btn = item.clone();
        let kind_class = match item.kind {
            SlashItemKind::Builtin => "composer-slash-menu-item composer-slash-menu-item--builtin",
            SlashItemKind::Skill => "composer-slash-menu-item composer-slash-menu-item--skill",
        };
        views.push(
            view! {
                <button
                    type="button"
                    class=kind_class
                    class:composer-slash-menu-item--active=active
                    role="option"
                    prop:aria-selected=active
                    on:mousedown=move |ev| {
                        ev.prevent_default();
                        apply_slash_item(
                            draft,
                            selected_idx,
                            menu_dismissed,
                            composer_input_ref,
                            &item_btn,
                        );
                    }
                >
                    <span class="composer-slash-menu-id">{item.label.clone()}</span>
                    <span class="composer-slash-menu-desc">{item.description.clone()}</span>
                </button>
            }
            .into_any(),
        );
    }

    views.collect_view().into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slash_prefix_rejects_whitespace_and_extra_slash() {
        assert_eq!(slash_skill_prefix("/"), Some(""));
        assert_eq!(slash_skill_prefix("/skills"), Some("skills"));
        assert_eq!(slash_skill_prefix("  /ab"), Some("ab"));
        assert!(slash_skill_prefix("/skills list").is_none());
        assert!(slash_skill_prefix("/a/b").is_none());
        assert!(slash_skill_prefix("hi").is_none());
    }

    #[test]
    fn builtins_always_available_without_cache() {
        let items = build_filtered_menu("", Locale::ZhHans, None);
        assert!(items.iter().any(|i| i.insert == "/skills"));
        assert!(items.iter().any(|i| i.insert == "/skills list"));
        assert!(items.iter().any(|i| i.insert == "/help"));
        assert!(items.iter().any(|i| i.insert == "/context"));
        assert!(items.iter().any(|i| i.insert.starts_with("/workspace")));
        assert!(items.iter().all(|i| i.kind == SlashItemKind::Builtin));
    }

    #[test]
    fn filter_prefix_matches_builtin_and_skill() {
        let cache = SkillsListData {
            enabled: true,
            skills_dir: ".crabmate/skills".into(),
            skills_user_dir: String::new(),
            skills_system_dir: String::new(),
            skills: vec![SkillListItem {
                id: "cpp-programming".into(),
                name: None,
                description: "C++ tips".into(),
                path: "x".into(),
            }],
            error: None,
        };
        let items = build_filtered_menu("sk", Locale::En, Some(&cache));
        assert!(items.iter().any(|i| i.insert == "/skills"));
        assert!(items.iter().any(|i| i.insert == "/skills list"));
        assert!(!items.iter().any(|i| i.kind == SlashItemKind::Skill));

        let items = build_filtered_menu("cpp", Locale::En, Some(&cache));
        assert!(items.iter().all(|i| i.kind == SlashItemKind::Skill));
        assert_eq!(items[0].insert, "/cpp-programming ");
    }

    #[test]
    fn disabled_skills_omit_skill_rows() {
        let cache = SkillsListData {
            enabled: false,
            skills_dir: ".crabmate/skills".into(),
            skills_user_dir: String::new(),
            skills_system_dir: String::new(),
            skills: vec![SkillListItem {
                id: "x".into(),
                name: None,
                description: String::new(),
                path: "x".into(),
            }],
            error: None,
        };
        let items = build_filtered_menu("", Locale::En, Some(&cache));
        assert!(items.iter().all(|i| i.kind == SlashItemKind::Builtin));
    }
}
