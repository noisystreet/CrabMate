//! 窄屏顶栏：菜单、标题、布局分段与（仅对话模式）新对话。

use leptos::prelude::*;

use crate::i18n;

use super::app_shell_ctx::MobileShellHeaderSignals;
use super::layout_mode_segment::LayoutModeSegment;

pub fn mobile_shell_header_view(signals: MobileShellHeaderSignals) -> impl IntoView {
    let MobileShellHeaderSignals {
        mobile_nav_open,
        locale,
        new_session,
        editor_layout_mode,
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
            <span class="shell-main-header-title">{move || {
                if editor_layout_mode.get() {
                    i18n::ide_toggle_editor(locale.get())
                } else {
                    i18n::app_shell_title(locale.get())
                }
            }}</span>
            <div class="shell-main-header-mobile-actions">
                <LayoutModeSegment
                    locale=locale
                    editor_layout_mode=editor_layout_mode
                    extra_class="mobile-layout-segment"
                />
                <button
                    type="button"
                    class="btn btn-secondary btn-sm shell-mobile-new-chat"
                    prop:style:display=move || {
                        if editor_layout_mode.get() {
                            "none"
                        } else {
                            ""
                        }
                    }
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
        </div>
    }
}
