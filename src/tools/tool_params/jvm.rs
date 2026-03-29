//! JVM 工具 JSON Schema。

pub(in crate::tools) fn params_maven_compile() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "profile": { "type": "string", "description": "可选：Maven -P profile，仅字母数字与 _-.，无空白" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_maven_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "profile": { "type": "string", "description": "可选：Maven -P profile" },
            "test": { "type": "string", "description": "可选：-Dtest= 过滤（类名/方法片段，保守字符集）" }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_gradle_compile() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "tasks": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：Gradle 任务名列表，默认 [\"classes\"]；仅允许字母数字与 _:.-"
            }
        },
        "required": []
    })
}

pub(in crate::tools) fn params_gradle_test() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "tasks": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：Gradle 任务名列表，默认 [\"test\"]"
            }
        },
        "required": []
    })
}
