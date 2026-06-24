//! 侧栏：布局分段 + 紧凑「新建对话」图标按钮（仅对话模式）。

use std::rc::Rc;

use leptos::prelude::*;

use crate::app::layout_mode_segment::LayoutModeSegment;
use crate::i18n::{self, Locale};

#[component]
pub(super) fn NavRailModeActions(
    locale: RwSignal<Locale>,
    editor_layout_mode: RwSignal<bool>,
    new_session: Rc<dyn Fn()>,
    mobile_nav_open: RwSignal<bool>,
) -> impl IntoView {
    let on_new_chat = {
        let new_session = Rc::clone(&new_session);
        move |_| {
            new_session();
            mobile_nav_open.set(false);
        }
    };
    view! {
        <div class="nav-rail-mode-actions">
            <div class="nav-rail-mode-toolbar">
                <LayoutModeSegment
                    locale=locale
                    editor_layout_mode=editor_layout_mode
                    extra_class="nav-rail-layout-segment"
                />
                <button
                    type="button"
                    class="btn btn-primary btn-icon btn-nav-new-chat"
                    data-testid="nav-new-chat"
                    prop:title=move || i18n::nav_new_chat(locale.get())
                    prop:aria-label=move || i18n::nav_new_chat_aria(locale.get())
                    prop:style:display=move || {
                        if editor_layout_mode.get() {
                            "none"
                        } else {
                            ""
                        }
                    }
                    on:click=on_new_chat
                >
                    <span aria-hidden="true">"+"</span>
                </button>
            </div>
        </div>
    }
}
