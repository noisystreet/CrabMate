#![recursion_limit = "256"]
// CSR 宏生成与大量闭包捕获使若干 style lint 噪声偏高；保持与主包 `-D warnings` 分离。
#![allow(clippy::collapsible_if)]
#![allow(clippy::redundant_locals)]
#![allow(clippy::clone_on_copy)]

mod api;
mod app;
mod app_prefs;
mod assistant_body;
mod markdown;
mod message_format;
mod session_export;
mod session_modal_row;
mod session_ops;
mod sse_dispatch;
mod storage;
mod workspace_shell;

use app::App;
use leptos::mount::mount_to_body;
use leptos::prelude::*;
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| view! { <App /> });
}
