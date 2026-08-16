//! 本机 XDG 用户目录解析（Config / Cache；Data 见 `crabmate-internal::user_data`）。

use std::path::PathBuf;

/// 覆盖本机用户配置根；未设则走 **`XDG_CONFIG_HOME`**。
pub const ENV_CONFIG_DIR: &str = "CM_CRABMATE_CONFIG_DIR";

/// 覆盖本机用户缓存根；未设则走 **`XDG_CACHE_HOME`**。
pub const ENV_CACHE_DIR: &str = "CM_CRABMATE_CACHE_DIR";

fn env_nonempty(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

fn home_join(rel: &str) -> Option<PathBuf> {
    std::env::var("HOME")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .map(|h| PathBuf::from(h).join(rel))
}

/// `$XDG_*_HOME/<app>`，或 `$HOME/<fallback_under_home>/<app>`，或 cwd 相对回退。
fn xdg_app_dir(
    override_env: &str,
    xdg_home_env: &str,
    fallback_under_home: &str,
    cwd_fallback: &str,
    app: &str,
) -> PathBuf {
    if let Some(p) = env_nonempty(override_env) {
        return p;
    }
    let base = env_nonempty(xdg_home_env)
        .or_else(|| home_join(fallback_under_home))
        .unwrap_or_else(|| PathBuf::from(cwd_fallback));
    base.join(app)
}

/// `$XDG_CONFIG_HOME/crabmate`，或 `~/.config/crabmate`；可由 **`CM_CRABMATE_CONFIG_DIR`** 覆盖。
#[must_use]
pub fn user_config_dir() -> PathBuf {
    xdg_app_dir(
        ENV_CONFIG_DIR,
        "XDG_CONFIG_HOME",
        ".config",
        ".config",
        "crabmate",
    )
}

/// `$XDG_CACHE_HOME/crabmate`，或 `~/.cache/crabmate`；可由 **`CM_CRABMATE_CACHE_DIR`** 覆盖。
///
/// 用于可清理的下载物（如 **fastembed** ONNX）；勿把会话/密钥放此处。
#[must_use]
pub fn user_cache_dir() -> PathBuf {
    xdg_app_dir(
        ENV_CACHE_DIR,
        "XDG_CACHE_HOME",
        ".cache",
        ".cache",
        "crabmate",
    )
}

/// 确保 `user_cache_dir()/name` 存在并返回该路径（`name` 为单段子目录名，如 `fastembed`）。
pub fn ensure_user_cache_subdir(name: &str) -> Result<PathBuf, String> {
    let name = name.trim().trim_matches('/');
    if name.is_empty() || name.contains('/') || name.contains('\\') || name == ".." {
        return Err(format!("非法缓存子目录名: {name:?}"));
    }
    let dir = user_cache_dir().join(name);
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("无法创建缓存目录 \"{}\": {e}", dir.display()))?;
    Ok(dir)
}

/// fastembed / ONNX 模型缓存目录（`$XDG_CACHE_HOME/crabmate/fastembed`）。
pub fn ensure_fastembed_cache_dir() -> Result<PathBuf, String> {
    ensure_user_cache_subdir("fastembed")
}

#[cfg(test)]
static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// 测试用：序列化所有会改 `CM_*` / `HOME` / cwd 的配置发现用例。
#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_subdir_rejects_path_segments() {
        let _g = test_env_lock();
        assert!(ensure_user_cache_subdir("../x").is_err());
        assert!(ensure_user_cache_subdir("a/b").is_err());
        assert!(ensure_user_cache_subdir("").is_err());
    }

    #[test]
    fn cache_override_env_used() {
        let _g = test_env_lock();
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("cache-root");
        // SAFETY: serialized by ENV_LOCK; test-only.
        unsafe {
            std::env::set_var(ENV_CACHE_DIR, &root);
        }
        let got = user_cache_dir();
        unsafe {
            std::env::remove_var(ENV_CACHE_DIR);
        }
        assert_eq!(got, root);
    }
}
