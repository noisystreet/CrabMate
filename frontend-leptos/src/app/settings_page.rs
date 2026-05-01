//! 设置页面组件：替代原有的对话框模式。

use leptos::prelude::*;
use leptos_dom::helpers::window_event_listener;

use super::settings_form_state::{SettingsFormCurrent, is_settings_dirty, refresh_baselines};
use super::settings_sections::{
    SettingsAppearanceBlock, SettingsExecutorLlmBlock, SettingsLlmBlock, SettingsShortcutsBlock,
};
use crate::app::settings_commit::{CommitAllSettingsInput, commit_all_settings};
use crate::i18n::{self, Locale};

/// 设置页中与 LLM / 外观相关的 `RwSignal` 聚合（缩短 `SettingsPageView` 形参列表）。
#[derive(Clone, Copy)]
pub struct SettingsPageFormSignals {
    pub locale: RwSignal<Locale>,
    pub theme: RwSignal<String>,
    pub bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub llm_settings_feedback: RwSignal<Option<String>>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_settings_feedback: RwSignal<Option<String>>,
    pub execution_mode_draft: RwSignal<String>,
    pub client_llm_storage_tick: RwSignal<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSection {
    Appearance,
    Llm,
    ExecutorLlm,
    Shortcuts,
}

impl SettingsSection {
    fn slug(self) -> &'static str {
        match self {
            Self::Appearance => "appearance",
            Self::Llm => "llm",
            Self::ExecutorLlm => "executor-llm",
            Self::Shortcuts => "shortcuts",
        }
    }

    fn from_slug(s: &str) -> Option<Self> {
        match s {
            "appearance" => Some(Self::Appearance),
            "llm" => Some(Self::Llm),
            "executor-llm" => Some(Self::ExecutorLlm),
            "shortcuts" => Some(Self::Shortcuts),
            _ => None,
        }
    }
}

fn read_settings_section_from_hash() -> Option<SettingsSection> {
    let win = web_sys::window()?;
    let hash = win.location().hash().ok()?;
    let slug = if let Some(v) = hash.strip_prefix("#settings/") {
        v
    } else if let Some(v) = hash.strip_prefix("#settings=") {
        // Backward compatibility with previous hash format.
        v
    } else {
        return None;
    };
    SettingsSection::from_slug(slug)
}

fn write_settings_section_to_hash(section: SettingsSection) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let _ = win
        .location()
        .set_hash(&format!("settings/{}", section.slug()));
}

fn clear_settings_hash_if_present() {
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

fn section_title(section: SettingsSection, locale: Locale) -> &'static str {
    match section {
        SettingsSection::Appearance => i18n::settings_section_appearance_title(locale),
        SettingsSection::Llm => i18n::settings_section_llm_title(locale),
        SettingsSection::ExecutorLlm => i18n::settings_section_executor_llm_title(locale),
        SettingsSection::Shortcuts => i18n::settings_section_shortcuts_title(locale),
    }
}

fn section_desc(section: SettingsSection, locale: Locale) -> &'static str {
    match section {
        SettingsSection::Appearance => i18n::settings_section_appearance_desc(locale),
        SettingsSection::Llm => i18n::settings_section_llm_desc(locale),
        SettingsSection::ExecutorLlm => i18n::settings_section_executor_llm_desc(locale),
        SettingsSection::Shortcuts => i18n::settings_section_shortcuts_desc(locale),
    }
}

fn apply_theme_preview_to_dom(theme: &str) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(root) = doc.document_element()
    {
        let _ = root.set_attribute("data-theme", theme);
    }
}

fn apply_bg_decor_preview_to_dom(bg_decor: bool) {
    if let Some(doc) = web_sys::window().and_then(|w| w.document())
        && let Some(root) = doc.document_element()
    {
        if bg_decor {
            let _ = root.remove_attribute("data-bg-decor");
        } else {
            let _ = root.set_attribute("data-bg-decor", "plain");
        }
    }
}

fn settings_page_install_hashchange_listener(active_section: RwSignal<SettingsSection>) {
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

#[component]
fn SettingsPageNavRail(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
) -> impl IntoView {
    view! {
        <nav class="settings-nav" prop:aria-label=move || i18n::settings_nav_aria(appearance_locale.get())>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Appearance
                on:click=move |_| {
                    active_section.set(SettingsSection::Appearance);
                    write_settings_section_to_hash(SettingsSection::Appearance);
                }
            >
                {move || i18n::settings_section_appearance_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Llm
                on:click=move |_| {
                    active_section.set(SettingsSection::Llm);
                    write_settings_section_to_hash(SettingsSection::Llm);
                }
            >
                {move || i18n::settings_section_llm_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::ExecutorLlm
                on:click=move |_| {
                    active_section.set(SettingsSection::ExecutorLlm);
                    write_settings_section_to_hash(SettingsSection::ExecutorLlm);
                }
            >
                {move || i18n::settings_section_executor_llm_title(appearance_locale.get())}
            </button>
            <button
                type="button"
                class="settings-nav-item"
                class:active=move || active_section.get() == SettingsSection::Shortcuts
                on:click=move |_| {
                    active_section.set(SettingsSection::Shortcuts);
                    write_settings_section_to_hash(SettingsSection::Shortcuts);
                }
            >
                {move || i18n::settings_section_shortcuts_title(appearance_locale.get())}
            </button>
        </nav>
    }
}

/// 设置内容区各块共用的草稿信号（缩短 `SettingsPageContentPanels` 形参列表）。
#[derive(Clone, Copy)]
pub struct SettingsPagePanelDrafts {
    pub appearance_theme: RwSignal<String>,
    pub appearance_bg_decor: RwSignal<bool>,
    pub llm_api_base_draft: RwSignal<String>,
    pub llm_api_base_preset_select: RwSignal<String>,
    pub llm_model_draft: RwSignal<String>,
    pub llm_temperature_draft: RwSignal<String>,
    pub llm_context_tokens_draft: RwSignal<String>,
    pub llm_api_key_draft: RwSignal<String>,
    pub llm_has_saved_key: RwSignal<bool>,
    pub executor_llm_api_base_draft: RwSignal<String>,
    pub executor_llm_api_base_preset_select: RwSignal<String>,
    pub executor_llm_model_draft: RwSignal<String>,
    pub executor_llm_api_key_draft: RwSignal<String>,
    pub executor_llm_has_saved_key: RwSignal<bool>,
}

#[component]
fn SettingsPageContentPanels(
    active_section: RwSignal<SettingsSection>,
    appearance_locale: RwSignal<Locale>,
    drafts: SettingsPagePanelDrafts,
    clear_client_key_intent: RwSignal<bool>,
    clear_executor_key_intent: RwSignal<bool>,
    execution_mode_draft: RwSignal<String>,
) -> impl IntoView {
    let SettingsPagePanelDrafts {
        appearance_theme,
        appearance_bg_decor,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_api_key_draft,
        executor_llm_has_saved_key,
    } = drafts;

    view! {
        <section class="settings-content">
            <header class="settings-content-header">
                <h2 class="settings-content-title">{move || section_title(active_section.get(), appearance_locale.get())}</h2>
                <p class="settings-content-desc">{move || section_desc(active_section.get(), appearance_locale.get())}</p>
            </header>
            <Show when=move || active_section.get() == SettingsSection::Appearance>
                <SettingsAppearanceBlock
                    locale=appearance_locale
                    appearance_locale=appearance_locale
                    appearance_theme=appearance_theme
                    appearance_bg_decor=appearance_bg_decor
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Llm>
                <SettingsLlmBlock
                    locale=appearance_locale
                    llm_api_base_draft=llm_api_base_draft
                    llm_api_base_preset_select=llm_api_base_preset_select
                    llm_model_draft=llm_model_draft
                    llm_temperature_draft=llm_temperature_draft
                    llm_context_tokens_draft=llm_context_tokens_draft
                    execution_mode_draft=Some(execution_mode_draft)
                    llm_api_key_draft=llm_api_key_draft
                    llm_has_saved_key=llm_has_saved_key
                    clear_client_key_intent=clear_client_key_intent
                    hint_class="settings-field-nested-hint"
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::ExecutorLlm>
                <SettingsExecutorLlmBlock
                    locale=appearance_locale
                    executor_llm_api_base_draft=executor_llm_api_base_draft
                    executor_llm_api_base_preset_select=executor_llm_api_base_preset_select
                    executor_llm_model_draft=executor_llm_model_draft
                    executor_llm_api_key_draft=executor_llm_api_key_draft
                    executor_llm_has_saved_key=executor_llm_has_saved_key
                    clear_executor_key_intent=clear_executor_key_intent
                    hint_class="settings-field-nested-hint"
                />
            </Show>

            <Show when=move || active_section.get() == SettingsSection::Shortcuts>
                <SettingsShortcutsBlock
                    locale=appearance_locale
                    body_class="settings-intro"
                />
            </Show>
        </section>
    }
}

#[component]
pub fn SettingsPageView(
    settings_page: RwSignal<bool>,
    form: SettingsPageFormSignals,
) -> impl IntoView {
    let SettingsPageFormSignals {
        locale,
        theme,
        bg_decor,
        llm_api_base_draft,
        llm_api_base_preset_select,
        llm_model_draft,
        llm_temperature_draft,
        llm_context_tokens_draft,
        llm_api_key_draft,
        llm_has_saved_key,
        llm_settings_feedback,
        executor_llm_api_base_draft,
        executor_llm_api_base_preset_select,
        executor_llm_model_draft,
        executor_llm_api_key_draft,
        executor_llm_has_saved_key,
        executor_llm_settings_feedback,
        execution_mode_draft,
        client_llm_storage_tick,
    } = form;

    let active_section =
        RwSignal::new(read_settings_section_from_hash().unwrap_or(SettingsSection::Appearance));

    let appearance_locale = RwSignal::new(locale.get_untracked());
    let appearance_theme = RwSignal::new(theme.get_untracked());
    let appearance_bg_decor = RwSignal::new(bg_decor.get_untracked());

    let baseline_appearance = StoredValue::new((
        locale.get_untracked(),
        theme.get_untracked(),
        bg_decor.get_untracked(),
    ));
    let baseline_llm = StoredValue::new((
        llm_api_base_draft.get_untracked(),
        llm_api_base_preset_select.get_untracked(),
        llm_model_draft.get_untracked(),
        llm_temperature_draft.get_untracked(),
        llm_context_tokens_draft.get_untracked(),
        execution_mode_draft.get_untracked(),
        llm_has_saved_key.get_untracked(),
    ));
    let baseline_executor = StoredValue::new((
        executor_llm_api_base_draft.get_untracked(),
        executor_llm_api_base_preset_select.get_untracked(),
        executor_llm_model_draft.get_untracked(),
        executor_llm_has_saved_key.get_untracked(),
    ));

    let clear_client_key_intent = RwSignal::new(false);
    let clear_executor_key_intent = RwSignal::new(false);

    let current_state_untracked = move || SettingsFormCurrent {
        appearance_locale: appearance_locale.get_untracked(),
        appearance_theme: appearance_theme.get_untracked(),
        appearance_bg_decor: appearance_bg_decor.get_untracked(),
        llm_api_base_draft: llm_api_base_draft.get_untracked(),
        llm_api_base_preset_select: llm_api_base_preset_select.get_untracked(),
        llm_model_draft: llm_model_draft.get_untracked(),
        llm_temperature_draft: llm_temperature_draft.get_untracked(),
        llm_context_tokens_draft: llm_context_tokens_draft.get_untracked(),
        execution_mode_draft: execution_mode_draft.get_untracked(),
        llm_has_saved_key: llm_has_saved_key.get_untracked(),
        executor_llm_api_base_draft: executor_llm_api_base_draft.get_untracked(),
        executor_llm_api_base_preset_select: executor_llm_api_base_preset_select.get_untracked(),
        executor_llm_model_draft: executor_llm_model_draft.get_untracked(),
        executor_llm_has_saved_key: executor_llm_has_saved_key.get_untracked(),
        clear_client_key_intent: clear_client_key_intent.get_untracked(),
        clear_executor_key_intent: clear_executor_key_intent.get_untracked(),
        llm_api_key_draft: llm_api_key_draft.get_untracked(),
        executor_llm_api_key_draft: executor_llm_api_key_draft.get_untracked(),
    };

    settings_page_install_hashchange_listener(active_section);

    Effect::new(move |_| {
        if !settings_page.get() {
            clear_settings_hash_if_present();
            return;
        }
        write_settings_section_to_hash(active_section.get());
    });

    Effect::new(move |_| {
        if !settings_page.get() {
            return;
        }
        appearance_locale.set(locale.get_untracked());
        appearance_theme.set(theme.get_untracked());
        appearance_bg_decor.set(bg_decor.get_untracked());
        refresh_baselines(
            baseline_appearance,
            baseline_llm,
            baseline_executor,
            &current_state_untracked(),
        );
        clear_client_key_intent.set(false);
        clear_executor_key_intent.set(false);
        llm_settings_feedback.set(None);
        executor_llm_settings_feedback.set(None);
    });

    Effect::new(move |_| {
        if !settings_page.get() {
            apply_theme_preview_to_dom(theme.get().as_str());
            apply_bg_decor_preview_to_dom(bg_decor.get());
            return;
        }
        apply_theme_preview_to_dom(appearance_theme.get().as_str());
        apply_bg_decor_preview_to_dom(appearance_bg_decor.get());
    });

    let dirty = Memo::new(move |_| {
        let current = SettingsFormCurrent {
            appearance_locale: appearance_locale.get(),
            appearance_theme: appearance_theme.get(),
            appearance_bg_decor: appearance_bg_decor.get(),
            llm_api_base_draft: llm_api_base_draft.get(),
            llm_api_base_preset_select: llm_api_base_preset_select.get(),
            llm_model_draft: llm_model_draft.get(),
            llm_temperature_draft: llm_temperature_draft.get(),
            llm_context_tokens_draft: llm_context_tokens_draft.get(),
            execution_mode_draft: execution_mode_draft.get(),
            llm_has_saved_key: llm_has_saved_key.get(),
            executor_llm_api_base_draft: executor_llm_api_base_draft.get(),
            executor_llm_api_base_preset_select: executor_llm_api_base_preset_select.get(),
            executor_llm_model_draft: executor_llm_model_draft.get(),
            executor_llm_has_saved_key: executor_llm_has_saved_key.get(),
            clear_client_key_intent: clear_client_key_intent.get(),
            clear_executor_key_intent: clear_executor_key_intent.get(),
            llm_api_key_draft: llm_api_key_draft.get(),
            executor_llm_api_key_draft: executor_llm_api_key_draft.get(),
        };
        is_settings_dirty(
            &current,
            &baseline_appearance.get_value(),
            &baseline_llm.get_value(),
            &baseline_executor.get_value(),
        )
    });

    let discard = move |_| {
        let (bl, bt, bbd) = baseline_appearance.get_value();
        appearance_locale.set(bl);
        appearance_theme.set(bt);
        appearance_bg_decor.set(bbd);

        let (bb, bp, bm, bt, bct, be, bh) = baseline_llm.get_value();
        llm_api_base_draft.set(bb);
        llm_api_base_preset_select.set(bp);
        llm_model_draft.set(bm);
        llm_temperature_draft.set(bt);
        llm_context_tokens_draft.set(bct);
        execution_mode_draft.set(be);
        llm_has_saved_key.set(bh);
        llm_api_key_draft.set(String::new());

        let (eb, ep, em, eh) = baseline_executor.get_value();
        executor_llm_api_base_draft.set(eb);
        executor_llm_api_base_preset_select.set(ep);
        executor_llm_model_draft.set(em);
        executor_llm_has_saved_key.set(eh);
        executor_llm_api_key_draft.set(String::new());

        clear_client_key_intent.set(false);
        clear_executor_key_intent.set(false);
        llm_settings_feedback.set(None);
        executor_llm_settings_feedback.set(None);
    };

    let save_all = move |_| {
        llm_settings_feedback.set(None);
        executor_llm_settings_feedback.set(None);
        if !dirty.get() {
            llm_settings_feedback.set(Some(
                i18n::settings_nothing_to_save(locale.get()).to_string(),
            ));
            return;
        }
        let ui_locale = appearance_locale.get();
        match commit_all_settings(CommitAllSettingsInput {
            ui_locale,
            appearance_locale: appearance_locale.get(),
            appearance_theme: appearance_theme.get(),
            appearance_bg_decor: appearance_bg_decor.get(),
            locale,
            theme,
            bg_decor,
            client_base: llm_api_base_draft.get().as_str(),
            client_model: llm_model_draft.get().as_str(),
            client_temperature: llm_temperature_draft.get().as_str(),
            client_llm_context_tokens: llm_context_tokens_draft.get().as_str(),
            client_api_key_draft: llm_api_key_draft.get().as_str(),
            executor_base: executor_llm_api_base_draft.get().as_str(),
            executor_model: executor_llm_model_draft.get().as_str(),
            executor_api_key_draft: executor_llm_api_key_draft.get().as_str(),
            execution_mode: execution_mode_draft.get().as_str(),
            clear_client_llm_key: clear_client_key_intent.get(),
            clear_executor_llm_key: clear_executor_key_intent.get(),
            llm_api_key_draft,
            llm_has_saved_key,
            executor_llm_api_key_draft,
            executor_llm_has_saved_key,
            client_llm_storage_tick,
        }) {
            Ok(()) => {
                refresh_baselines(
                    baseline_appearance,
                    baseline_llm,
                    baseline_executor,
                    &current_state_untracked(),
                );
                clear_client_key_intent.set(false);
                clear_executor_key_intent.set(false);
                llm_settings_feedback.set(Some(i18n::settings_save_all_ok(ui_locale).to_string()));
            }
            Err(e) => llm_settings_feedback.set(Some(e)),
        }
    };

    view! {
        <div class="settings-page" class:settings-page-visible=move || settings_page.get()>
            <div class="settings-page-header">
                <button
                    type="button"
                    class="btn btn-ghost settings-page-back"
                    on:click=move |_| {
                        if dirty.get() {
                            discard(());
                        }
                        settings_page.set(false);
                    }
                >
                    <svg
                        viewBox="0 0 24 24"
                        fill="none"
                        stroke="currentColor"
                        stroke-width="2"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                        aria-hidden="true"
                    >
                        <polyline points="15 18 9 12 15 6" />
                    </svg>
                    <span>{move || i18n::settings_back(appearance_locale.get())}</span>
                </button>
                <h1 class="settings-page-title">{move || i18n::settings_title(appearance_locale.get())}</h1>
                <span class="settings-page-badge">{move || i18n::settings_badge_local(appearance_locale.get())}</span>
                <Show when=move || dirty.get()>
                    <span class="settings-unsaved-pill">{move || i18n::settings_unsaved_badge(appearance_locale.get())}</span>
                </Show>
                <span class="settings-page-head-spacer"></span>
                <div class="settings-page-header-actions">
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm"
                        prop:disabled=move || !dirty.get()
                        on:click=move |_| discard(())
                    >
                        {move || i18n::settings_discard_changes(appearance_locale.get())}
                    </button>
                    <button
                        type="button"
                        class="btn btn-primary btn-sm"
                        prop:disabled=move || !dirty.get()
                        on:click=move |_| save_all(())
                    >
                        {move || i18n::settings_save_all(appearance_locale.get())}
                    </button>
                </div>
            </div>
            <div class="settings-page-body">
                <p class="settings-intro">{move || i18n::settings_intro(appearance_locale.get())}</p>
                <Show when=move || llm_settings_feedback.get().is_some()>
                    <p class="settings-save-feedback settings-save-feedback-global">{move || {
                        llm_settings_feedback.get().unwrap_or_default()
                    }}</p>
                </Show>
                <div class="settings-layout">
                    <SettingsPageNavRail active_section=active_section appearance_locale=appearance_locale />
                    <SettingsPageContentPanels
                        active_section=active_section
                        appearance_locale=appearance_locale
                        drafts=SettingsPagePanelDrafts {
                            appearance_theme,
                            appearance_bg_decor,
                            llm_api_base_draft,
                            llm_api_base_preset_select,
                            llm_model_draft,
                            llm_temperature_draft,
                            llm_context_tokens_draft,
                            llm_api_key_draft,
                            llm_has_saved_key,
                            executor_llm_api_base_draft,
                            executor_llm_api_base_preset_select,
                            executor_llm_model_draft,
                            executor_llm_api_key_draft,
                            executor_llm_has_saved_key,
                        }
                        clear_client_key_intent=clear_client_key_intent
                        clear_executor_key_intent=clear_executor_key_intent
                        execution_mode_draft=execution_mode_draft
                    />
                </div>
            </div>
        </div>
    }
}
