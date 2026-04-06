//! 窄屏顶栏：菜单、标题、新对话。

use leptos::prelude::*;

pub fn mobile_shell_header_view(
    mobile_nav_open: RwSignal<bool>,
    new_session: impl Fn() + Clone + 'static,
) -> impl IntoView {
    view! {
        <div class="shell-main-header-mobile">
            <button
                type="button"
                class="btn btn-icon"
                aria-label="打开菜单"
                on:click=move |_| mobile_nav_open.update(|o| *o = !*o)
            >
                "☰"
            </button>
            <span class="shell-main-header-title">"CrabMate"</span>
            <button
                type="button"
                class="btn btn-secondary btn-sm"
                on:click={
                    let new_session = new_session.clone();
                    move |_| {
                        new_session();
                        mobile_nav_open.set(false);
                    }
                }
            >
                "新对话"
            </button>
        </div>
    }
}
