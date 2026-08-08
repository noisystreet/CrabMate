//! 侧栏 GitHub 仓库按钮：在系统浏览器打开仓库页。

use crate::i18n::Locale;
use crate::tauri_shell;

/// 侧栏按钮能否直接打开仓库页（有非空 URL 即可；不要求 Device Flow / `gh` 已连接）。
pub fn github_repo_can_open(repo: Option<&crate::api::GithubRepoContextData>) -> bool {
    repo.and_then(|r| r.url.as_deref())
        .map(str::trim)
        .is_some_and(|u| !u.is_empty())
}

/// 通过系统浏览器打开 GitHub 仓库页。
pub fn open_github_repo_url(url: &str) {
    tauri_shell::tauri_open_external_url(url);
}

pub fn try_open_github_embed_from_repo(
    repo: Option<crate::api::GithubRepoContextData>,
    _locale: Locale,
) -> bool {
    let Some(r) = repo else {
        return false;
    };
    let Some(url) = r.url.filter(|u| !u.trim().is_empty()) else {
        return false;
    };
    open_github_repo_url(&url);
    true
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
    fn github_repo_can_open_requires_url_not_connected_flag() {
        assert!(!github_repo_can_open(None));
        assert!(github_repo_can_open(Some(&repo(
            false,
            Some("https://github.com/octocat/Hello-World")
        ))));
        assert!(!github_repo_can_open(Some(&repo(true, None))));
        assert!(!github_repo_can_open(Some(&repo(true, Some("  ")))));
        assert!(github_repo_can_open(Some(&repo(
            true,
            Some("https://github.com/octocat/Hello-World")
        ))));
    }

    #[test]
    fn try_open_requires_url() {
        assert!(!try_open_github_embed_from_repo(None, Locale::ZhHans));
        assert!(!try_open_github_embed_from_repo(
            Some(repo(true, None)),
            Locale::ZhHans
        ));
        assert!(try_open_github_embed_from_repo(
            Some(repo(false, Some("https://github.com/octocat/Hello-World"))),
            Locale::ZhHans
        ));
    }
}
