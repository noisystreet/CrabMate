//! 工作流节点 `tool_args` 的**粗粒度**校验：对照内置工具的 JSON Schema 检查 `required` 键是否出现在 `tool_args` 中（含嵌套对象/数组内）。
//!
//! 完整 JSON Schema 校验未引入第三方库；此处仅降低「缺必填字段仍进入 DAG」的概率，详细类型校验仍由各 `runner_*` 负责。

use serde_json::Value;

/// 返回 `Err` 当 `tool_name` 非内置工具，或 `tool_args` 缺少 schema 声明的必填键（任意深度）。
pub fn workflow_tool_args_satisfy_required(
    tool_name: &str,
    tool_args: &Value,
) -> Result<(), String> {
    let schema = super::cached_params_for_tool_name(tool_name).ok_or_else(|| {
        format!(
            "未知工具 tool_name={}（非当前内置工具注册表中的名称）",
            tool_name
        )
    })?;
    required_keys_present_in_value(&schema, tool_args)
}

fn required_keys_present_in_value(schema: &Value, data: &Value) -> Result<(), String> {
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return Ok(());
    };
    let required = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();

    let data_obj = match data {
        Value::Object(m) => m,
        // 模型偶发传入非对象：视为缺失全部 required
        _ => {
            if required.is_empty() {
                return Ok(());
            }
            let first = required.first().copied().unwrap_or("?");
            return Err(format!(
                "tool_args 应为 JSON 对象，但当前不是；无法提供必填字段 `{first}`"
            ));
        }
    };

    for key in required {
        if !object_has_key_at_any_depth(data_obj, key) {
            return Err(format!(
                "缺少必填参数 `{key}`（对照工具 JSON Schema 的 required 列表）"
            ));
        }
        let sub_schema = props.get(key);
        let sub_data = data_obj.get(key);
        if let (Some(s), Some(d)) = (sub_schema, sub_data) {
            validate_nested_required(s, d)?;
        }
    }
    Ok(())
}

fn validate_nested_required(schema: &Value, data: &Value) -> Result<(), String> {
    match data {
        Value::Array(arr) => {
            for item in arr {
                validate_nested_required(schema, item)?;
            }
            Ok(())
        }
        Value::Object(_) => required_keys_present_in_value(schema, data),
        _ => Ok(()),
    }
}

fn object_has_key_at_any_depth(obj: &serde_json::Map<String, Value>, key: &str) -> bool {
    if obj.contains_key(key) {
        return true;
    }
    obj.values().any(|v| value_contains_key(v, key))
}

fn value_contains_key(v: &Value, key: &str) -> bool {
    match v {
        Value::Object(m) => object_has_key_at_any_depth(m, key),
        Value::Array(arr) => arr.iter().any(|x| value_contains_key(x, key)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calc_requires_expression() {
        let r = workflow_tool_args_satisfy_required("calc", &serde_json::json!({}));
        assert!(r.is_err());
        let r =
            workflow_tool_args_satisfy_required("calc", &serde_json::json!({"expression": "1+1"}));
        assert!(r.is_ok());
    }

    #[test]
    fn unknown_tool_errors() {
        let r = workflow_tool_args_satisfy_required("not_a_real_tool_xyz", &serde_json::json!({}));
        assert!(r.is_err());
    }
}
