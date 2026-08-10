//! 「已保存模型」条目：服务端 **不再** 存模型 API Key（权威在 Client）。
//! JSON 仅保留端点元数据；若仍含明文 `api_key` 则剥离，并固定 `has_api_key=false`。

use serde_json::Value;

/// 剥离明文 `api_key`，并标记服务端不持有该密钥。
pub(super) fn scrub_saved_model_api_keys(values: &mut [Value]) {
    for value in values {
        let Some(object) = value.as_object_mut() else {
            continue;
        };
        object.remove("api_key");
        object.insert("has_api_key".to_string(), Value::Bool(false));
    }
}

/// 加载 / 保存前规范化：与 [`scrub_saved_model_api_keys`] 相同（兼容旧调用名）。
pub(super) fn prepare_saved_model_secrets(values: &mut [Value]) -> Result<(), String> {
    scrub_saved_model_api_keys(values);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scrub_strips_api_key_and_clears_flag() {
        let mut values = vec![json!({
            "api_base": "https://example.invalid/v1",
            "model": "m1",
            "api_key": "example-token",
            "has_api_key": true
        })];
        prepare_saved_model_secrets(&mut values).expect("prepare");
        let object = values[0].as_object().expect("object");
        assert!(!object.contains_key("api_key"));
        assert_eq!(object.get("has_api_key"), Some(&Value::Bool(false)));
    }
}
