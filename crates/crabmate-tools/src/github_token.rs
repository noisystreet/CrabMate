//! 子进程 `gh` / Git HTTPS 的 GitHub token 解析与注入。
//!
//! 优先级：进程环境 **`GH_TOKEN` / `GITHUB_TOKEN`**（非空）→
//! 可选 [`set_token_provider`]（由宿主注册钥匙串读取）→ 否则依赖本机 `gh auth`。
//!
//! **勿**在日志中输出 token 明文。Git HTTPS 使用子进程环境变量
//! **`GIT_CONFIG_*`** 注入 `http.extraHeader`（避免把凭据写进 argv）。
//! GitHub App user token（`ghu_`）等对 smart HTTP **不接受** `Authorization: Bearer`，
//! 须用 **`Basic` + 用户名 `x-access-token`**（PAT / `gho_` 同样可用）。

use std::process::Command;
use std::sync::{Arc, OnceLock};

use base64::Engine;

type TokenProvider = Arc<dyn Fn() -> Option<String> + Send + Sync>;

static TOKEN_PROVIDER: OnceLock<TokenProvider> = OnceLock::new();

/// 注册钥匙串等回退源（进程内至多一次；重复调用忽略）。
pub fn set_token_provider(provider: TokenProvider) {
    let _ = TOKEN_PROVIDER.set(provider);
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

fn token_from_provider() -> Option<String> {
    TOKEN_PROVIDER
        .get()
        .and_then(|f| f())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// 明文 token：环境优先，否则 provider。供 Git HTTPS `Authorization` 头使用。
pub fn resolve_token_plaintext() -> Option<String> {
    token_from_env().or_else(token_from_provider)
}

/// 供 `gh` 子进程使用的 token：环境已有则 `None`（让子进程继承）；否则问 provider。
pub fn resolve_token_for_child_env() -> Option<String> {
    if env_github_token_set() {
        return None;
    }
    token_from_provider()
}

/// 若需注入，设置子进程 **`GH_TOKEN`**（`gh` 官方优先识别）。
pub fn apply_gh_token_env(cmd: &mut Command) {
    if let Some(token) = resolve_token_for_child_env() {
        cmd.env("GH_TOKEN", token);
    }
}

/// `run_command` / 直接 spawn 时：仅当命令名为 `gh` 时注入。
pub fn apply_gh_token_env_if_gh_command(cmd: &mut Command, command_name: &str) {
    let base = std::path::Path::new(command_name)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(command_name);
    if base == "gh" {
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
    fn resolve_skips_when_env_set() {
        // 不依赖真实钥匙串；仅验证「环境已设则返回 None」分支可调用。
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
        // 无 provider 且通常无环境时返回 None；非 github URL 必为 None。
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
}
