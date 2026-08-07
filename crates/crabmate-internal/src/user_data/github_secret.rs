//! GitHub OAuth / PAT：系统钥匙串账户 **`github`**。

use std::path::PathBuf;
use std::sync::Arc;

use super::path::user_data_root;

fn secret_path() -> PathBuf {
    user_data_root().join("secrets").join("github")
}

/// 写入或清除（空串）GitHub token。
pub fn write_secret_github(token: &str) -> Result<(), String> {
    super::credential_store::write_migrating_secret("github", &secret_path(), token)
}

pub fn read_secret_github() -> Option<String> {
    super::credential_store::read_migrating_secret("github", &secret_path())
}

/// 将钥匙串 `github` 注册为 `crabmate_tools::github_token` 回退源（进程内一次）。
pub fn install_github_cli_token_provider() {
    crabmate_tools::github_token::set_token_provider(Arc::new(|| {
        read_secret_github().filter(|s| !s.trim().is_empty())
    }));
}
