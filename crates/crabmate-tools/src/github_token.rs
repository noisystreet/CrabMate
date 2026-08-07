//! 子进程 `gh` 的 GitHub token 解析与注入。
//!
//! 优先级：进程环境 **`GH_TOKEN` / `GITHUB_TOKEN`**（非空则继承，不覆盖）→
//! 可选 [`set_token_provider`]（由宿主注册钥匙串读取）→ 否则依赖本机 `gh auth`。
//!
//! **勿**在日志中输出 token 明文。

use std::process::Command;
use std::sync::{Arc, OnceLock};

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

/// 供子进程使用的 token：环境已有则 `None`（让子进程继承）；否则问 provider。
pub fn resolve_token_for_child_env() -> Option<String> {
    if env_github_token_set() {
        return None;
    }
    TOKEN_PROVIDER
        .get()
        .and_then(|f| f())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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
}
