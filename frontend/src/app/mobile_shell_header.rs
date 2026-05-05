//! 窄屏顶栏：菜单、标题、新对话。

use leptos::prelude::*;

use crate::i18n;

use super::app_shell_ctx::MobileShellHeaderSignals;

pub fn mobile_shell_header_view(signals: MobileShellHeaderSignals) -> impl IntoView {
    let MobileShellHeaderSignals {
        mobile_nav_open,
        locale,
        new_session,
    } = signals;
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
            <span class="shell-main-header-title">{move || i18n::app_shell_title(locale.get())}</span>
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
