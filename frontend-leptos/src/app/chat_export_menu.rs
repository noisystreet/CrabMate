//! 聊天区右键菜单：复制选中文本、多选导出 Markdown。

use leptos::prelude::*;

use crate::i18n::{self, Locale};
use crate::session_export::{
    export_filename_stem, stored_messages_by_ids_to_markdown, trigger_download,
};
use crate::session_ops::write_clipboard_text;
use crate::storage::ChatSession;

#[component]
pub fn ChatExportContextMenu(
    chat_export_ctx_menu: RwSignal<Option<(f64, f64, Option<String>)>>,
    locale: RwSignal<Locale>,
    bubble_md_select_mode: RwSignal<bool>,
    bubble_md_selected_ids: RwSignal<Vec<String>>,
    sessions: RwSignal<Vec<ChatSession>>,
    active_id: RwSignal<String>,
) -> impl IntoView {
    view! {
        <div class="session-ctx-layer">
            <div
                class="session-ctx-backdrop"
                aria-hidden="true"
                on:click=move |_| chat_export_ctx_menu.set(None)
            ></div>
            <div
                class="session-ctx-menu"
                role="menu"
                prop:aria-label=move || i18n::chat_ctx_menu_aria(locale.get())
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                style=move || {
                    chat_export_ctx_menu
                        .get()
                        .map(|(x, y, _)| format!("left:{}px;top:{}px;", x, y))
                        .unwrap_or_default()
                }
            >
                <Show when=move || {
                    matches!(
                        chat_export_ctx_menu.get(),
                        Some((_, _, Some(ref s))) if !s.is_empty()
                    )
                }>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some((_, _, Some(text))) = chat_export_ctx_menu.get() else {
                                return;
                            };
                            chat_export_ctx_menu.set(None);
                            write_clipboard_text(&text, locale.get_untracked());
                        }
                    >
                        {move || i18n::chat_ctx_copy_selection(locale.get())}
                    </button>
                </Show>
                <Show when=move || !bubble_md_select_mode.get()>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            chat_export_ctx_menu.set(None);
                            bubble_md_selected_ids.set(Vec::new());
                            bubble_md_select_mode.set(true);
                        }
                    >
                        {move || i18n::chat_ctx_md_multi(locale.get())}
                    </button>
                </Show>
                <Show when=move || bubble_md_select_mode.get()>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            chat_export_ctx_menu.set(None);
                            let ids = sessions.with(|list| {
                                let aid = active_id.get_untracked();
                                list.iter()
                                    .find(|s| s.id == aid)
                                    .map(|s| {
                                        s.messages.iter().map(|m| m.id.clone()).collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default()
                            });
                            bubble_md_selected_ids.set(ids);
                        }
                    >
                        {move || i18n::chat_ctx_select_all(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            chat_export_ctx_menu.set(None);
                            bubble_md_selected_ids.set(Vec::new());
                        }
                    >
                        {move || i18n::chat_ctx_clear_sel(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        prop:disabled=move || bubble_md_selected_ids.with(|v| v.is_empty())
                        on:click=move |_| {
                            chat_export_ctx_menu.set(None);
                            let ids = bubble_md_selected_ids.get();
                            if ids.is_empty() {
                                return;
                            }
                            let loc = locale.get_untracked();
                            let md = sessions.with(|list| {
                                let aid = active_id.get_untracked();
                                list.iter()
                                    .find(|s| s.id == aid)
                                    .map(|s| {
                                        stored_messages_by_ids_to_markdown(
                                            &s.messages,
                                            &ids,
                                            loc,
                                        )
                                    })
                                    .unwrap_or_default()
                            });
                            let stem = export_filename_stem("chat_selection");
                            let name = format!("{stem}.md");
                            if let Err(e) = trigger_download(&name, "text/markdown;charset=utf-8", &md) {
                                if let Some(w) = web_sys::window() {
                                    let _ = w.alert_with_message(&e);
                                }
                            }
                        }
                    >
                        {move || {
                            let n = bubble_md_selected_ids.with(|v| v.len());
                            let loc = locale.get();
                            if n == 0 {
                                i18n::chat_ctx_export_md_empty(loc).to_string()
                            } else {
                                i18n::chat_ctx_export_md_n(loc, n)
                            }
                        }}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            chat_export_ctx_menu.set(None);
                            bubble_md_select_mode.set(false);
                            bubble_md_selected_ids.set(Vec::new());
                        }
                    >
                        {move || i18n::chat_ctx_exit_multi(locale.get())}
                    </button>
                </Show>
            </div>
        </div>
    }
}
