//! 窄屏顶栏：菜单、标题、新对话。

use leptos::prelude::*;

use crate::i18n;

use super::app_shell_ctx::AppShellCtx;

pub fn mobile_shell_header_view(ctx: AppShellCtx) -> impl IntoView {
    let mobile_nav_open = ctx.mobile_nav_open;
    let locale = ctx.locale;
    let new_session = ctx.new_session;
    view! {
        <div class="shell-main-header-mobile">
            <button
                type="button"
                class="btn btn-icon"
                prop:aria-label=move || i18n::mobile_open_menu(locale.get())
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
                {move || i18n::nav_new_chat(locale.get())}
            </button>
        </div>
    }
}
