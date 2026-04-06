//! 聊天区右键菜单：多选导出 Markdown。

use leptos::prelude::*;

use crate::session_export::{
    export_filename_stem, stored_messages_by_ids_to_markdown, trigger_download,
};
use crate::storage::ChatSession;

#[component]
pub fn ChatExportContextMenu(
    chat_export_ctx_menu: RwSignal<Option<(f64, f64)>>,
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
                aria-label="聊天区菜单"
                on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                style=move || {
                    chat_export_ctx_menu
                        .get()
                        .map(|(x, y)| format!("left:{}px;top:{}px;", x, y))
                        .unwrap_or_default()
                }
            >
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
                        "多选导出 Markdown…"
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
                        "全选消息"
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
                        "清除选择"
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
                            let md = sessions.with(|list| {
                                let aid = active_id.get_untracked();
                                list.iter()
                                    .find(|s| s.id == aid)
                                    .map(|s| stored_messages_by_ids_to_markdown(&s.messages, &ids))
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
                            if n == 0 {
                                "导出已选为 Markdown".to_string()
                            } else {
                                format!("导出已选为 Markdown（{n} 条）")
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
                        "退出多选"
                    </button>
                </Show>
            </div>
        </div>
    }
}
