//! 持久密钥的系统钥匙串适配与旧明文文件迁移。

use std::path::Path;
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

fn remove_legacy_file(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除旧密钥文件 {} 失败: {error}", path.display())),
    }
}

fn read_or_migrate(entry: &dyn SecretEntry, legacy_path: &Path) -> Result<Option<String>, String> {
    if let Some(secret) = entry.get_password()? {
        let secret = secret.trim();
        if !secret.is_empty() {
            remove_legacy_file(legacy_path)?;
            return Ok(Some(secret.to_string()));
        }
    }

    let Some(secret) = super::io::read_secret_line(legacy_path) else {
        remove_legacy_file(legacy_path)?;
        return Ok(None);
    };
    entry.set_password(&secret)?;
    remove_legacy_file(legacy_path)?;
    Ok(Some(secret))
}

fn write_secret(entry: &dyn SecretEntry, legacy_path: &Path, secret: &str) -> Result<(), String> {
    let secret = secret.trim();
    if secret.is_empty() {
        entry.delete_credential()?;
    } else {
        entry.set_password(secret)?;
    }
    remove_legacy_file(legacy_path)
}

#[cfg(not(test))]
pub(super) fn read_migrating_secret(account: &str, legacy_path: &Path) -> Option<String> {
    let result =
        SystemSecretEntry::new(account).and_then(|entry| read_or_migrate(&entry, legacy_path));
    match result {
        Ok(secret) => secret,
        Err(error) => {
            tracing::warn!(target: "crabmate", account, error = %error, "读取系统钥匙串失败");
            None
        }
    }
}

#[cfg(test)]
struct TestNamedSecretEntry<'a> {
    account: &'a str,
}

#[cfg(test)]
impl SecretEntry for TestNamedSecretEntry<'_> {
    fn get_password(&self) -> Result<Option<String>, String> {
        Ok(read_named_secret(self.account))
    }

    fn set_password(&self, password: &str) -> Result<(), String> {
        write_named_secret(self.account, password)
    }

    fn delete_credential(&self) -> Result<(), String> {
        write_named_secret(self.account, "")
    }
}

#[cfg(test)]
pub(super) fn read_migrating_secret(account: &str, legacy_path: &Path) -> Option<String> {
    read_or_migrate(&TestNamedSecretEntry { account }, legacy_path)
        .expect("test migrating secret read")
}

#[cfg(not(test))]
pub(super) fn read_named_secret(account: &str) -> Option<String> {
    let result = SystemSecretEntry::new(account).and_then(|entry| entry.get_password());
    match result {
        Ok(secret) => secret.filter(|value| !value.trim().is_empty()),
        Err(error) => {
            tracing::warn!(target: "crabmate", account, error = %error, "读取系统钥匙串失败");
            None
        }
    }
}

#[cfg(test)]
fn test_named_secrets() -> &'static Mutex<std::collections::HashMap<String, String>> {
    static SECRETS: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> =
        std::sync::OnceLock::new();
    SECRETS.get_or_init(|| Mutex::new(std::collections::HashMap::new()))
}

#[cfg(test)]
pub(super) fn read_named_secret(account: &str) -> Option<String> {
    test_named_secrets()
        .lock()
        .expect("test named secrets lock")
        .get(account)
        .cloned()
}

#[cfg(not(test))]
pub(super) fn write_migrating_secret(
    account: &str,
    legacy_path: &Path,
    secret: &str,
) -> Result<(), String> {
    let entry = SystemSecretEntry::new(account)?;
    write_secret(&entry, legacy_path, secret)
}

#[cfg(test)]
pub(super) fn write_migrating_secret(
    account: &str,
    legacy_path: &Path,
    secret: &str,
) -> Result<(), String> {
    write_secret(&TestNamedSecretEntry { account }, legacy_path, secret)
}

#[cfg(not(test))]
pub(super) fn write_named_secret(account: &str, secret: &str) -> Result<(), String> {
    let entry = SystemSecretEntry::new(account)?;
    let secret = secret.trim();
    if secret.is_empty() {
        entry.delete_credential()
    } else {
        entry.set_password(secret)
    }
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
    use std::sync::Mutex;

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
    fn migrates_legacy_file_only_after_keyring_write_succeeds() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("client_llm");
        std::fs::write(&legacy, "example-token").expect("write legacy");
        let entry = FakeEntry::default();

        let loaded = read_or_migrate(&entry, &legacy).expect("migrate");

        assert_eq!(loaded.as_deref(), Some("example-token"));
        assert_eq!(
            entry.secret.lock().expect("fake secret lock").as_deref(),
            Some("example-token")
        );
        assert!(!legacy.exists());
    }

    #[test]
    fn failed_keyring_write_keeps_legacy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("client_llm");
        std::fs::write(&legacy, "example-token").expect("write legacy");
        let entry = FakeEntry {
            fail_set: true,
            ..FakeEntry::default()
        };

        assert!(read_or_migrate(&entry, &legacy).is_err());
        assert!(legacy.exists());
    }

    #[test]
    fn existing_keyring_value_wins_and_removes_legacy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("client_llm");
        std::fs::write(&legacy, "legacy-example").expect("write legacy");
        let entry = FakeEntry {
            secret: Mutex::new(Some("keyring-example".to_string())),
            fail_set: false,
        };

        let loaded = read_or_migrate(&entry, &legacy).expect("read");

        assert_eq!(loaded.as_deref(), Some("keyring-example"));
        assert!(!legacy.exists());
    }

    #[test]
    fn clearing_removes_keyring_value_and_legacy_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let legacy = dir.path().join("client_llm");
        std::fs::write(&legacy, "legacy-example").expect("write legacy");
        let entry = FakeEntry {
            secret: Mutex::new(Some("keyring-example".to_string())),
            fail_set: false,
        };

        write_secret(&entry, &legacy, "").expect("clear");

        assert!(entry.secret.lock().expect("fake secret lock").is_none());
        assert!(!legacy.exists());
    }
}
