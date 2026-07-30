//! 「已保存模型」API 密钥的钥匙串存储；JSON 仅保留脱敏状态。

use std::collections::HashSet;
use std::fmt::Write;

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::credential_store::{read_named_secret, write_named_secret};

const ACCOUNT_PREFIX: &str = "saved_model_";

fn model_identity(value: &Value) -> Option<(&str, &str)> {
    let object = value.as_object()?;
    let api_base = object.get("api_base")?.as_str()?.trim();
    let model = object.get("model")?.as_str()?.trim();
    if api_base.is_empty() || model.is_empty() {
        None
    } else {
        Some((api_base, model))
    }
}

fn model_account(api_base: &str, model: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_base.trim().as_bytes());
    hasher.update([0]);
    hasher.update(model.trim().as_bytes());
    let digest = hasher.finalize();
    let mut account = String::with_capacity(ACCOUNT_PREFIX.len() + digest.len() * 2);
    account.push_str(ACCOUNT_PREFIX);
    for byte in digest {
        let _ = write!(account, "{byte:02x}");
    }
    account
}

fn account_for_value(value: &Value) -> Option<String> {
    model_identity(value).map(|(api_base, model)| model_account(api_base, model))
}

pub(super) fn prepare_saved_model_secrets(values: &mut [Value]) -> Result<(), String> {
    for value in values {
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        let api_key = object.remove("api_key");
        let identity = object
            .get("api_base")
            .and_then(Value::as_str)
            .zip(object.get("model").and_then(Value::as_str))
            .map(|(api_base, model)| (api_base.trim(), model.trim()))
            .filter(|(api_base, model)| !api_base.is_empty() && !model.is_empty());

        let account = identity.map(|(api_base, model)| model_account(api_base, model));
        if let Some(secret) = api_key.as_ref().and_then(Value::as_str) {
            let secret = secret.trim();
            if !secret.is_empty() {
                let account = account.as_deref().ok_or_else(|| {
                    "已保存模型含 API 密钥，但缺少 api_base 或 model，无法写入系统钥匙串"
                        .to_string()
                })?;
                write_named_secret(account, secret)?;
            }
        }

        let is_set = account
            .as_deref()
            .and_then(read_named_secret)
            .is_some_and(|secret| !secret.trim().is_empty());
        object.insert("has_api_key".to_string(), Value::Bool(is_set));
    }
    Ok(())
}

pub(super) fn scrub_saved_model_api_keys(values: &mut [Value]) {
    for value in values {
        if let Some(object) = value.as_object_mut() {
            object.remove("api_key");
        }
    }
}

pub(super) fn saved_model_accounts(values: &[Value]) -> HashSet<String> {
    values.iter().filter_map(account_for_value).collect()
}

pub(super) fn delete_removed_saved_model_secrets(
    old_accounts: &HashSet<String>,
    new_accounts: &HashSet<String>,
) -> Result<(), String> {
    for account in old_accounts.difference(new_accounts) {
        write_named_secret(account, "")?;
    }
    Ok(())
}

pub fn read_saved_model_secret(
    values: &[Value],
    api_base: Option<&str>,
    model: Option<&str>,
) -> Option<String> {
    let api_base = api_base?.trim();
    let model = model?.trim();
    let matching = values.iter().any(|value| {
        model_identity(value)
            .is_some_and(|(base, candidate)| base == api_base && candidate == model)
    });
    matching
        .then(|| model_account(api_base, model))
        .and_then(|account| read_named_secret(&account))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_is_stable_and_separates_model_identity() {
        assert_eq!(
            model_account(" https://example.com/v1 ", " model-a "),
            model_account("https://example.com/v1", "model-a")
        );
        assert_ne!(
            model_account("https://example.com/v1", "model-a"),
            model_account("https://example.com/v1", "model-b")
        );
    }

    #[test]
    fn account_collection_ignores_incomplete_entries() {
        let values = vec![
            serde_json::json!({"api_base": "https://example.com/v1", "model": "model-a"}),
            serde_json::json!({"api_base": "", "model": "model-b"}),
        ];
        assert_eq!(saved_model_accounts(&values).len(), 1);
    }

    #[test]
    fn prepare_moves_api_key_out_of_json_and_keeps_status_only() {
        let mut values = vec![serde_json::json!({
            "api_base": "https://keyring-test.invalid/v1",
            "model": "model-migration-test",
            "api_key": "example-token"
        })];

        prepare_saved_model_secrets(&mut values).expect("prepare");

        let object = values[0].as_object().expect("saved model object");
        assert!(!object.contains_key("api_key"));
        assert_eq!(object.get("has_api_key"), Some(&Value::Bool(true)));
        assert_eq!(
            read_saved_model_secret(
                &values,
                Some("https://keyring-test.invalid/v1"),
                Some("model-migration-test")
            )
            .as_deref(),
            Some("example-token")
        );
    }
}
