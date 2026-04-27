//! 在工具执行前对照内置 `FunctionDef` 的 `parameters` 做 **JSON Schema** 校验
//!（`jsonschema` 自动检测草案版本），与发给上游的 `tools` 定义保持一致。
//!
//! 与 [`super::schema_check::workflow_tool_args_satisfy_required`] 的「必填键」粗检互补：
//! 此模块还校验类型、枚举、数值范围、嵌套子对象、以及 `additionalProperties` 等。

use std::collections::HashMap;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

use super::cached_params_for_tool_name;
use super::tool_specs_registry::tool_specs;

static ARG_SCHEMA_VALIDATORS: OnceLock<HashMap<&'static str, Validator>> = OnceLock::new();

fn validators_map() -> &'static HashMap<&'static str, Validator> {
    ARG_SCHEMA_VALIDATORS.get_or_init(build_validators_map)
}

fn build_validators_map() -> HashMap<&'static str, Validator> {
    let mut m = HashMap::new();
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name)
            .unwrap_or_else(|| panic!("工具 {} 缺少 parameters 缓存", spec.name));
        let v = jsonschema::validator_for(&schema).unwrap_or_else(|e| {
            panic!(
                "内置工具 {} 的 parameters 无法编译为 JSON Schema 验证器: {e}",
                spec.name
            );
        });
        m.insert(spec.name, v);
    }
    m
}

/// 将 `jsonschema` 的若干条错误合成为对用户与模型可读的**中文**短句（含 `instance path` 便于定位嵌套键）。
fn format_instance_errors(validator: &Validator, instance: &Value) -> String {
    let mut iter = validator.iter_errors(instance);
    let Some(e1) = iter.next() else {
        return "参数与 JSON Schema 不一致。".to_string();
    };
    let mut s = if e1.instance_path.as_str().is_empty() {
        e1.to_string()
    } else {
        format!("在路径 {}: {}", e1.instance_path, e1)
    };
    for (i, e) in iter.enumerate() {
        if i >= 2 {
            s.push_str(" …");
            break;
        }
        s.push('；');
        s.push_str(&e.to_string());
    }
    format!(
        "参数与工具 JSON Schema 不一致：{s}（可对照本工具 parameters 的 required/类型/enum 等修正后重试。）"
    )
}

/// 对已成功解析的 JSON 入参做 Schema 校验。仅内置工具有定义；`None` 表示无对应验证器（如未知 / MCP 名）。
pub(crate) fn validate_parsed_value_if_known(
    name: &str,
    args: &Value,
) -> Option<Result<(), String>> {
    let v = validators_map().get(name)?;
    if v.is_valid(args) {
        return Some(Ok(()));
    }
    Some(Err(format_instance_errors(v, args)))
}

/// 对 `args_json` 做「解析为 JSON +（**仅内置工具表** 中的名称）全量 Schema 校验」。
/// 非内置名返回 `None`（调用方不据此拦截）。解析错误与 [`parse_args::parse_args_json`] 一致。
pub(crate) fn validate_parsed_str_for_builtin(
    name: &str,
    args_json: &str,
) -> Option<Result<(), String>> {
    if !validators_map().contains_key(name) {
        return None;
    }
    let args = match super::parse_args::parse_args_json(args_json) {
        Ok(a) => a,
        Err(e) => {
            if name == "run_command"
                && let Some(repaired) =
                    super::parse_args::try_repair_run_command_args_json(args_json)
                && let Ok(v) = serde_json::from_str::<serde_json::Value>(&repaired)
            {
                v
            } else {
                return Some(Err(e));
            }
        }
    };
    validate_parsed_value_if_known(name, &args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_tool_rejects_wrong_type() {
        let r = validate_parsed_value_if_known("calc", &json!({ "expression": 1 })).expect("known");
        assert!(r.is_err());
        let msg = r.unwrap_err();
        assert!(msg.contains("参数与工具 JSON Schema"), "{}", msg);
    }
}
