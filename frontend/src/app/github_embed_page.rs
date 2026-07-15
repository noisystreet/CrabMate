//! GitHub 嵌入页（浏览器模式 overlay；Tauri 直接打开系统浏览器）。

use leptos::prelude::*;

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

/// 所有平台均通过系统浏览器打开 GitHub 仓库页。
pub fn open_github_embed_page(url: &str, _title: Option<&str>, _signals: GithubEmbedSignals) {
    tauri_shell::tauri_open_external_url(url);
}

pub fn try_open_github_embed_from_repo(
    repo: Option<crate::api::GithubRepoContextData>,
    _locale: Locale,
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
    open_github_embed_page(&url, None, signals);
    true
}

pub fn close_github_embed_page(signals: GithubEmbedSignals) {
    signals.open.set(false);
    signals.url.set(None);
    signals.title.set(None);
}

#[component]
pub fn GithubEmbedPageView(locale: RwSignal<Locale>, signals: GithubEmbedSignals) -> impl IntoView {
    view! {
        <Show when=move || signals.open.get()>
            <div
                class="settings-page settings-page-visible github-embed-page"
                data-testid="github-embed-page"
            >
                <div class="github-embed-browser-fallback">
                    <p>{move || i18n::github_embed_browser_hint(locale.get())}</p>
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
                </div>
                <div class="settings-page-header github-embed-page-header">
                    <button
                        type="button"
                        class="btn btn-ghost settings-page-back"
                        data-testid="github-embed-back"
                        on:click=move |_| close_github_embed_page(signals)
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
        let open = RwSignal::new(false);
        let url = RwSignal::new(None);
        let title = RwSignal::new(None);
        let signals = GithubEmbedSignals { open, url, title };

        assert!(!try_open_github_embed_from_repo(
            None,
            Locale::ZhHans,
            signals
        ));
        assert!(!open.get_untracked());
    }
}
