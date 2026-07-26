//! 将 MCP `inputSchema` 清洗为更易被 OpenAI 兼容网关接受的 JSON Schema。

use serde_json::{Map, Value};

/// 规范化 MCP → OpenAI `parameters` 用的 JSON Schema（原地递归）。
///
/// 常见修复：属性 `type` 为 integer/number 时，字符串形态的 `default`/`const`/`enum` 成员尽量解析为数字，
/// 避免部分网关因 schema 校验失败而丢弃整条工具。
pub fn sanitize_mcp_json_schema(schema: &Map<String, Value>) -> Value {
    let mut root = Value::Object(schema.clone());
    sanitize_schema_value(&mut root);
    if let Value::Object(obj) = &mut root
        && !obj.contains_key("type")
    {
        obj.insert("type".to_string(), Value::String("object".to_string()));
    }
    root
}

fn sanitize_schema_value(v: &mut Value) {
    let Value::Object(obj) = v else {
        return;
    };
    coerce_numeric_literals_for_type(obj);
    if let Some(props) = obj.get_mut("properties").and_then(Value::as_object_mut) {
        for prop in props.values_mut() {
            sanitize_schema_value(prop);
        }
    }
    if let Some(items) = obj.get_mut("items") {
        sanitize_schema_value(items);
    }
    if let Some(Value::Array(arr)) = obj.get_mut("anyOf") {
        for item in arr {
            sanitize_schema_value(item);
        }
    }
    if let Some(Value::Array(arr)) = obj.get_mut("oneOf") {
        for item in arr {
            sanitize_schema_value(item);
        }
    }
    if let Some(Value::Array(arr)) = obj.get_mut("allOf") {
        for item in arr {
            sanitize_schema_value(item);
        }
    }
    if let Some(addl) = obj.get_mut("additionalProperties")
        && addl.is_object()
    {
        sanitize_schema_value(addl);
    }
}

fn primary_json_type(obj: &Map<String, Value>) -> Option<&str> {
    match obj.get("type") {
        Some(Value::String(s)) => Some(s.as_str()),
        Some(Value::Array(arr)) => arr.iter().find_map(|x| {
            x.as_str()
                .filter(|s| *s != "null" && *s != "array" && *s != "object")
        }),
        _ => None,
    }
}

fn coerce_numeric_literals_for_type(obj: &mut Map<String, Value>) {
    let Some(ty) = primary_json_type(obj) else {
        return;
    };
    let is_int = ty == "integer";
    let is_num = ty == "number" || is_int;
    if !is_num {
        return;
    }
    for key in ["default", "const"] {
        if let Some(val) = obj.get_mut(key) {
            coerce_numeric_value(val, is_int);
        }
    }
    if let Some(Value::Array(arr)) = obj.get_mut("enum") {
        for item in arr {
            coerce_numeric_value(item, is_int);
        }
    }
}

fn coerce_numeric_value(val: &mut Value, prefer_int: bool) {
    let Value::String(s) = val else {
        return;
    };
    let t = s.trim();
    if prefer_int && let Ok(n) = t.parse::<i64>() {
        *val = Value::Number(n.into());
        return;
    }
    if let Ok(f) = t.parse::<f64>()
        && let Some(n) = serde_json::Number::from_f64(f)
    {
        if prefer_int && f.fract() == 0.0 && f >= i64::MIN as f64 && f <= i64::MAX as f64 {
            *val = Value::Number((f as i64).into());
        } else {
            *val = Value::Number(n);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coerces_string_default_on_integer_prop() {
        let schema = json!({
            "type": "object",
            "properties": {
                "limit": { "type": "integer", "default": "10" }
            }
        });
        let Value::Object(map) = schema else {
            panic!("object");
        };
        let out = sanitize_mcp_json_schema(&map);
        assert_eq!(out["properties"]["limit"]["default"], json!(10));
    }

    #[test]
    fn coerces_enum_and_adds_object_type() {
        let schema = json!({
            "properties": {
                "n": { "type": "number", "enum": ["1.5", "2"] }
            }
        });
        let Value::Object(map) = schema else {
            panic!("object");
        };
        let out = sanitize_mcp_json_schema(&map);
        assert_eq!(out["type"], json!("object"));
        assert_eq!(out["properties"]["n"]["enum"][0].as_f64(), Some(1.5));
        assert_eq!(out["properties"]["n"]["enum"][1].as_f64(), Some(2.0));
    }
}
