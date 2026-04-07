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
mod debounce_schedule;
mod i18n;
mod markdown;
mod message_format;
mod session_export;
mod session_modal_row;
mod session_ops;
mod session_search;
mod sse_dispatch;
mod storage;
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
