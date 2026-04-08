//! 工具调用入参 JSON（`args_json`）的公共解析，统一错误文案。

/// 将工具入参 JSON 解析为 [`serde_json::Value`]；失败时返回与用户可见 runner 一致的中文说明。
#[inline]
pub(crate) fn parse_args_json(args_json: &str) -> Result<serde_json::Value, String> {
    serde_json::from_str(args_json).map_err(|e| format!("参数 JSON 无效: {e}"))
}

#[cfg(test)]
mod tests {
    use super::parse_args_json;

    #[test]
    fn ok_object() {
        let v = parse_args_json(r#"{"a":1}"#).expect("valid");
        assert_eq!(v["a"], 1);
    }

    #[test]
    fn err_prefix() {
        let e = parse_args_json("{").unwrap_err();
        assert!(e.starts_with("参数 JSON 无效:"), "got {e:?}");
    }
}
