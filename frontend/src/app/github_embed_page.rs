//! GitHub PR 全屏嵌入页（与设置页同级 overlay；Tauri 在主窗口内挂载子 WebView）。

use gloo_timers::future::TimeoutFuture;
use leptos::prelude::*;
use leptos::task::spawn_local;
use leptos_dom::helpers::window_event_listener;
use wasm_bindgen::prelude::*;
use wasm_bindgen_futures::JsFuture;

use crate::i18n::{self, Locale};
use crate::tauri_shell;

#[derive(Clone, Copy)]
pub struct GithubEmbedSignals {
    pub open: RwSignal<bool>,
    pub url: RwSignal<Option<String>>,
    pub title: RwSignal<Option<String>>,
}

impl GithubEmbedSignals {
    pub fn from_modal(modal: crate::app::app_signals::ModalSignals) -> Self {
        Self {
            open: modal.github_embed_open,
            url: modal.github_embed_url,
            title: modal.github_embed_title,
        }
    }
}

pub fn use_github_embed_signals() -> GithubEmbedSignals {
    use_context::<GithubEmbedSignals>().expect("GithubEmbedSignals context")
}

/// 侧栏 GitHub 仓库按钮是否应禁用（与 `SideToolbarGithubRepoBtn` 的 `prop:disabled` 一致）。
pub fn github_repo_btn_disabled(repo: Option<&crate::api::GithubRepoContextData>) -> bool {
    let Some(r) = repo else {
        return true;
    };
    let no_url = r
        .url
        .as_deref()
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .is_none();
    no_url || !r.connected
}

pub fn open_github_embed_page(url: &str, title: Option<&str>, signals: GithubEmbedSignals) {
    if tauri_shell::tauri_linux_shell_available() {
        tauri_shell::tauri_open_github_webview(url, title);
        return;
    }
    signals.url.set(Some(url.to_string()));
    signals.title.set(title.map(str::to_string));
    signals.open.set(true);
}

pub fn try_open_github_embed_from_repo(
    repo: Option<crate::api::GithubRepoContextData>,
    locale: Locale,
    signals: GithubEmbedSignals,
) -> bool {
    let Some(r) = repo else {
        return false;
    };
    if !r.connected {
        return false;
    }
    let Some(url) = r.url.filter(|u| !u.trim().is_empty()) else {
        return false;
    };
    let title = r
        .repo
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| i18n::github_embed_title(locale).to_string());
    open_github_embed_page(&url, Some(title.as_str()), signals);
    true
}

pub fn close_github_embed_page(signals: GithubEmbedSignals) {
    signals.open.set(false);
    signals.url.set(None);
    signals.title.set(None);
    schedule_github_embed_unmount();
}

fn schedule_github_embed_unmount() {
    if !tauri_shell::tauri_shell_available() {
        return;
    }
    tauri_shell::tauri_unmount_github_embed();
}

#[wasm_bindgen(inline_js = r#"
export function measureGithubEmbedHostRect() {
  const page = document.querySelector("[data-testid='github-embed-page']");
  if (!page) return null;
  const w = window.innerWidth;
  const h = window.innerHeight;
  if (w < 1 || h < 1) return null;
  return [0, 0, w, h];
}

export function waitEmbedLayoutStable() {
  return new Promise((resolve) => {
    requestAnimationFrame(() => requestAnimationFrame(resolve));
  });
}

export function setGithubEmbedBodyTransparent(open) {
  if (open) {
    document.documentElement.setAttribute('data-github-embed-open', '');
  } else {
    document.documentElement.removeAttribute('data-github-embed-open');
  }
}
"#)]
extern "C" {
    #[wasm_bindgen(js_name = measureGithubEmbedHostRect)]
    fn measure_github_embed_host_rect() -> Option<js_sys::Array>;
    #[wasm_bindgen(js_name = waitEmbedLayoutStable)]
    fn wait_embed_layout_stable() -> js_sys::Promise;
    #[wasm_bindgen(js_name = setGithubEmbedBodyTransparent)]
    fn set_github_embed_body_transparent(open: bool);
}

fn read_embed_host_rect() -> Option<(f64, f64, f64, f64)> {
    let arr = measure_github_embed_host_rect()?;
    if arr.length() != 4 {
        return None;
    }
    Some((
        arr.get(0).as_f64()?,
        arr.get(1).as_f64()?,
        arr.get(2).as_f64()?,
        arr.get(3).as_f64()?,
    ))
}

fn sync_github_embed_if_ready(signals: GithubEmbedSignals) {
    if !signals.open.get_untracked() || !tauri_shell::tauri_shell_available() {
        return;
    }
    let Some(url) = signals.url.get_untracked().filter(|u| !u.trim().is_empty()) else {
        return;
    };
    let Some((x, y, w, h)) = read_embed_host_rect() else {
        return;
    };
    spawn_local(async move {
        let mount_result = tauri_shell::tauri_mount_github_embed(&url, x, y, w, h).await;
        if mount_result == Ok(true) {
            return;
        }
        if mount_result.is_err() {
            let title = signals.title.get_untracked();
            tauri_shell::tauri_open_github_webview(&url, title.as_deref());
        }
        // Linux 使用独立 WebViewWindow；关闭透明 overlay，恢复主界面。
        if signals.url.get_untracked().as_deref() == Some(url.as_str()) {
            close_github_embed_page(signals);
            set_github_embed_body_transparent(false);
        }
    });
}

async fn wait_embed_layout_then_sync(signals: GithubEmbedSignals) {
    if !signals.open.get_untracked() {
        return;
    }
    let _ = JsFuture::from(wait_embed_layout_stable()).await;
    if !signals.open.get_untracked() {
        return;
    }
    sync_github_embed_if_ready(signals);
}

fn wire_github_embed_mount(signals: GithubEmbedSignals) {
    if !tauri_shell::tauri_shell_available() {
        return;
    }
    // 阻止 Effect 首次执行时对初始 `false` 值触发 spurious unmount。
    let mounted = RwSignal::new(false);
    Effect::new(move |_| {
        let open = signals.open.get();
        // 支持同窗嵌入的平台上，子 WebView 位于 HTML 内容层下方；
        // 打开时须将 body 背景设为透明，否则会遮挡子 WebView。
        set_github_embed_body_transparent(open);
        if !open {
            // 仅在已经挂载过一次后才响应关闭，避免 Effect 初始探测阶段误卸载。
            if mounted.get_untracked() {
                tauri_shell::tauri_unmount_github_embed();
            }
            return;
        }
        mounted.set(true);
        let signals_stored = StoredValue::new(signals);
        let schedule_sync = move || {
            let signals = signals_stored.get_value();
            spawn_local(async move {
                wait_embed_layout_then_sync(signals).await;
                for delay in [50_u32, 150, 400] {
                    TimeoutFuture::new(delay).await;
                    if !signals.open.get_untracked() {
                        return;
                    }
                    sync_github_embed_if_ready(signals);
                }
            });
        };
        schedule_sync();
        let resize_handle = window_event_listener(leptos::ev::resize, move |_| schedule_sync());
        on_cleanup(move || {
            resize_handle.remove();
        });
    });
}

#[component]
pub fn GithubEmbedPageView(locale: RwSignal<Locale>, signals: GithubEmbedSignals) -> impl IntoView {
    wire_github_embed_mount(signals);
    let on_back = move |_| close_github_embed_page(signals);
    view! {
        <Show when=move || signals.open.get()>
            <div
                class="settings-page settings-page-visible github-embed-page"
                data-testid="github-embed-page"
            >
                <div id="github-embed-host" class="github-embed-host"></div>
                <Show when=move || !tauri_shell::tauri_shell_available()>
                    <div class="github-embed-browser-fallback">
                        <p>{move || i18n::github_embed_browser_hint(locale.get())}</p>
                    </div>
                </Show>
                <div class="settings-page-header github-embed-page-header">
                    <button
                        type="button"
                        class="btn btn-ghost settings-page-back"
                        data-testid="github-embed-back"
                        on:click=on_back
                    >
                        <svg
                            viewBox="0 0 24 24"
                            fill="none"
                            stroke="currentColor"
                            stroke-width="2"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                            aria-hidden="true"
                        >
                            <polyline points="15 18 9 12 15 6" />
                        </svg>
                        <span>{move || i18n::github_embed_back(locale.get())}</span>
                    </button>
                    <h1 class="settings-page-title github-embed-page-title">
                        {move || {
                            signals
                                .title
                                .get()
                                .filter(|t| !t.trim().is_empty())
                                .unwrap_or_else(|| i18n::github_embed_title(locale.get()).to_string())
                        }}
                    </h1>
                    <span class="settings-page-head-spacer"></span>
                    <Show when=move || signals.url.get().is_some() && !tauri_shell::tauri_shell_available()>
                        <button
                            type="button"
                            class="btn btn-secondary btn-sm"
                            data-testid="github-embed-open-browser"
                            on:click=move |_| {
                                if let Some(href) = signals.url.get_untracked() {
                                    tauri_shell::tauri_open_external_url(&href);
                                }
                            }
                        >
                            {move || i18n::github_embed_open_browser(locale.get())}
                        </button>
                    </Show>
                </div>
            </div>
        </Show>
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::GithubRepoContextData;

    fn repo(connected: bool, url: Option<&str>) -> GithubRepoContextData {
        GithubRepoContextData {
            connected,
            url: url.map(str::to_string),
            repo: Some("octocat/Hello-World".to_string()),
            ..Default::default()
        }
    }

    #[test]
    fn github_repo_btn_disabled_when_missing_or_not_connected() {
        assert!(github_repo_btn_disabled(None));
        assert!(github_repo_btn_disabled(Some(&repo(
            false,
            Some("https://github.com/octocat/Hello-World",)
        ))));
        assert!(github_repo_btn_disabled(Some(&repo(true, None))));
        assert!(github_repo_btn_disabled(Some(&repo(true, Some("  ")))));
    }

    #[test]
    fn github_repo_btn_enabled_when_connected_with_url() {
        assert!(!github_repo_btn_disabled(Some(&repo(
            true,
            Some("https://github.com/octocat/Hello-World"),
        ))));
    }

    #[test]
    fn try_open_github_embed_from_repo_requires_connected_and_url() {
        let locale = Locale::ZhHans;
        let open = RwSignal::new(false);
        let url = RwSignal::new(None);
        let title = RwSignal::new(None);
        let signals = GithubEmbedSignals { open, url, title };

        assert!(!try_open_github_embed_from_repo(None, locale, signals));
        assert!(!open.get_untracked());

        assert!(!try_open_github_embed_from_repo(
            Some(repo(false, Some("https://github.com/a/b"))),
            locale,
            signals,
        ));
        assert!(!open.get_untracked());

        assert!(try_open_github_embed_from_repo(
            Some(repo(true, Some("https://github.com/a/b"))),
            locale,
            signals,
        ));
        assert!(open.get_untracked());
        assert_eq!(
            url.get_untracked().as_deref(),
            Some("https://github.com/a/b")
        );
    }
}
