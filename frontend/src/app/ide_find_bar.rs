//! IDE 编辑器查找栏与跳转行栏。

use leptos::prelude::*;
use leptos_dom::helpers::event_target_value;

use crate::app::app_signals::IdeChromeSignals;
use crate::i18n::{self, Locale};
use crate::ide_find::{
    apply_textarea_selection, find_match_ranges, goto_line_in_textarea, textarea_from_ref,
};

#[derive(Clone, Copy)]
pub struct IdeFindBarInput {
    pub locale: RwSignal<Locale>,
    pub chrome: IdeChromeSignals,
    pub ide_text: RwSignal<String>,
    pub textarea_ref: NodeRef<leptos::html::Textarea>,
}

fn apply_find_match(input: IdeFindBarInput, match_index: usize) {
    let text = input.ide_text.get_untracked();
    let query = input.chrome.find_query.get_untracked();
    let ranges = find_match_ranges(&text, &query);
    if ranges.is_empty() {
        input.chrome.find_match_index.set(0);
        return;
    }
    let idx = match_index % ranges.len();
    input.chrome.find_match_index.set(idx);
    let (start, end) = ranges[idx];
    if let Some(ta) = textarea_from_ref(&input.textarea_ref) {
        apply_textarea_selection(&ta, start, end);
    }
}

fn find_nav(input: IdeFindBarInput, delta: isize) {
    let text = input.ide_text.get_untracked();
    let query = input.chrome.find_query.get_untracked();
    let ranges = find_match_ranges(&text, &query);
    if ranges.is_empty() {
        return;
    }
    let cur = input.chrome.find_match_index.get_untracked();
    let next = if delta < 0 {
        (cur + ranges.len() - 1) % ranges.len()
    } else {
        (cur + 1) % ranges.len()
    };
    apply_find_match(input, next);
}

fn find_meta_line(locale: Locale, query: &str, match_count: usize, cursor: usize) -> String {
    if query.trim().is_empty() {
        String::new()
    } else if match_count == 0 {
        i18n::ide_find_no_match(locale).to_string()
    } else {
        format!("{}/{}", cursor + 1, match_count)
    }
}

#[component]
fn IdeFindBarPanel(
    locale: RwSignal<Locale>,
    chrome: IdeChromeSignals,
    input: IdeFindBarInput,
    match_count: Memo<usize>,
) -> impl IntoView {
    view! {
        <div
            class="ide-find-bar"
            role="search"
            data-testid="ide-find-bar"
            prop:aria-label=move || i18n::ide_find_region(locale.get())
        >
            <label class="ide-find-label" for="ide-find-input">
                {move || i18n::ide_find_label(locale.get())}
            </label>
            <input
                id="ide-find-input"
                type="search"
                class="ide-find-input"
                data-testid="ide-find-input"
                prop:placeholder=move || i18n::ide_find_ph(locale.get())
                prop:value=move || chrome.find_query.get()
                on:input=move |ev| {
                    chrome.find_query.set(event_target_value(&ev));
                    chrome.find_match_index.set(0);
                }
                on:keydown=move |ev: web_sys::KeyboardEvent| {
                    if ev.key() == "Enter" {
                        ev.prevent_default();
                        find_nav(input, if ev.shift_key() { -1 } else { 1 });
                    }
                }
            />
            <span class="ide-find-meta" aria-live="polite">
                {move || {
                    find_meta_line(
                        locale.get(),
                        &chrome.find_query.get(),
                        match_count.get(),
                        chrome.find_match_index.get(),
                    )
                }}
            </span>
            <button
                type="button"
                class="btn btn-secondary btn-sm ide-find-nav"
                prop:title=move || i18n::ide_find_prev_title(locale.get())
                prop:disabled=move || match_count.get() == 0
                on:click=move |_| find_nav(input, -1)
            >
                "‹"
            </button>
            <button
                type="button"
                class="btn btn-secondary btn-sm ide-find-nav"
                prop:title=move || i18n::ide_find_next_title(locale.get())
                prop:disabled=move || match_count.get() == 0
                on:click=move |_| find_nav(input, 1)
            >
                "›"
            </button>
            <button
                type="button"
                class="btn btn-ghost btn-sm ide-find-close"
                prop:title=move || i18n::ide_find_close_title(locale.get())
                prop:aria-label=move || i18n::ide_find_close_aria(locale.get())
                on:click=move |_| chrome.find_panel_open.set(false)
            >
                "×"
            </button>
        </div>
    }
}

#[component]
pub fn IdeFindBar(input: IdeFindBarInput) -> impl IntoView {
    let IdeFindBarInput {
        locale,
        chrome,
        ide_text,
        ..
    } = input;

    let match_count =
        Memo::new(move |_| find_match_ranges(&ide_text.get(), &chrome.find_query.get()).len());

    Effect::new(move |_| {
        let _ = ide_text.get();
        let _ = chrome.find_query.get();
        if chrome.find_panel_open.get() {
            apply_find_match(input, chrome.find_match_index.get_untracked());
        }
    });

    view! {
        <Show when=move || chrome.find_panel_open.get()>
            <IdeFindBarPanel locale chrome input match_count />
        </Show>
    }
}

fn submit_goto_line(chrome: IdeChromeSignals, textarea_ref: &NodeRef<leptos::html::Textarea>) {
    let raw = chrome.goto_line.get_untracked();
    let Ok(line) = raw.trim().parse::<usize>() else {
        return;
    };
    if let Some(ta) = textarea_from_ref(textarea_ref) {
        goto_line_in_textarea(&ta, line);
    }
    chrome.goto_panel_open.set(false);
}

#[component]
pub fn IdeGotoLineBar(input: IdeFindBarInput) -> impl IntoView {
    let IdeFindBarInput {
        locale,
        chrome,
        textarea_ref,
        ..
    } = input;

    view! {
        <Show when=move || chrome.goto_panel_open.get()>
            <div
                class="ide-find-bar ide-goto-bar"
                role="search"
                data-testid="ide-goto-bar"
                prop:aria-label=move || i18n::ide_goto_region(locale.get())
            >
                <label class="ide-find-label" for="ide-goto-input">
                    {move || i18n::ide_goto_label(locale.get())}
                </label>
                <input
                    id="ide-goto-input"
                    type="text"
                    inputmode="numeric"
                    class="ide-find-input"
                    data-testid="ide-goto-input"
                    prop:placeholder=move || i18n::ide_goto_ph(locale.get())
                    prop:value=move || chrome.goto_line.get()
                    on:input=move |ev| chrome.goto_line.set(event_target_value(&ev))
                    on:keydown=move |ev: web_sys::KeyboardEvent| {
                        if ev.key() == "Enter" {
                            ev.prevent_default();
                            submit_goto_line(chrome, &textarea_ref);
                        }
                    }
                />
                <button
                    type="button"
                    class="btn btn-ghost btn-sm ide-find-close"
                    prop:title=move || i18n::ide_goto_close_title(locale.get())
                    prop:aria-label=move || i18n::ide_goto_close_aria(locale.get())
                    on:click=move |_| chrome.goto_panel_open.set(false)
                >
                    "×"
                </button>
            </div>
        </Show>
    }
}
