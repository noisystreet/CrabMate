//! 子进程 `gh` / Git HTTPS 的 GitHub token 解析与注入。
//!
//! 优先级：进程环境 **`GH_TOKEN` / `GITHUB_TOKEN`**（非空）→
//! 当前请求作用域 token（HTTP 头 / Cookie，经 [`with_request_github_token`]）→
//! 否则依赖本机 `gh auth`（子进程自行解析，本模块不注入）。
//!
//! **勿**在日志中输出 token 明文。Git HTTPS 使用子进程环境变量
//! **`GIT_CONFIG_*`** 注入 `http.extraHeader`（避免把凭据写进 argv）。
//! GitHub App user token（`ghu_`）等对 smart HTTP **不接受** `Authorization: Bearer`，
//! 须用 **`Basic` + 用户名 `x-access-token`**（PAT / `gho_` 同样可用）。

use std::future::Future;
use std::process::Command;

use base64::Engine;

tokio::task_local! {
    static REQUEST_GITHUB_TOKEN: Option<String>;
}

/// 在异步作用域内设置本请求的 GitHub user token（供工具 / clone 注入）。
///
/// Chat 队列 worker 须在执行回合时再次 [`with_request_github_token`]，因 HTTP 中间件作用域在入队后结束。
/// `spawn_blocking` 内须用 [`with_request_github_token_blocking`]（task-local 不会自动传到阻塞线程）。
pub async fn with_request_github_token<F, R>(token: Option<String>, fut: F) -> R
where
    F: Future<Output = R>,
{
    let cleaned = token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    REQUEST_GITHUB_TOKEN.scope(cleaned, fut).await
}

/// 在 **阻塞线程**（如 `spawn_blocking`）内设置请求作用域 token。
pub fn with_request_github_token_blocking<F, R>(token: Option<String>, f: F) -> R
where
    F: FnOnce() -> R,
{
    let cleaned = token
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    REQUEST_GITHUB_TOKEN.sync_scope(cleaned, f)
}

fn env_github_token_set() -> bool {
    ["GH_TOKEN", "GITHUB_TOKEN"].iter().any(|k| {
        std::env::var(k)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false)
    })
}

fn token_from_env() -> Option<String> {
    for k in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Ok(v) = std::env::var(k) {
            let t = v.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

fn token_from_request() -> Option<String> {
    REQUEST_GITHUB_TOKEN
        .try_with(|t| t.clone())
        .ok()
        .flatten()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 明文 token：环境优先，否则请求作用域。供 Git HTTPS `Authorization` 头使用。
pub fn resolve_token_plaintext() -> Option<String> {
    token_from_env().or_else(token_from_request)
}

/// 供 `gh` 子进程使用的 token：环境已有则 `None`（让子进程继承）；否则用请求作用域。
pub fn resolve_token_for_child_env() -> Option<String> {
    if env_github_token_set() {
        return None;
    }
    token_from_request()
}

/// 若需注入，设置子进程 **`GH_TOKEN`**（`gh` 官方优先识别）。
pub fn apply_gh_token_env(cmd: &mut Command) {
    if let Some(token) = resolve_token_for_child_env() {
        cmd.env("GH_TOKEN", token);
    }
}

/// `run_command` / 直接 spawn 时：仅当命令名为 `gh` 时注入。
pub fn command_basename_is_gh(command_name: &str) -> bool {
    let base = std::path::Path::new(command_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command_name);
    base.eq_ignore_ascii_case("gh")
}

/// `run_command` / 直接 spawn 时：仅当命令名为 `gh` 时注入。
pub fn apply_gh_token_env_if_gh_command(cmd: &mut Command, command_name: &str) {
    if command_basename_is_gh(command_name) {
        apply_gh_token_env(cmd);
    }
}

/// 是否为 GitHub.com 的 HTTPS 远程（含可选 `www.`；不含 SSH / 其他 host）。
pub fn is_github_https_url(url: &str) -> bool {
    let u = url.trim();
    if u.is_empty() {
        return false;
    }
    let lower = u.to_ascii_lowercase();
    lower.starts_with("https://github.com/") || lower.starts_with("https://www.github.com/")
}

/// Git HTTPS：`Authorization: Basic base64(x-access-token:<token>)`。
///
/// Device Flow / GitHub App 的 **`ghu_`** user token 对 git smart HTTP 拒绝 Bearer；
/// 官方推荐用户名 **`x-access-token`**、密码为 token（Basic）。
pub fn github_https_authorization_header(token: &str) -> String {
    let basic = base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{token}"));
    format!("Authorization: Basic {basic}")
}

/// 为 Git 子进程准备的环境键值（含 `GIT_TERMINAL_PROMPT=0` 与 `http.extraHeader`）。
/// 仅当 URL 命中 GitHub HTTPS 且能解析到 token 时返回 `Some`。
pub fn github_https_auth_envs(remote_url: &str) -> Option<Vec<(String, String)>> {
    if !is_github_https_url(remote_url) {
        return None;
    }
    let token = resolve_token_plaintext()?;
    Some(vec![
        ("GIT_TERMINAL_PROMPT".into(), "0".into()),
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "http.extraHeader".into()),
        (
            "GIT_CONFIG_VALUE_0".into(),
            github_https_authorization_header(&token),
        ),
    ])
}

/// 对 [`std::process::Command`] 注入 GitHub HTTPS 认证环境（见 [`github_https_auth_envs`]）。
pub fn apply_github_https_auth(cmd: &mut Command, remote_url: &str) -> bool {
    let Some(pairs) = github_https_auth_envs(remote_url) else {
        return false;
    };
    for (k, v) in pairs {
        cmd.env(k, v);
    }
    true
}

/// 对任意支持 `.env(K, V)` 的命令构建器注入（如 `tokio::process::Command`）。
pub fn apply_github_https_auth_pairs<F>(remote_url: &str, mut set_env: F) -> bool
where
    F: FnMut(&str, String),
{
    let Some(pairs) = github_https_auth_envs(remote_url) else {
        return false;
    };
    for (k, v) in pairs {
        set_env(&k, v);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_basename_is_gh_matches_path() {
        assert!(command_basename_is_gh("gh"));
        assert!(command_basename_is_gh("/usr/bin/gh"));
        assert!(!command_basename_is_gh("bash"));
    }

    #[test]
    fn resolve_skips_when_env_set() {
        if env_github_token_set() {
            assert!(resolve_token_for_child_env().is_none());
        }
    }

    #[test]
    fn is_github_https_url_accepts_https_github() {
        assert!(is_github_https_url("https://github.com/a/b.git"));
        assert!(is_github_https_url(" https://GitHub.com/a/b "));
        assert!(is_github_https_url("https://www.github.com/org/repo"));
        assert!(!is_github_https_url("http://github.com/a/b"));
        assert!(!is_github_https_url("git@github.com:a/b.git"));
        assert!(!is_github_https_url("https://gitlab.com/a/b.git"));
        assert!(!is_github_https_url(""));
    }

    #[test]
    fn github_https_auth_envs_none_without_token_or_non_github() {
        assert!(github_https_auth_envs("https://gitlab.com/a/b.git").is_none());
        assert!(github_https_auth_envs("git@github.com:a/b.git").is_none());
    }

    #[test]
    fn github_https_authorization_header_is_basic_x_access_token() {
        let h = github_https_authorization_header("ghu_test_token");
        assert!(h.starts_with("Authorization: Basic "));
        let b64 = h.strip_prefix("Authorization: Basic ").expect("prefix");
        let raw = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(b64)
                .expect("b64"),
        )
        .expect("utf8");
        assert_eq!(raw, "x-access-token:ghu_test_token");
        assert!(!h.contains("Bearer"));
        assert!(!h.contains("ghu_test_token"));
    }

    #[tokio::test]
    async fn request_scope_token_is_visible_inside_scope() {
        assert!(token_from_request().is_none());
        with_request_github_token(Some("ghu_scope".into()), async {
            assert_eq!(token_from_request().as_deref(), Some("ghu_scope"));
            if !env_github_token_set() {
                assert_eq!(resolve_token_plaintext().as_deref(), Some("ghu_scope"));
            }
        })
        .await;
        assert!(token_from_request().is_none());
    }

    #[test]
    fn blocking_scope_token_visible_on_thread() {
        assert!(token_from_request().is_none());
        with_request_github_token_blocking(Some("ghu_block".into()), || {
            assert_eq!(token_from_request().as_deref(), Some("ghu_block"));
        });
        assert!(token_from_request().is_none());
    }
}
