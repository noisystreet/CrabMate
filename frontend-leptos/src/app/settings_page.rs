//! 设置页面组件：替代原有的对话框模式。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;
use leptos_dom::helpers::window_event_listener;

use crate::api::{
    clear_client_llm_api_key_storage, clear_executor_llm_api_key_storage,
    client_llm_storage_has_api_key, executor_llm_storage_has_api_key,
    persist_client_llm_to_storage, persist_executor_llm_to_storage,
};
use crate::client_llm_presets::{CLIENT_LLM_API_BASE_PRESETS, preset_by_id};
use crate::i18n::{self, Locale, store_locale_slug};

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

#[component]
pub fn SettingsPageView(
    settings_page: RwSignal<bool>,
    locale: RwSignal<Locale>,
    theme: RwSignal<String>,
    bg_decor: RwSignal<bool>,
    llm_api_base_draft: RwSignal<String>,
    llm_api_base_preset_select: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_api_key_draft: RwSignal<String>,
    llm_has_saved_key: RwSignal<bool>,
    llm_settings_feedback: RwSignal<Option<String>>,
    executor_llm_api_base_draft: RwSignal<String>,
    executor_llm_api_base_preset_select: RwSignal<String>,
    executor_llm_model_draft: RwSignal<String>,
    executor_llm_api_key_draft: RwSignal<String>,
    executor_llm_has_saved_key: RwSignal<bool>,
    executor_llm_settings_feedback: RwSignal<Option<String>>,
    client_llm_storage_tick: RwSignal<u64>,
) -> impl IntoView {
    let active_section =
        RwSignal::new(read_settings_section_from_hash().unwrap_or(SettingsSection::Appearance));

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

    Effect::new(move |_| {
        if !settings_page.get() {
            clear_settings_hash_if_present();
            return;
        }
        write_settings_section_to_hash(active_section.get());
    });

    view! {
        <div class="settings-page" class:settings-page-visible=move || settings_page.get()>
            <div class="settings-page-header">
                <button
                    type="button"
                    class="btn btn-ghost settings-page-back"
                    on:click=move |_| settings_page.set(false)
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
                    <span>{move || i18n::settings_back(locale.get())}</span>
                </button>
                <h1 class="settings-page-title">{move || i18n::settings_title(locale.get())}</h1>
                <span class="settings-page-badge">{move || i18n::settings_badge_local(locale.get())}</span>
            </div>
            <div class="settings-page-body">
                <p class="settings-intro">{move || i18n::settings_intro(locale.get())}</p>
                <div class="settings-layout">
                    <nav class="settings-nav" aria-label="Settings sections">
                        <p class="settings-nav-group-label">
                            {move || i18n::settings_nav_group_general(locale.get())}
                        </p>
                        <button
                            type="button"
                            class="settings-nav-item"
                            class:active=move || active_section.get() == SettingsSection::Appearance
                            on:click=move |_| {
                                active_section.set(SettingsSection::Appearance);
                                write_settings_section_to_hash(SettingsSection::Appearance);
                            }
                        >
                            {move || i18n::settings_block_theme(locale.get())}
                        </button>
                        <p class="settings-nav-group-label">
                            {move || i18n::settings_nav_group_models(locale.get())}
                        </p>
                        <button
                            type="button"
                            class="settings-nav-item"
                            class:active=move || active_section.get() == SettingsSection::Llm
                            on:click=move |_| {
                                active_section.set(SettingsSection::Llm);
                                write_settings_section_to_hash(SettingsSection::Llm);
                            }
                        >
                            {move || i18n::settings_block_llm(locale.get())}
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
                            {move || i18n::settings_block_executor_llm(locale.get())}
                        </button>
                        <p class="settings-nav-group-label">
                            {move || i18n::settings_nav_group_help(locale.get())}
                        </p>
                        <button
                            type="button"
                            class="settings-nav-item"
                            class:active=move || active_section.get() == SettingsSection::Shortcuts
                            on:click=move |_| {
                                active_section.set(SettingsSection::Shortcuts);
                                write_settings_section_to_hash(SettingsSection::Shortcuts);
                            }
                        >
                            {move || i18n::settings_block_shortcuts(locale.get())}
                        </button>
                    </nav>

                    <section class="settings-content">
                        <header class="settings-content-header">
                            <h2 class="settings-content-title">{move || section_title(active_section.get(), locale.get())}</h2>
                            <p class="settings-content-desc">{move || section_desc(active_section.get(), locale.get())}</p>
                        </header>
                        <Show when=move || active_section.get() == SettingsSection::Appearance>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_language(locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || locale.get() == Locale::ZhHans
                                        on:click=move |_| {
                                            locale.set(Locale::ZhHans);
                                            store_locale_slug(Locale::ZhHans.storage_slug());
                                        }
                                    >
                                        {move || i18n::settings_lang_zh(locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || locale.get() == Locale::En
                                        on:click=move |_| {
                                            locale.set(Locale::En);
                                            store_locale_slug(Locale::En.storage_slug());
                                        }
                                    >
                                        {move || i18n::settings_lang_en(locale.get())}
                                    </button>
                                </div>
                            </div>

                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_theme(locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "dark"
                                        on:click=move |_| theme.set("dark".to_string())
                                    >
                                        {move || i18n::settings_theme_dark(locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || theme.get() == "light"
                                        on:click=move |_| theme.set("light".to_string())
                                    >
                                        {move || i18n::settings_theme_light(locale.get())}
                                    </button>
                                </div>
                            </div>

                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_bg(locale.get())}</h3>
                                <label class="settings-checkbox-label">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || bg_decor.get()
                                        on:change=move |_| bg_decor.update(|v| *v = !*v)
                                    />
                                    <span>{move || i18n::settings_bg_glow(locale.get())}</span>
                                </label>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::Llm>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_llm(locale.get())}</h3>
                                <p class="settings-field-nested-hint">
                                    {move || i18n::settings_llm_hint(locale.get())}
                                </p>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-llm-api-base-preset">
                            {move || i18n::settings_label_api_base_preset(locale.get())}
                        </label>
                        <select
                            id="settings-llm-api-base-preset"
                            class="settings-select"
                            prop:value=move || llm_api_base_preset_select.get()
                            on:change=move |ev| {
                                let id = event_target_value(&ev);
                                llm_api_base_preset_select.set(id.clone());
                                let Some(p) = preset_by_id(id.as_str()) else {
                                    return;
                                };
                                if p.id == "custom" {
                                    return;
                                }
                                llm_api_base_draft.set(p.url.to_string());
                                if let Some(m) = p.suggested_model {
                                    if llm_model_draft.get_untracked().trim().is_empty() {
                                        llm_model_draft.set(m.to_string());
                                    }
                                }
                            }
                        >
                            {CLIENT_LLM_API_BASE_PRESETS.iter().filter(|p| p.id != "custom").map(|p| {
                                let id = p.id;
                                view! {
                                    <option value=id>
                                        {move || i18n::settings_api_base_preset_label(id, locale.get())}
                                    </option>
                                }
                            }).collect_view()}
                            <option value="custom">
                                {move || i18n::settings_api_base_preset_custom(locale.get())}
                            </option>
                        </select>
                    </div>
                    <Show when=move || llm_api_base_preset_select.get() == "custom">
                        <div class="settings-field">
                            <label class="settings-field-label" for="settings-llm-api-base">
                                {move || i18n::settings_label_api_base(locale.get())}
                            </label>
                            <input
                                type="text"
                                id="settings-llm-api-base"
                                class="settings-text-input"
                                prop:placeholder=move || i18n::settings_ph_api_base(locale.get())
                                prop:value=move || llm_api_base_draft.get()
                                on:input=move |ev| {
                                    llm_api_base_preset_select.set("custom".to_string());
                                    llm_api_base_draft.set(event_target_value(&ev));
                                }
                            />
                        </div>
                    </Show>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-llm-model">
                            {move || i18n::settings_label_model(locale.get())}
                        </label>
                        <input
                            type="text"
                            id="settings-llm-model"
                            class="settings-text-input"
                            prop:placeholder=move || i18n::settings_ph_model(locale.get())
                            prop:value=move || llm_model_draft.get()
                            on:input=move |ev| {
                                llm_model_draft.set(event_target_value(&ev));
                            }
                        />
                    </div>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-llm-api-key">
                            {move || i18n::settings_label_api_key(locale.get())}
                        </label>
                        <input
                            type="password"
                            id="settings-llm-api-key"
                            class="settings-text-input"
                            autocomplete="off"
                            prop:placeholder=move || i18n::settings_ph_api_key(locale.get())
                            prop:value=move || llm_api_key_draft.get()
                            on:input=move |ev| {
                                llm_api_key_draft.set(event_target_value(&ev));
                            }
                        />
                    </div>
                    <Show when=move || llm_has_saved_key.get()>
                        <p class="settings-field-nested-hint">
                            {move || i18n::settings_key_saved_note(locale.get())}
                        </p>
                    </Show>
                    <div class="settings-actions-row">
                        <button
                            type="button"
                            class="btn btn-primary btn-sm"
                            on:click=move |_| {
                                llm_settings_feedback.set(None);
                                let loc = locale.get_untracked();
                                let key_raw = llm_api_key_draft.get();
                                let api_key_upd = if key_raw.trim().is_empty() {
                                    None
                                } else {
                                    Some(key_raw)
                                };
                                let base = llm_api_base_draft.get();
                                let model = llm_model_draft.get();
                                match persist_client_llm_to_storage(
                                    &base,
                                    &model,
                                    api_key_upd.as_deref(),
                                    loc,
                                ) {
                                    Ok(()) => {
                                        llm_api_key_draft.set(String::new());
                                        llm_has_saved_key
                                            .set(client_llm_storage_has_api_key());
                                        client_llm_storage_tick
                                            .update(|n| *n = n.wrapping_add(1));
                                        llm_settings_feedback.set(Some(
                                            i18n::settings_saved_browser(loc).to_string(),
                                        ));
                                    }
                                    Err(e) => llm_settings_feedback.set(Some(e)),
                                }
                            }
                        >
                            {move || i18n::settings_save_llm(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            prop:disabled=move || !llm_has_saved_key.get()
                            on:click=move |_| {
                                llm_settings_feedback.set(None);
                                let loc = locale.get_untracked();
                                let _ = clear_client_llm_api_key_storage(loc);
                                llm_has_saved_key.set(false);
                                llm_settings_feedback.set(Some(
                                    i18n::settings_cleared_key(loc).to_string(),
                                ));
                            }
                        >
                            {move || i18n::settings_clear_key(locale.get())}
                        </button>
                    </div>
                    <Show when=move || llm_settings_feedback.get().is_some()>
                        <p class="settings-save-feedback">{move || {
                            llm_settings_feedback.get().unwrap_or_default()
                        }}</p>
                    </Show>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::ExecutorLlm>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_executor_llm(locale.get())}</h3>
                                <p class="settings-field-nested-hint">
                                    {move || i18n::settings_executor_llm_hint(locale.get())}
                                </p>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-executor-llm-api-base-preset">
                            {move || i18n::settings_label_api_base_preset(locale.get())}
                        </label>
                        <select
                            id="settings-executor-llm-api-base-preset"
                            class="settings-select"
                            prop:value=move || executor_llm_api_base_preset_select.get()
                            on:change=move |ev| {
                                let id = event_target_value(&ev);
                                executor_llm_api_base_preset_select.set(id.clone());
                                let Some(p) = preset_by_id(id.as_str()) else {
                                    return;
                                };
                                if p.id == "custom" {
                                    return;
                                }
                                executor_llm_api_base_draft.set(p.url.to_string());
                                if let Some(m) = p.suggested_model {
                                    if executor_llm_model_draft.get_untracked().trim().is_empty() {
                                        executor_llm_model_draft.set(m.to_string());
                                    }
                                }
                            }
                        >
                            {CLIENT_LLM_API_BASE_PRESETS.iter().filter(|p| p.id != "custom").map(|p| {
                                let id = p.id;
                                view! {
                                    <option value=id>
                                        {move || i18n::settings_api_base_preset_label(id, locale.get())}
                                    </option>
                                }
                            }).collect_view()}
                            <option value="custom">
                                {move || i18n::settings_api_base_preset_custom(locale.get())}
                            </option>
                        </select>
                    </div>
                    <Show when=move || executor_llm_api_base_preset_select.get() == "custom">
                        <div class="settings-field">
                            <label class="settings-field-label" for="settings-executor-llm-api-base">
                                {move || i18n::settings_label_executor_api_base(locale.get())}
                            </label>
                            <input
                                type="text"
                                id="settings-executor-llm-api-base"
                                class="settings-text-input"
                                prop:placeholder=move || i18n::settings_ph_api_base(locale.get())
                                prop:value=move || executor_llm_api_base_draft.get()
                                on:input=move |ev| {
                                    executor_llm_api_base_preset_select.set("custom".to_string());
                                    executor_llm_api_base_draft.set(event_target_value(&ev));
                                }
                            />
                        </div>
                    </Show>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-executor-llm-model">
                            {move || i18n::settings_label_executor_model(locale.get())}
                        </label>
                        <input
                            type="text"
                            id="settings-executor-llm-model"
                            class="settings-text-input"
                            prop:placeholder=move || i18n::settings_ph_model(locale.get())
                            prop:value=move || executor_llm_model_draft.get()
                            on:input=move |ev| {
                                executor_llm_model_draft.set(event_target_value(&ev));
                            }
                        />
                    </div>
                    <div class="settings-field">
                        <label class="settings-field-label" for="settings-executor-llm-api-key">
                            {move || i18n::settings_label_executor_api_key(locale.get())}
                        </label>
                        <input
                            type="password"
                            id="settings-executor-llm-api-key"
                            class="settings-text-input"
                            autocomplete="off"
                            prop:placeholder=move || i18n::settings_ph_executor_api_key(locale.get())
                            prop:value=move || executor_llm_api_key_draft.get()
                            on:input=move |ev| {
                                executor_llm_api_key_draft.set(event_target_value(&ev));
                            }
                        />
                    </div>
                    <Show when=move || executor_llm_has_saved_key.get()>
                        <p class="settings-field-nested-hint">
                            {move || i18n::settings_executor_key_saved_note(locale.get())}
                        </p>
                    </Show>
                    <div class="settings-actions-row">
                        <button
                            type="button"
                            class="btn btn-primary btn-sm"
                            on:click=move |_| {
                                executor_llm_settings_feedback.set(None);
                                let loc = locale.get_untracked();
                                let key_raw = executor_llm_api_key_draft.get();
                                let api_key_upd = if key_raw.trim().is_empty() {
                                    None
                                } else {
                                    Some(key_raw)
                                };
                                let base = executor_llm_api_base_draft.get();
                                let model = executor_llm_model_draft.get();
                                match persist_executor_llm_to_storage(
                                    &base,
                                    &model,
                                    api_key_upd.as_deref(),
                                    loc,
                                ) {
                                    Ok(()) => {
                                        executor_llm_api_key_draft.set(String::new());
                                        executor_llm_has_saved_key
                                            .set(executor_llm_storage_has_api_key());
                                        client_llm_storage_tick
                                            .update(|n| *n = n.wrapping_add(1));
                                        executor_llm_settings_feedback.set(Some(
                                            i18n::settings_executor_saved_browser(loc).to_string(),
                                        ));
                                    }
                                    Err(e) => executor_llm_settings_feedback.set(Some(e)),
                                }
                            }
                        >
                            {move || i18n::settings_save_executor_llm(locale.get())}
                        </button>
                        <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            prop:disabled=move || !executor_llm_has_saved_key.get()
                            on:click=move |_| {
                                executor_llm_settings_feedback.set(None);
                                let loc = locale.get_untracked();
                                let _ = clear_executor_llm_api_key_storage(loc);
                                executor_llm_has_saved_key.set(false);
                                executor_llm_settings_feedback.set(Some(
                                    i18n::settings_executor_cleared_key(loc).to_string(),
                                ));
                            }
                        >
                            {move || i18n::settings_clear_executor_key(locale.get())}
                        </button>
                    </div>
                    <Show when=move || executor_llm_settings_feedback.get().is_some()>
                        <p class="settings-save-feedback">{move || {
                            executor_llm_settings_feedback.get().unwrap_or_default()
                        }}</p>
                    </Show>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::Shortcuts>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_shortcuts(locale.get())}</h3>
                                <p class="settings-intro">{move || i18n::settings_shortcuts_body(locale.get())}</p>
                            </div>
                        </Show>
                    </section>
                </div>
            </div>
        </div>
    }
}
