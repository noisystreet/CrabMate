//! GitHub Pull Requests 侧栏卡片子组件。

use crate::api::{
    GithubPrCheckItem, GithubPrCurrentChecksData, GithubPrItem, GithubRepoContextData,
};
use crate::i18n::{self, Locale};
use leptos::prelude::*;

use super::status_tasks_state::StatusTasksSignals;

fn github_head_stat(
    locale: Locale,
    loading: bool,
    err: Option<String>,
    open_pr_count: usize,
) -> String {
    if loading {
        i18n::github_loading(locale).to_string()
    } else if err.is_some() {
        i18n::github_error(locale).to_string()
    } else {
        i18n::github_open_prs(locale, open_pr_count)
    }
}

fn github_not_connected_hint(repo: Option<GithubRepoContextData>, locale: Locale) -> String {
    if repo.as_ref().is_some_and(|r| !r.is_git_repo) {
        "当前工作区不是 git 仓库。".to_string()
    } else if repo.as_ref().is_some_and(|r| !r.gh_available) {
        "allowed_commands 未包含 gh，或 gh 不可用。".to_string()
    } else {
        i18n::github_not_connected(locale).to_string()
    }
}

#[component]
fn GithubRepoMetaSection(repo: GithubRepoContextData, locale: RwSignal<Locale>) -> impl IntoView {
    let default_branch = repo.default_branch.clone();
    let repo_url = repo.url.clone();
    view! {
        <div class="github-repo-meta">
            <div>
                <strong>{move || i18n::github_repo_label(locale.get_untracked())}</strong>
                " " {repo.repo.clone().unwrap_or_else(|| "—".to_string())}
            </div>
            <div>
                <strong>{move || i18n::github_branch_label(locale.get_untracked())}</strong>
                " " {repo.current_branch.clone().unwrap_or_else(|| "—".to_string())}
            </div>
            {default_branch.as_ref().map(|b| view! {
                <div class="github-default-branch">{format!("default: {b}")}</div>
            })}
            {repo_url.as_ref().map(|href| view! {
                <a class="github-repo-link" href=href.clone() target="_blank" rel="noopener noreferrer">{href.clone()}</a>
            })}
        </div>
    }
}

#[component]
fn GithubCheckRow(chk: GithubPrCheckItem) -> impl IntoView {
    let state = chk.state.clone();
    let bucket = chk.bucket.clone().unwrap_or_default();
    let name = chk.name.clone();
    let link = chk.link.clone();
    view! {
        <li>
            <span class="github-check-state">{state}</span>
            {if !bucket.is_empty() { format!(" ({bucket})") } else { String::new() }}
            " "
            {match link {
                Some(href) if !href.is_empty() => view! {
                    <a href=href target="_blank" rel="noopener noreferrer">{name}</a>
                }.into_any(),
                _ => view! { <span>{name}</span> }.into_any(),
            }}
        </li>
    }
}

#[component]
fn GithubChecksDetail(
    checks: GithubPrCurrentChecksData,
    locale: RwSignal<Locale>,
) -> impl IntoView {
    let n = checks.pr_number.unwrap_or(0);
    let s = checks.summary.clone();
    let pr_url = checks.pr_url.clone();
    let pr_title = checks.pr_title.clone();
    view! {
        <div class="github-current-pr">
            <div class="github-current-pr-title">
                {move || i18n::github_status_chip_pr(locale.get_untracked(), n, pr_title.as_deref().unwrap_or(""))}
            </div>
            {pr_url.as_ref().map(|href| view! {
                <a class="github-pr-link" href=href.clone() target="_blank" rel="noopener noreferrer">{href.clone()}</a>
            })}
            <div class="github-checks-summary">
                {move || format!(
                    "{} · {}",
                    i18n::github_checks_summary(locale.get_untracked(), s.passing, s.failing, s.pending),
                    s.total
                )}
            </div>
            <ul class="github-checks-list">
                {checks.checks.into_iter().map(|chk| view! { <GithubCheckRow chk=chk /> }).collect_view()}
            </ul>
        </div>
    }
}

#[component]
fn GithubChecksEmpty(locale: RwSignal<Locale>) -> impl IntoView {
    view! {
        <p class="github-panel-hint">{move || i18n::github_no_open_prs(locale.get())}</p>
    }
}

#[component]
fn GithubPrListRow(pr: GithubPrItem, stagger: String) -> impl IntoView {
    let title = pr.title.clone();
    let num = pr.number;
    let head = pr.head_ref.clone().unwrap_or_default();
    let base = pr.base_ref.clone().unwrap_or_default();
    let state = pr.state.clone();
    let is_draft = pr.is_draft.unwrap_or(false);
    let url = pr.url.clone();
    view! {
        <li style=format!("--list-stagger: {stagger}")>
            <span class="github-pr-num">{"#"}{num}</span>
            " "
            <span class="github-pr-title">{title}</span>
            <span class="github-pr-head">
                {head}
                {if !base.is_empty() { format!(" → {base}") } else { String::new() }}
                " " {state}
                {if is_draft { " draft" } else { "" }}
            </span>
            {url.as_ref().map(|href| view! {
                <a class="github-pr-link" href=href.clone() target="_blank" rel="noopener noreferrer">"GitHub"</a>
            })}
        </li>
    }
}

#[component]
fn GithubPrList(prs: RwSignal<crate::api::GithubPrsData>) -> impl IntoView {
    view! {
        <ul class=move || {
            if prs.get().items.is_empty() {
                "tasks-list"
            } else {
                "tasks-list list-stagger github-pr-list"
            }
        }>
            {move || {
                prs.get()
                    .items
                    .into_iter()
                    .enumerate()
                    .map(|(i, pr)| view! { <GithubPrListRow pr=pr stagger=i.to_string() /> })
                    .collect_view()
            }}
        </ul>
    }
}

#[component]
pub(crate) fn SideColumnGithubLoadedPane(
    locale: RwSignal<Locale>,
    github_err: RwSignal<Option<String>>,
    status_tasks: StatusTasksSignals,
) -> impl IntoView {
    view! {
        <div class="side-card-loaded github-panel-loaded">
            <Show when=move || github_err.get().is_some()>
                <div class="msg-error">{move || github_err.get().unwrap_or_default()}</div>
            </Show>
            {move || {
                let repo = status_tasks.github_repo.get();
                if repo.as_ref().is_none_or(|r| !r.connected) {
                    let hint = github_not_connected_hint(repo, locale.get_untracked());
                    view! { <p class="github-panel-hint">{hint}</p> }.into_any()
                } else {
                    view! { <GithubRepoMetaSection repo=repo.unwrap() locale=locale /> }.into_any()
                }
            }}
            <h4 class="github-checks-heading">{move || i18n::github_checks_heading(locale.get())}</h4>
            {move || {
                match status_tasks.github_checks.get() {
                    Some(c) if c.pr_number.is_some() => {
                        view! { <GithubChecksDetail checks=c locale=locale /> }.into_any()
                    }
                    Some(_) => view! { <GithubChecksEmpty locale=locale /> }.into_any(),
                    None => view! { <p class="github-panel-hint">"—"</p> }.into_any(),
                }
            }}
            <GithubPrList prs=status_tasks.github_prs />
        </div>
    }
}

#[component]
pub(crate) fn SideColumnGithubCard(
    locale: RwSignal<Locale>,
    github_loading: RwSignal<bool>,
    github_err: RwSignal<Option<String>>,
    status_tasks: StatusTasksSignals,
    refresh_github: std::sync::Arc<dyn Fn() + Send + Sync>,
) -> impl IntoView {
    view! {
        <div class="side-pane" style:flex="1" style:min-width="0">
            <div class="side-card">
                <div class="side-card-head">
                    <div class="side-head-main">
                        <div class="side-pane-title">{move || i18n::github_panel_title(locale.get())}</div>
                        <span class="side-head-stat">{move || {
                            github_head_stat(
                                locale.get(),
                                github_loading.get(),
                                github_err.get(),
                                status_tasks.github_prs.get().items.len(),
                            )
                        }}</span>
                    </div>
                    <button
                        type="button"
                        class="btn btn-secondary btn-sm side-head-action"
                        on:click={
                            let refresh_github = std::sync::Arc::clone(&refresh_github);
                            move |_| refresh_github()
                        }
                    >
                        {move || i18n::github_refresh(locale.get())}
                    </button>
                </div>
                <div class="side-card-body">
                    <Show when=move || github_loading.get()>
                        <div class="skeleton-stack" aria-busy="true" prop:aria-label=move || i18n::github_loading_aria(locale.get())>
                            <ul class="tasks-list tasks-list-skeleton">
                                <li><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                                <li><span class="skeleton skeleton-line skeleton-task-line"></span></li>
                            </ul>
                        </div>
                    </Show>
                    <Show when=move || !github_loading.get()>
                        <SideColumnGithubLoadedPane
                            locale=locale
                            github_err=github_err
                            status_tasks=status_tasks
                        />
                    </Show>
                </div>
            </div>
        </div>
    }
}
