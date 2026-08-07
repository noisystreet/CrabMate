//! GitHub OAuth / PAT：系统钥匙串账户 **`github`**（user token）与 **`github_oauth_client_id`**（App Client ID）。

use std::path::PathBuf;
use std::sync::Arc;

use super::path::user_data_root;

fn secret_path() -> PathBuf {
    user_data_root().join("secrets").join("github")
}

fn oauth_client_id_secret_path() -> PathBuf {
    user_data_root()
        .join("secrets")
        .join("github_oauth_client_id")
}

/// 写入或清除（空串）GitHub user access token / PAT。
pub fn write_secret_github(token: &str) -> Result<(), String> {
    super::credential_store::write_migrating_secret("github", &secret_path(), token)
}

pub fn read_secret_github() -> Option<String> {
    super::credential_store::read_migrating_secret("github", &secret_path())
}

/// 写入或清除 GitHub App / OAuth App **Client ID**（账户 `github_oauth_client_id`）。
/// 非 Client Secret；仍走钥匙串以免出现在明文 prefs / status 全文。
pub fn write_secret_github_oauth_client_id(client_id: &str) -> Result<(), String> {
    super::credential_store::write_migrating_secret(
        "github_oauth_client_id",
        &oauth_client_id_secret_path(),
        client_id,
    )
}

pub fn read_secret_github_oauth_client_id() -> Option<String> {
    super::credential_store::read_migrating_secret(
        "github_oauth_client_id",
        &oauth_client_id_secret_path(),
    )
}

/// 环境变量 **`CM_GITHUB_OAUTH_CLIENT_ID`** 是否非空（不回显值）。
pub fn github_oauth_client_id_env_is_set() -> bool {
    std::env::var("CM_GITHUB_OAUTH_CLIENT_ID")
        .ok()
        .is_some_and(|s| !s.trim().is_empty())
}

/// 将钥匙串 `github` 注册为 `crabmate_tools::github_token` 回退源（进程内一次）。
pub fn install_github_cli_token_provider() {
    crabmate_tools::github_token::set_token_provider(Arc::new(|| {
        read_secret_github().filter(|s| !s.trim().is_empty())
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_client_id_write_read_clear_and_status_suffix() {
        let _root = super::super::path::ensure_test_user_data_root();
        let _secrets = super::super::credential_store::lock_test_named_secret_suite();
        write_secret_github_oauth_client_id("Iv1.abcdEF12").expect("write");
        assert_eq!(
            read_secret_github_oauth_client_id().as_deref(),
            Some("Iv1.abcdEF12")
        );
        let st = super::super::store::secrets_status();
        assert!(st.github_oauth_client_id.set);
        assert_eq!(st.github_oauth_client_id.suffix.as_deref(), Some("EF12"));
        write_secret_github_oauth_client_id("").expect("clear");
        assert!(read_secret_github_oauth_client_id().is_none());
        assert!(
            !super::super::store::secrets_status()
                .github_oauth_client_id
                .set
        );
    }
}
