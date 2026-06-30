//! IDE 编辑器多标签页标签栏。

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::i18n::{self, Locale};
use crate::ide_confirm::IdeConfirmSignals;
use crate::ide_tabs::{
    IdeTabsEditorSignals, IdeTabsHandle, close_all_tabs, close_other_tabs_at, close_tab_at,
    ide_tab_basename, toggle_tab_pinned, try_switch_tab,
};

/// 标签栏右键菜单锚点（`position: fixed` 使用视口坐标）。
#[derive(Clone, Debug, PartialEq)]
pub struct IdeTabContextAnchor {
    pub x: f64,
    pub y: f64,
    pub tab_index: usize,
}

#[derive(Clone, Copy)]
pub struct IdeTabsBarInput {
    pub locale: RwSignal<Locale>,
    pub tabs: IdeTabsHandle,
    pub confirm: IdeConfirmSignals,
    pub editor: IdeTabsEditorSignals,
}

#[component]
fn IdeTabContextMenuLayer(
    locale: RwSignal<Locale>,
    tabs: IdeTabsHandle,
    confirm: IdeConfirmSignals,
    editor: IdeTabsEditorSignals,
    ctx_menu: RwSignal<Option<IdeTabContextAnchor>>,
) -> impl IntoView {
    view! {
        <Show when=move || ctx_menu.get().is_some()>
            <div class="session-ctx-layer">
                <div
                    class="session-ctx-backdrop"
                    aria-hidden="true"
                    on:click=move |_| ctx_menu.set(None)
                ></div>
                <div
                    class="session-ctx-menu"
                    role="menu"
                    on:click=|ev: leptos::ev::MouseEvent| ev.stop_propagation()
                    style=move || {
                        ctx_menu
                            .get()
                            .map(|a| format!("left:{}px;top:{}px;", a.x, a.y))
                            .unwrap_or_default()
                    }
                >
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = ctx_menu.get() else {
                                return;
                            };
                            let index = a.tab_index;
                            ctx_menu.set(None);
                            spawn_local(async move {
                                close_tab_at(tabs, index, locale, editor, confirm).await;
                            });
                        }
                    >
                        {move || i18n::ide_tab_ctx_close(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = ctx_menu.get() else {
                                return;
                            };
                            let index = a.tab_index;
                            ctx_menu.set(None);
                            spawn_local(async move {
                                close_other_tabs_at(tabs, index, locale, editor, confirm).await;
                            });
                        }
                    >
                        {move || i18n::ide_tab_ctx_close_others(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            ctx_menu.set(None);
                            spawn_local(async move {
                                close_all_tabs(tabs, locale, editor, confirm).await;
                            });
                        }
                    >
                        {move || i18n::ide_tab_ctx_close_all(locale.get())}
                    </button>
                    <button
                        type="button"
                        class="session-ctx-item"
                        role="menuitem"
                        on:click=move |_| {
                            let Some(a) = ctx_menu.get() else {
                                return;
                            };
                            let index = a.tab_index;
                            let IdeTabsEditorSignals {
                                ide_path,
                                ide_text,
                                ide_baseline,
                            } = editor;
                            ctx_menu.set(None);
                            toggle_tab_pinned(tabs, index, ide_path, ide_text, ide_baseline);
                        }
                    >
                        {move || {
                            let _ = tabs.tabs.get();
                            let loc = locale.get();
                            let Some(a) = ctx_menu.get() else {
                                return i18n::ide_tab_ctx_pin(loc).to_string();
                            };
                            let pinned = tabs
                                .tabs
                                .get_untracked()
                                .get(a.tab_index)
                                .is_some_and(|t| t.pinned);
                            if pinned {
                                i18n::ide_tab_ctx_unpin(loc).to_string()
                            } else {
                                i18n::ide_tab_ctx_pin(loc).to_string()
                            }
                        }}
                    </button>
                </div>
            </div>
        </Show>
    }
}

#[component]
pub fn IdeTabsBar(input: IdeTabsBarInput) -> impl IntoView {
    let IdeTabsBarInput {
        locale,
        tabs,
        confirm,
        editor,
    } = input;
    let IdeTabsEditorSignals {
        ide_path: _,
        ide_text,
        ide_baseline,
    } = editor;
    let ctx_menu = RwSignal::new(None::<IdeTabContextAnchor>);

    view! {
        <Show when=move || !tabs.tabs.get().is_empty()>
            <IdeTabContextMenuLayer
                locale
                tabs
                confirm
                editor
                ctx_menu
            />
            <div
                class="ide-tabs-bar"
                role="tablist"
                prop:aria-label=move || i18n::ide_tabs_aria(locale.get())
            >
                <For
                    each=move || {
                        tabs.tabs
                            .get()
                            .into_iter()
                            .enumerate()
                            .collect::<Vec<_>>()
                    }
                    key=|(idx, tab)| format!("{}:{}:{}", idx, tab.path, tab.pinned)
                    children=move |(index, tab)| {
                        let path = tab.path.clone();
                        let label = ide_tab_basename(&path);
                        let is_pinned = tab.pinned;
                        let is_active = move || tabs.active.get() == Some(index);
                        let is_dirty = move || {
                            tabs.tab_display_dirty(index, ide_text, ide_baseline)
                        };
                        view! {
                            <div
                                class="ide-tab"
                                class:ide-tab-active=is_active
                                class:ide-tab-pinned=move || is_pinned
                                role="presentation"
                                on:contextmenu=move |ev: web_sys::MouseEvent| {
                                    ev.prevent_default();
                                    ctx_menu.set(Some(IdeTabContextAnchor {
                                        x: ev.client_x() as f64,
                                        y: ev.client_y() as f64,
                                        tab_index: index,
                                    }));
                                }
                            >
                                <button
                                    type="button"
                                    class="ide-tab-select"
                                    role="tab"
                                    prop:id=format!("ide-tab-{}", index)
                                    prop:aria-selected=move || is_active().to_string()
                                    prop:aria-controls="ide-editor-panel"
                                    prop:title=path.clone()
                                    data-testid=format!("ide-tab-{}", index)
                                    on:click=move |_| {
                                        spawn_local(async move {
                                            let _ = try_switch_tab(
                                                tabs, index, locale, editor, confirm,
                                            )
                                            .await;
                                        });
                                    }
                                >
                                    <Show when=move || is_pinned>
                                        <span
                                            class="ide-tab-pin"
                                            aria-hidden="true"
                                            prop:title=move || {
                                                i18n::ide_tab_pinned_aria(locale.get()).to_string()
                                            }
                                        ></span>
                                    </Show>
                                    <Show when=is_dirty>
                                        <span class="ide-tab-dirty" aria-hidden="true">"●"</span>
                                    </Show>
                                    <span class="ide-tab-label">{label.clone()}</span>
                                </button>
                                <button
                                    type="button"
                                    class="ide-tab-close"
                                    prop:aria-label={
                                        let label = label.clone();
                                        move || i18n::ide_tab_close_aria(locale.get(), &label)
                                    }
                                    on:click=move |ev| {
                                        ev.stop_propagation();
                                        spawn_local(async move {
                                            close_tab_at(tabs, index, locale, editor, confirm)
                                                .await;
                                        });
                                    }
                                >
                                    <span aria-hidden="true">"×"</span>
                                </button>
                            </div>
                        }
                    }
                />
            </div>
        </Show>
    }
}
