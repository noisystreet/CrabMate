//! 设置页面组件：替代原有的对话框模式。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;
use leptos_dom::helpers::window_event_listener;

use crate::app::settings_commit::commit_all_settings;
use crate::i18n::{self, Locale};

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

#[component]
pub fn SettingsPageView(
    settings_page: RwSignal<bool>,
    locale: RwSignal<Locale>,
    theme: RwSignal<String>,
    bg_decor: RwSignal<bool>,
    llm_api_base_draft: RwSignal<String>,
    llm_api_base_preset_select: RwSignal<String>,
    llm_model_draft: RwSignal<String>,
    llm_temperature_draft: RwSignal<String>,
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

    Effect::new(move |_| {
        if !settings_page.get() {
            return;
        }
        appearance_locale.set(locale.get_untracked());
        appearance_theme.set(theme.get_untracked());
        appearance_bg_decor.set(bg_decor.get_untracked());
        let _ = baseline_appearance.try_update_value(|v| {
            *v = (
                locale.get_untracked(),
                theme.get_untracked(),
                bg_decor.get_untracked(),
            );
        });
        let _ = baseline_llm.try_update_value(|v| {
            *v = (
                llm_api_base_draft.get_untracked(),
                llm_api_base_preset_select.get_untracked(),
                llm_model_draft.get_untracked(),
                llm_temperature_draft.get_untracked(),
                llm_has_saved_key.get_untracked(),
            );
        });
        let _ = baseline_executor.try_update_value(|v| {
            *v = (
                executor_llm_api_base_draft.get_untracked(),
                executor_llm_api_base_preset_select.get_untracked(),
                executor_llm_model_draft.get_untracked(),
                executor_llm_has_saved_key.get_untracked(),
            );
        });
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
        let (bl, bt, bbd) = baseline_appearance.get_value();
        if appearance_locale.get() != bl
            || appearance_theme.get() != bt
            || appearance_bg_decor.get() != bbd
        {
            return true;
        }
        if clear_client_key_intent.get() || clear_executor_key_intent.get() {
            return true;
        }
        if !llm_api_key_draft.get().trim().is_empty()
            || !executor_llm_api_key_draft.get().trim().is_empty()
        {
            return true;
        }
        let (bb, bp, bm, bt, bh) = baseline_llm.get_value();
        if llm_api_base_draft.get() != bb
            || llm_api_base_preset_select.get() != bp
            || llm_model_draft.get() != bm
            || llm_temperature_draft.get() != bt
            || llm_has_saved_key.get() != bh
        {
            return true;
        }
        let (eb, ep, em, eh) = baseline_executor.get_value();
        if executor_llm_api_base_draft.get() != eb
            || executor_llm_api_base_preset_select.get() != ep
            || executor_llm_model_draft.get() != em
            || executor_llm_has_saved_key.get() != eh
        {
            return true;
        }
        false
    });

    let discard = move |_| {
        let (bl, bt, bbd) = baseline_appearance.get_value();
        appearance_locale.set(bl);
        appearance_theme.set(bt);
        appearance_bg_decor.set(bbd);

        let (bb, bp, bm, bt, bh) = baseline_llm.get_value();
        llm_api_base_draft.set(bb);
        llm_api_base_preset_select.set(bp);
        llm_model_draft.set(bm);
        llm_temperature_draft.set(bt);
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
        match commit_all_settings(
            ui_locale,
            appearance_locale.get(),
            appearance_theme.get(),
            appearance_bg_decor.get(),
            locale,
            theme,
            bg_decor,
            llm_api_base_draft.get().as_str(),
            llm_model_draft.get().as_str(),
            llm_temperature_draft.get().as_str(),
            llm_api_key_draft.get().as_str(),
            executor_llm_api_base_draft.get().as_str(),
            executor_llm_model_draft.get().as_str(),
            executor_llm_api_key_draft.get().as_str(),
            clear_client_key_intent.get(),
            clear_executor_key_intent.get(),
            llm_api_key_draft,
            llm_has_saved_key,
            executor_llm_api_key_draft,
            executor_llm_has_saved_key,
            client_llm_storage_tick,
        ) {
            Ok(()) => {
                let _ = baseline_appearance.try_update_value(|v| {
                    *v = (
                        appearance_locale.get_untracked(),
                        appearance_theme.get_untracked(),
                        appearance_bg_decor.get_untracked(),
                    );
                });
                let _ = baseline_llm.try_update_value(|v| {
                    *v = (
                        llm_api_base_draft.get_untracked(),
                        llm_api_base_preset_select.get_untracked(),
                        llm_model_draft.get_untracked(),
                        llm_temperature_draft.get_untracked(),
                        llm_has_saved_key.get_untracked(),
                    );
                });
                let _ = baseline_executor.try_update_value(|v| {
                    *v = (
                        executor_llm_api_base_draft.get_untracked(),
                        executor_llm_api_base_preset_select.get_untracked(),
                        executor_llm_model_draft.get_untracked(),
                        executor_llm_has_saved_key.get_untracked(),
                    );
                });
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
                    <nav class="settings-nav" aria-label="Settings sections">
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

                    <section class="settings-content">
                        <header class="settings-content-header">
                            <h2 class="settings-content-title">{move || section_title(active_section.get(), appearance_locale.get())}</h2>
                            <p class="settings-content-desc">{move || section_desc(active_section.get(), appearance_locale.get())}</p>
                        </header>
                        <Show when=move || active_section.get() == SettingsSection::Appearance>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_language(appearance_locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || appearance_locale.get() == Locale::ZhHans
                                        on:click=move |_| appearance_locale.set(Locale::ZhHans)
                                    >
                                        {move || i18n::settings_lang_zh(appearance_locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || appearance_locale.get() == Locale::En
                                        on:click=move |_| appearance_locale.set(Locale::En)
                                    >
                                        {move || i18n::settings_lang_en(appearance_locale.get())}
                                    </button>
                                </div>
                            </div>

                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_theme(appearance_locale.get())}</h3>
                                <div class="settings-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || appearance_theme.get() == "dark"
                                        on:click=move |_| appearance_theme.set("dark".to_string())
                                    >
                                        {move || i18n::settings_theme_dark(appearance_locale.get())}
                                    </button>
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        class:active=move || appearance_theme.get() == "light"
                                        on:click=move |_| appearance_theme.set("light".to_string())
                                    >
                                        {move || i18n::settings_theme_light(appearance_locale.get())}
                                    </button>
                                </div>
                            </div>

                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_bg(appearance_locale.get())}</h3>
                                <label class="settings-checkbox-label">
                                    <input
                                        type="checkbox"
                                        prop:checked=move || appearance_bg_decor.get()
                                        on:change=move |_| appearance_bg_decor.update(|v| *v = !*v)
                                    />
                                    <span>{move || i18n::settings_bg_glow(appearance_locale.get())}</span>
                                </label>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::Llm>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_llm(appearance_locale.get())}</h3>
                                <p class="settings-field-nested-hint">
                                    {move || i18n::settings_llm_hint(appearance_locale.get())}
                                </p>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-base-preset">
                                        {move || i18n::settings_label_api_base_preset(appearance_locale.get())}
                                    </label>
                                    <select
                                        id="settings-llm-api-base-preset"
                                        class="settings-select"
                                        prop:value=move || llm_api_base_preset_select.get()
                                        on:change=move |ev| {
                                            let id = event_target_value(&ev);
                                            llm_api_base_preset_select.set(id.clone());
                                            let Some(p) = crate::client_llm_presets::preset_by_id(id.as_str()) else {
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
                                        {crate::client_llm_presets::CLIENT_LLM_API_BASE_PRESETS.iter().filter(|p| p.id != "custom").map(|p| {
                                            let id = p.id;
                                            view! {
                                                <option value=id>
                                                    {move || i18n::settings_api_base_preset_label(id, appearance_locale.get())}
                                                </option>
                                            }
                                        }).collect_view()}
                                        <option value="custom">
                                            {move || i18n::settings_api_base_preset_custom(appearance_locale.get())}
                                        </option>
                                    </select>
                                </div>
                                <Show when=move || llm_api_base_preset_select.get() == "custom">
                                    <div class="settings-field">
                                        <label class="settings-field-label" for="settings-llm-api-base">
                                            {move || i18n::settings_label_api_base(appearance_locale.get())}
                                        </label>
                                        <input
                                            type="text"
                                            id="settings-llm-api-base"
                                            class="settings-text-input"
                                            prop:placeholder=move || i18n::settings_ph_api_base(appearance_locale.get())
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
                                        {move || i18n::settings_label_model(appearance_locale.get())}
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-llm-model"
                                        class="settings-text-input"
                                        prop:placeholder=move || i18n::settings_ph_model(appearance_locale.get())
                                        prop:value=move || llm_model_draft.get()
                                        on:input=move |ev| {
                                            llm_model_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-temperature">
                                        {move || i18n::settings_label_temperature(appearance_locale.get())}
                                    </label>
                                    <input
                                        type="number"
                                        id="settings-llm-temperature"
                                        class="settings-text-input"
                                        min="0"
                                        max="2"
                                        step="0.1"
                                        prop:placeholder=move || i18n::settings_ph_temperature(appearance_locale.get())
                                        prop:value=move || llm_temperature_draft.get()
                                        on:input=move |ev| {
                                            llm_temperature_draft.set(event_target_value(&ev));
                                        }
                                    />
                                    <p class="settings-field-nested-hint">
                                        {move || i18n::settings_temperature_hint(appearance_locale.get())}
                                    </p>
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-llm-api-key">
                                        {move || i18n::settings_label_api_key(appearance_locale.get())}
                                    </label>
                                    <input
                                        type="password"
                                        id="settings-llm-api-key"
                                        class="settings-text-input"
                                        autocomplete="off"
                                        prop:placeholder=move || i18n::settings_ph_api_key(appearance_locale.get())
                                        prop:value=move || llm_api_key_draft.get()
                                        on:input=move |ev| {
                                            llm_api_key_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <Show when=move || llm_has_saved_key.get() && !clear_client_key_intent.get()>
                                    <p class="settings-field-nested-hint">
                                        {move || i18n::settings_key_saved_note(appearance_locale.get())}
                                    </p>
                                </Show>
                                <div class="settings-actions-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        prop:disabled=move || !llm_has_saved_key.get() || clear_client_key_intent.get()
                                        on:click=move |_| {
                                            clear_client_key_intent.set(true);
                                            llm_api_key_draft.set(String::new());
                                        }
                                    >
                                        {move || i18n::settings_clear_key(appearance_locale.get())}
                                    </button>
                                </div>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::ExecutorLlm>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_executor_llm(appearance_locale.get())}</h3>
                                <p class="settings-field-nested-hint">
                                    {move || i18n::settings_executor_llm_hint(appearance_locale.get())}
                                </p>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-executor-llm-api-base-preset">
                                        {move || i18n::settings_label_api_base_preset(appearance_locale.get())}
                                    </label>
                                    <select
                                        id="settings-executor-llm-api-base-preset"
                                        class="settings-select"
                                        prop:value=move || executor_llm_api_base_preset_select.get()
                                        on:change=move |ev| {
                                            let id = event_target_value(&ev);
                                            executor_llm_api_base_preset_select.set(id.clone());
                                            let Some(p) = crate::client_llm_presets::preset_by_id(id.as_str()) else {
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
                                        {crate::client_llm_presets::CLIENT_LLM_API_BASE_PRESETS.iter().filter(|p| p.id != "custom").map(|p| {
                                            let id = p.id;
                                            view! {
                                                <option value=id>
                                                    {move || i18n::settings_api_base_preset_label(id, appearance_locale.get())}
                                                </option>
                                            }
                                        }).collect_view()}
                                        <option value="custom">
                                            {move || i18n::settings_api_base_preset_custom(appearance_locale.get())}
                                        </option>
                                    </select>
                                </div>
                                <Show when=move || executor_llm_api_base_preset_select.get() == "custom">
                                    <div class="settings-field">
                                        <label class="settings-field-label" for="settings-executor-llm-api-base">
                                            {move || i18n::settings_label_executor_api_base(appearance_locale.get())}
                                        </label>
                                        <input
                                            type="text"
                                            id="settings-executor-llm-api-base"
                                            class="settings-text-input"
                                            prop:placeholder=move || i18n::settings_ph_api_base(appearance_locale.get())
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
                                        {move || i18n::settings_label_executor_model(appearance_locale.get())}
                                    </label>
                                    <input
                                        type="text"
                                        id="settings-executor-llm-model"
                                        class="settings-text-input"
                                        prop:placeholder=move || i18n::settings_ph_model(appearance_locale.get())
                                        prop:value=move || executor_llm_model_draft.get()
                                        on:input=move |ev| {
                                            executor_llm_model_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <div class="settings-field">
                                    <label class="settings-field-label" for="settings-executor-llm-api-key">
                                        {move || i18n::settings_label_executor_api_key(appearance_locale.get())}
                                    </label>
                                    <input
                                        type="password"
                                        id="settings-executor-llm-api-key"
                                        class="settings-text-input"
                                        autocomplete="off"
                                        prop:placeholder=move || i18n::settings_ph_executor_api_key(appearance_locale.get())
                                        prop:value=move || executor_llm_api_key_draft.get()
                                        on:input=move |ev| {
                                            executor_llm_api_key_draft.set(event_target_value(&ev));
                                        }
                                    />
                                </div>
                                <Show when=move || executor_llm_has_saved_key.get() && !clear_executor_key_intent.get()>
                                    <p class="settings-field-nested-hint">
                                        {move || i18n::settings_executor_key_saved_note(appearance_locale.get())}
                                    </p>
                                </Show>
                                <div class="settings-actions-row">
                                    <button
                                        type="button"
                                        class="btn btn-secondary btn-sm"
                                        prop:disabled=move || !executor_llm_has_saved_key.get() || clear_executor_key_intent.get()
                                        on:click=move |_| {
                                            clear_executor_key_intent.set(true);
                                            executor_llm_api_key_draft.set(String::new());
                                        }
                                    >
                                        {move || i18n::settings_clear_executor_key(appearance_locale.get())}
                                    </button>
                                </div>
                            </div>
                        </Show>

                        <Show when=move || active_section.get() == SettingsSection::Shortcuts>
                            <div class="settings-block">
                                <h3 class="settings-block-title">{move || i18n::settings_block_shortcuts(appearance_locale.get())}</h3>
                                <p class="settings-intro">{move || i18n::settings_shortcuts_body(appearance_locale.get())}</p>
                            </div>
                        </Show>
                    </section>
                </div>
            </div>
        </div>
    }
}
