//! JSON/YAML 格式化与转换工具（纯内存）

use crate::cm_tools::tool_result::ToolError;

const MAX_INPUT_BYTES: usize = 512 * 1024;

/// 显式失败版本（成功正文与迁移前的 String 面逐字节一致）。
///
/// 本族无内部 String 消费者，故不保留 `run` 门面；测试用本地 `run_str` 还原旧形态。
#[allow(clippy::result_large_err)]
pub fn run_try(args_json: &str) -> Result<String, ToolError> {
    let args: super::tool_param_types::JsonFormatArgs =
        super::parse_args_typed(args_json).map_err(ToolError::invalid_args)?;
    let text = args.text.trim();
    if text.is_empty() {
        return Err(ToolError::invalid_args("错误：缺少 text 参数".to_string()));
    }
    if text.len() > MAX_INPUT_BYTES {
        return Err(ToolError::invalid_args(format!(
            "错误：输入超过上限（{} 字节，上限 {} 字节）",
            text.len(),
            MAX_INPUT_BYTES
        )));
    }
    match args
        .mode
        .unwrap_or(super::tool_param_types::JsonFormatMode::Pretty)
    {
        super::tool_param_types::JsonFormatMode::Pretty => json_pretty(text),
        super::tool_param_types::JsonFormatMode::Compact => json_compact(text),
        super::tool_param_types::JsonFormatMode::YamlToJson => yaml_to_json(text),
        super::tool_param_types::JsonFormatMode::JsonToYaml => json_to_yaml(text),
    }
}

/// 序列化失败按内部错误处理：`serde_json::Value` 已解析成功，写出失败属逻辑不变量破坏。
#[allow(clippy::result_large_err)]
fn json_pretty(text: &str) -> Result<String, ToolError> {
    let v: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ToolError::invalid_args(format!("JSON 解析失败：{e}")))?;
    serde_json::to_string_pretty(&v).map_err(|e| {
        ToolError::internal_code("json_format_serialize_failed", format!("序列化失败：{e}"))
    })
}

#[allow(clippy::result_large_err)]
fn json_compact(text: &str) -> Result<String, ToolError> {
    let v: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ToolError::invalid_args(format!("JSON 解析失败：{e}")))?;
    serde_json::to_string(&v).map_err(|e| {
        ToolError::internal_code("json_format_serialize_failed", format!("序列化失败：{e}"))
    })
}

#[allow(clippy::result_large_err)]
fn yaml_to_json(text: &str) -> Result<String, ToolError> {
    let v: serde_json::Value = serde_yaml::from_str(text)
        .map_err(|e| ToolError::invalid_args(format!("YAML 解析失败：{e}")))?;
    serde_json::to_string_pretty(&v).map_err(|e| {
        ToolError::internal_code(
            "json_format_serialize_failed",
            format!("JSON 序列化失败：{e}"),
        )
    })
}

#[allow(clippy::result_large_err)]
fn json_to_yaml(text: &str) -> Result<String, ToolError> {
    let v: serde_json::Value = serde_json::from_str(text)
        .map_err(|e| ToolError::invalid_args(format!("JSON 解析失败：{e}")))?;
    serde_yaml::to_string(&v).map_err(|e| {
        ToolError::internal_code(
            "json_format_serialize_failed",
            format!("YAML 序列化失败：{e}"),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 迁移前 String 面的等价物（`ToolError` → `message`），供断言旧文案用。
    fn run_str(args_json: &str) -> String {
        run_try(args_json).unwrap_or_else(|e| e.message)
    }

    #[test]
    fn pretty_format() {
        let out = run_str(r#"{"text":"{\"a\":1,\"b\":2}","mode":"pretty"}"#);
        assert!(out.contains("\"a\": 1"), "out={}", out);
        assert!(out.contains('\n'));
    }

    #[test]
    fn compact_format() {
        let out = run_str(r#"{"text":"{ \"a\" : 1 , \"b\" : 2 }","mode":"compact"}"#);
        assert_eq!(out, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn yaml_to_json_conversion() {
        let out = run_str(r#"{"text":"name: hello\nvalue: 42","mode":"yaml_to_json"}"#);
        assert!(out.contains("\"name\""), "out={}", out);
        assert!(out.contains("\"hello\""), "out={}", out);
    }

    #[test]
    fn json_to_yaml_conversion() {
        let out = run_str(r#"{"text":"{\"name\":\"hello\"}","mode":"json_to_yaml"}"#);
        assert!(out.contains("name:"), "out={}", out);
    }

    #[test]
    fn invalid_json() {
        let out = run_str(r#"{"text":"not json","mode":"pretty"}"#);
        assert!(out.contains("JSON 解析失败"));
    }

    #[test]
    fn empty_text() {
        let out = run_str(r#"{"text":""}"#);
        assert!(out.contains("缺少 text"));
    }
}
