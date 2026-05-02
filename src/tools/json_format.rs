//! JSON/YAML 格式化与转换工具（纯内存）

const MAX_INPUT_BYTES: usize = 512 * 1024;

pub fn run(args_json: &str) -> String {
    let args: super::tool_param_types::JsonFormatArgs = match serde_json::from_str(args_json) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 无效: {e}"),
    };
    let text = args.text.trim();
    if text.is_empty() {
        return "错误：缺少 text 参数".to_string();
    }
    if text.len() > MAX_INPUT_BYTES {
        return format!(
            "错误：输入超过上限（{} 字节，上限 {} 字节）",
            text.len(),
            MAX_INPUT_BYTES
        );
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

fn json_pretty(text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => serde_json::to_string_pretty(&v).unwrap_or_else(|e| format!("序列化失败：{}", e)),
        Err(e) => format!("JSON 解析失败：{}", e),
    }
}

fn json_compact(text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => serde_json::to_string(&v).unwrap_or_else(|e| format!("序列化失败：{}", e)),
        Err(e) => format!("JSON 解析失败：{}", e),
    }
}

fn yaml_to_json(text: &str) -> String {
    match serde_yaml::from_str::<serde_json::Value>(text) {
        Ok(v) => {
            serde_json::to_string_pretty(&v).unwrap_or_else(|e| format!("JSON 序列化失败：{}", e))
        }
        Err(e) => format!("YAML 解析失败：{}", e),
    }
}

fn json_to_yaml(text: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(text) {
        Ok(v) => serde_yaml::to_string(&v).unwrap_or_else(|e| format!("YAML 序列化失败：{}", e)),
        Err(e) => format!("JSON 解析失败：{}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pretty_format() {
        let out = run(r#"{"text":"{\"a\":1,\"b\":2}","mode":"pretty"}"#);
        assert!(out.contains("\"a\": 1"), "out={}", out);
        assert!(out.contains('\n'));
    }

    #[test]
    fn compact_format() {
        let out = run(r#"{"text":"{ \"a\" : 1 , \"b\" : 2 }","mode":"compact"}"#);
        assert_eq!(out, r#"{"a":1,"b":2}"#);
    }

    #[test]
    fn yaml_to_json_conversion() {
        let out = run(r#"{"text":"name: hello\nvalue: 42","mode":"yaml_to_json"}"#);
        assert!(out.contains("\"name\""), "out={}", out);
        assert!(out.contains("\"hello\""), "out={}", out);
    }

    #[test]
    fn json_to_yaml_conversion() {
        let out = run(r#"{"text":"{\"name\":\"hello\"}","mode":"json_to_yaml"}"#);
        assert!(out.contains("name:"), "out={}", out);
    }

    #[test]
    fn invalid_json() {
        let out = run(r#"{"text":"not json","mode":"pretty"}"#);
        assert!(out.contains("JSON 解析失败"));
    }

    #[test]
    fn empty_text() {
        let out = run(r#"{"text":""}"#);
        assert!(out.contains("缺少 text"));
    }
}
