//! 主区内 IDE 布局：菜单栏 + 工作区树 + 多标签编辑器（`GET/POST /workspace/file`）。

use std::sync::Arc;

use leptos::prelude::*;

use crate::chat_session_state::ChatSessionSignals;
use crate::i18n::{self, Locale};
use crate::ide_tabs::{
    IdeTabsHandle, make_ide_open_file_handler, wire_ide_editor_sync_to_active_tab,
};

use super::ide_editor_pane::IdeEditorPane;
use super::ide_menu_bar::{IdeMenuBar, IdeMenuBarSignals};
use super::ide_tabs_bar::{IdeTabsBar, IdeTabsBarInput};
use super::side_column_workspace_scroll::WorkspaceSideCardScrollInner;
use super::workspace_panel_state::WorkspacePanelSignals;
use crate::app::app_signals::IdeEditorSignals;
use crate::ide_tabs::IdeTabsEditorSignals;

#[component]
fn IdeLayoutLeftPane(
    locale: RwSignal<Locale>,
    chat: ChatSessionSignals,
    workspace_panel: WorkspacePanelSignals,
    noop_sv: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
    open_sv: StoredValue<Arc<dyn Fn(String) + Send + Sync>>,
) -> impl IntoView {
    view! {
        <div class="ide-layout-left">
            <div class="ide-layout-left-head">
                <div class="ide-pane-title">{move || i18n::ide_workspace_title(locale.get())}</div>
                <p class="ide-open-hint">{move || i18n::ide_open_hint(locale.get())}</p>
            </div>
            <div class="ide-workspace-scroll">
                <WorkspaceSideCardScrollInner
                    locale=locale
                    chat=chat
                    ws=workspace_panel
                    insert_workspace_file_ref=noop_sv
                    on_file_single_click=open_sv
                />
            </div>
        </div>
    }
}

#[derive(Clone, Copy)]
struct IdeLayoutRightPaneInput {
    locale: RwSignal<Locale>,
    editor: IdeEditorSignals,
    tabs: IdeTabsHandle,
    ide_path: RwSignal<Option<String>>,
    ide_text: RwSignal<String>,
    ide_baseline: RwSignal<String>,
    ide_load_busy: RwSignal<bool>,
    ide_err: RwSignal<Option<String>>,
    textarea_ref: NodeRef<leptos::html::Textarea>,
}

#[component]
fn IdeLayoutRightPane(input: IdeLayoutRightPaneInput) -> impl IntoView {
    let IdeLayoutRightPaneInput {
        locale,
        editor,
        tabs,
        ide_path,
        ide_text,
        ide_baseline,
        ide_load_busy,
        ide_err,
        textarea_ref,
    } = input;
    view! {
        <div class="ide-layout-right">
            <Show when=move || ide_err.get().is_some()>
                <div class="msg-error ide-editor-err">{move || ide_err.get().unwrap_or_default()}</div>
            </Show>
            <Show when=move || ide_load_busy.get()>
                <p class="ide-editor-loading" role="status">"…"</p>
            </Show>
            <IdeTabsBar input=IdeTabsBarInput {
                locale,
                tabs,
                editor: IdeTabsEditorSignals {
                    ide_path,
                    ide_text,
                    ide_baseline,
                },
            } />
            <IdeEditorPane
                locale=locale
                editor=editor
                ide_path=ide_path
                ide_text=ide_text
                ide_load_busy=ide_load_busy
                textarea_ref=textarea_ref
            />
        </div>
    }
}

/// 主壳传入 IDE 布局的只读信号 bundle（控制形参个数棘轮）。
#[derive(Clone)]
pub struct IdeLayoutShellSignals {
    pub locale: RwSignal<Locale>,
    pub editor: IdeEditorSignals,
    pub editor_layout_mode: RwSignal<bool>,
    pub ide_settings_page: RwSignal<bool>,
    pub ide_menubar_dropdown_open: RwSignal<bool>,
    pub chat: ChatSessionSignals,
    pub workspace_panel: WorkspacePanelSignals,
    pub refresh_workspace: Arc<dyn Fn() + Send + Sync>,
    pub initialized: RwSignal<bool>,
    /// 与主壳「编辑器布局」一致；不可见时不触发工作区刷新，避免后台无意义请求。
    pub editor_visible: RwSignal<bool>,
}

#[component]
pub fn IdeLayoutView(shell: IdeLayoutShellSignals) -> impl IntoView {
    let IdeLayoutShellSignals {
        locale,
        editor,
        editor_layout_mode,
        ide_settings_page,
        ide_menubar_dropdown_open,
        chat,
        workspace_panel,
        refresh_workspace,
        initialized,
        editor_visible,
    } = shell;

    let tabs = IdeTabsHandle::new();
    let ide_path = RwSignal::new(None::<String>);
    let ide_text = RwSignal::new(String::new());
    let ide_baseline = RwSignal::new(String::new());
    let textarea_ref = NodeRef::<leptos::html::Textarea>::new();

    wire_ide_editor_sync_to_active_tab(tabs, tabs.active, ide_text);

    let noop: Arc<dyn Fn(String) + Send + Sync> = Arc::new(|_| {});
    let noop_sv = StoredValue::new(noop);

    let refresh_ws = refresh_workspace.clone();
    Effect::new(move |_| {
        if !editor_visible.get() {
            return;
        }
        if initialized.get() {
            refresh_ws();
        }
    });

    let tab_editor = IdeTabsEditorSignals {
        ide_path,
        ide_text,
        ide_baseline,
    };
    let open_file = make_ide_open_file_handler(locale, tabs, tab_editor);
    let open_sv = StoredValue::new(open_file);

    view! {
        <div class="ide-layout-root" data-testid="ide-layout-root">
            <IdeMenuBar signals=IdeMenuBarSignals {
                locale,
                editor,
                editor_layout_mode,
                ide_settings_page,
                ide_menubar_dropdown_open,
                ide_path,
                ide_text,
                ide_baseline,
                ide_load_busy: tabs.load_busy,
                ide_save_busy: tabs.save_busy,
                ide_err: tabs.err,
                textarea_ref,
                tabs,
            } />
            <div class="ide-layout-body">
                <IdeLayoutLeftPane
                    locale=locale
                    chat=chat
                    workspace_panel=workspace_panel
                    noop_sv=noop_sv
                    open_sv=open_sv
                />
                <IdeLayoutRightPane input=IdeLayoutRightPaneInput {
                    locale,
                    editor,
                    tabs,
                    ide_path,
                    ide_text,
                    ide_baseline,
                    ide_load_busy: tabs.load_busy,
                    ide_err: tabs.err,
                    textarea_ref,
                } />
            </div>
        </div>
    }
}
