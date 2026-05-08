//! 工作流节点 `tool_args` 的校验：与 **内置** `run_tool` 前一致，对照
//! [`FunctionDef::parameters`][`crate::types::FunctionDef`] 的 **JSON Schema** 全量验证
//!（`jsonschema`：类型、`required`、`enum`、`additionalProperties` 等），与 [tool_args_validate](`super::tool_args_validate`) 同路径。

use serde_json::Value;

use super::tool_args_validate::validate_parsed_value_if_known;

/// 返回 `Err` 当 `tool_name` 非内置工具，或 `tool_args` 不符合该工具的 **JSON Schema**。
pub fn workflow_tool_args_satisfy_required(
    tool_name: &str,
    tool_args: &Value,
) -> Result<(), String> {
    if super::cached_params_for_tool_name(tool_name).is_none() {
        return Err(format!(
            "未知工具 tool_name={}（非当前内置工具注册表中的名称）",
            tool_name
        ));
    }
    let mut args = tool_args.clone();
    if tool_name == "read_file" {
        super::tool_args_validate::coerce_read_file_tool_args_value(&mut args);
    }
    if let Some(r) = validate_parsed_value_if_known(tool_name, &args) {
        return r;
    }
    // `cached_params_for_tool_name` 有值时验证器应已随 `tool_specs` 全量预编译；否则为内部不一致。
    Err("内部错误：已注册工具缺少 JSON Schema 验证器".to_string())
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

    #[test]
    fn read_file_workflow_accepts_string_line_numbers_after_coercion() {
        let r = workflow_tool_args_satisfy_required(
            "read_file",
            &serde_json::json!({"path": "README.md", "start_line": "10"}),
        );
        assert!(r.is_ok(), "{r:?}");
    }
}
