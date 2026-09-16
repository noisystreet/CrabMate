//! 持久密钥的系统钥匙串适配（系统钥匙串是唯一来源）。

#[cfg(test)]
use std::sync::Mutex;

#[cfg(not(test))]
const KEYRING_SERVICE: &str = "com.crabmate.credentials";

trait SecretEntry {
    fn get_password(&self) -> Result<Option<String>, String>;
    fn set_password(&self, password: &str) -> Result<(), String>;
    fn delete_credential(&self) -> Result<(), String>;
}

#[cfg(not(test))]
struct SystemSecretEntry {
    inner: keyring::Entry,
}

#[cfg(not(test))]
impl SystemSecretEntry {
    fn new(account: &str) -> Result<Self, String> {
        keyring::Entry::new(KEYRING_SERVICE, account)
            .map(|inner| Self { inner })
            .map_err(keyring_error)
    }
}

#[cfg(not(test))]
impl SecretEntry for SystemSecretEntry {
    fn get_password(&self) -> Result<Option<String>, String> {
        match self.inner.get_password() {
            Ok(secret) => Ok(Some(secret)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(keyring_error(error)),
        }
    }

    fn set_password(&self, password: &str) -> Result<(), String> {
        self.inner.set_password(password).map_err(keyring_error)
    }

    fn delete_credential(&self) -> Result<(), String> {
        match self.inner.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(keyring_error(error)),
        }
    }
}

#[cfg(not(test))]
fn keyring_error(error: keyring::Error) -> String {
    format!("系统钥匙串操作失败: {error}")
}

fn normalized(secret: Option<String>) -> Option<String> {
    secret
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn read_entry_secret(entry: &dyn SecretEntry) -> Result<Option<String>, String> {
    Ok(normalized(entry.get_password()?))
}

fn write_entry_secret(entry: &dyn SecretEntry, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        entry.delete_credential()
    } else {
        entry.set_password(secret)
    }
}

#[cfg(not(test))]
pub(super) fn read_secret(account: &str) -> Option<String> {
    match SystemSecretEntry::new(account).and_then(|entry| read_entry_secret(&entry)) {
        Ok(secret) => secret,
        Err(error) => {
            tracing::debug!(target: "crabmate", account, error = %error, "读取系统钥匙串失败");
            None
        }
    }
}

#[cfg(test)]
pub(super) fn read_secret(account: &str) -> Option<String> {
    normalized(read_named_secret(account))
}

#[cfg(not(test))]
pub(super) fn write_secret(account: &str, secret: &str) -> Result<(), String> {
    let entry = SystemSecretEntry::new(account)?;
    write_entry_secret(&entry, secret)
}

#[cfg(test)]
pub(super) fn write_secret(account: &str, secret: &str) -> Result<(), String> {
    write_named_secret(account, secret)
}

#[cfg(test)]
fn test_named_secrets() -> &'static Mutex<std::collections::HashMap<String, String>> {
    static SECRETS: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    SECRETS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

/// 序列化所有「命名钥匙串账户」相关单测（与 `store` / `github_secret` 共用）。
#[cfg(test)]
pub(super) fn lock_test_named_secret_suite() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
fn read_named_secret(account: &str) -> Option<String> {
    test_named_secrets()
        .lock()
        .expect("test named secrets lock")
        .get(account)
        .cloned()
}

#[cfg(test)]
pub(super) fn write_named_secret(account: &str, secret: &str) -> Result<(), String> {
    let mut secrets = test_named_secrets()
        .lock()
        .expect("test named secrets lock");
    let secret = secret.trim();
    if secret.is_empty() {
        secrets.remove(account);
    } else {
        secrets.insert(account.to_string(), secret.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeEntry {
        secret: Mutex<Option<String>>,
        fail_set: bool,
    }

    impl SecretEntry for FakeEntry {
        fn get_password(&self) -> Result<Option<String>, String> {
            Ok(self.secret.lock().expect("fake secret lock").clone())
        }

        fn set_password(&self, password: &str) -> Result<(), String> {
            if self.fail_set {
                return Err("mock keyring unavailable".to_string());
            }
            *self.secret.lock().expect("fake secret lock") = Some(password.to_string());
            Ok(())
        }

        fn delete_credential(&self) -> Result<(), String> {
            *self.secret.lock().expect("fake secret lock") = None;
            Ok(())
        }
    }

    #[test]
    fn reads_trimmed_keyring_value() {
        let entry = FakeEntry {
            secret: Mutex::new(Some("  example-token\n".to_string())),
            fail_set: false,
        };

        assert_eq!(
            read_entry_secret(&entry).expect("read").as_deref(),
            Some("example-token")
        );
    }

    #[test]
    fn blank_keyring_value_reads_as_unset() {
        let entry = FakeEntry {
            secret: Mutex::new(Some("   ".to_string())),
            fail_set: false,
        };

        assert_eq!(read_entry_secret(&entry).expect("read"), None);
    }

    #[test]
    fn write_trims_before_storing() {
        let entry = FakeEntry::default();

        write_entry_secret(&entry, " example-token ").expect("write");

        assert_eq!(
            entry.secret.lock().expect("fake secret lock").as_deref(),
            Some("example-token")
        );
    }

    #[test]
    fn write_blank_clears_keyring_value() {
        let entry = FakeEntry {
            secret: Mutex::new(Some("example-token".to_string())),
            fail_set: false,
        };

        write_entry_secret(&entry, "").expect("clear");

        assert!(entry.secret.lock().expect("fake secret lock").is_none());
    }

    #[test]
    fn write_surfaces_keyring_errors() {
        let entry = FakeEntry {
            fail_set: true,
            ..FakeEntry::default()
        };

        assert!(write_entry_secret(&entry, "example-token").is_err());
    }
}
