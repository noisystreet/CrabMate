#![recursion_limit = "256"]
// `cdylib` + 测试 harness 在 wasm32 上会与生成的 `main` 冲突；WASM 单测走 `wasm-bindgen-test`。
#![cfg_attr(all(test, target_arch = "wasm32"), no_main)]
// CSR 宏生成与大量闭包捕获使若干 style lint 噪声偏高；保持与主包 `-D warnings` 分离。
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::clone_on_copy)]

mod a11y;
mod api;
mod app;
mod app_prefs;
mod assistant_body;
mod chat_actions;
mod chat_session_state;
mod clarification_form;
mod client_llm_presets;
mod confirm_dialog;
mod conversation_hydrate;
mod conversation_hydrate_timeline;
mod conversation_messages_page;
mod conversation_prompt_tokens_apply;
mod debounce_schedule;
mod i18n;
mod ide_codemirror;
mod ide_confirm;
mod ide_disk_sync;
mod ide_editor_prefs;
mod ide_find;
mod ide_save;
mod ide_syntax_highlight;
mod ide_tabs;
mod markdown;
mod message_dedupe;
mod message_format;
mod message_loading;
mod message_render;
mod scroll_anchor;
mod session_export;
mod session_modal_row;
mod session_ops;
mod session_search;
mod session_sort;
mod session_sync;
mod session_typography_prefs;
mod session_workspace_bind;
mod session_workspace_partition;
mod settings_llm_fields;
mod sse_dispatch;
mod storage;
mod stream_text_overlay;
mod tauri_shell;
mod timeline_scan;
mod user_data_bootstrap;
mod user_prefs_sync;
mod visible_messages;
mod workspace_context_menu;
mod workspace_fs_ops;
mod workspace_shell;
mod workspace_tree;

use app::App;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
