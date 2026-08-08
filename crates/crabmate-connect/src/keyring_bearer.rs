//! 连接用 Web API Bearer：本机系统钥匙串（非 TOML / 非 localStorage）。
//!
//! 账户名与服务端 `web_api_bearer` 槽位区分，避免与 serve 主机钥匙串语义混淆。
//! Android 等无钥匙串后端的平台上读写失败时静默降级（连接页仍可用 localStorage）。

const KEYRING_SERVICE: &str = "com.crabmate.credentials";
/// Tauri 壳「连接页」客户端凭证（桌面 / 移动共用）。
const KEYRING_ACCOUNT: &str = "tauri_connect_web_api_bearer";

fn entry() -> Result<keyring::Entry, String> {
    keyring::Entry::new(KEYRING_SERVICE, KEYRING_ACCOUNT)
        .map_err(|e| format!("系统钥匙串不可用: {e}"))
}

/// 读取已保存的连接 Bearer；无条目或空串返回 `None`。
pub fn read_connect_bearer() -> Result<Option<String>, String> {
    let entry = entry()?;
    match entry.get_password() {
        Ok(secret) => {
            let t = secret.trim();
            if t.is_empty() {
                Ok(None)
            } else {
                Ok(Some(t.to_string()))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(format!("读取系统钥匙串失败: {e}")),
    }
}

/// **连接成功**：非空 `bearer` 写入（覆盖）系统钥匙串。
///
/// 返回 `Ok(true)` 表示本次写入；`Ok(false)` 表示跳过（空串）。
pub fn write_connect_bearer_on_connect(bearer: &str) -> Result<bool, String> {
    let b = bearer.trim();
    if b.is_empty() {
        return Ok(false);
    }
    write_connect_bearer_unchecked(b)?;
    Ok(true)
}

/// 无条件写入（测试 / 显式更新）。空串删除条目。
pub fn write_connect_bearer_unchecked(bearer: &str) -> Result<(), String> {
    let entry = entry()?;
    let b = bearer.trim();
    if b.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("清除系统钥匙串失败: {e}")),
        }
    } else {
        entry
            .set_password(b)
            .map_err(|e| format!("写入系统钥匙串失败: {e}"))
    }
}
