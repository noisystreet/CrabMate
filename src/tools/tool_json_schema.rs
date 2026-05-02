//! 从带 [`schemars::JsonSchema`] 的 Rust 类型生成 OpenAI 兼容 **`parameters`** JSON Schema，
//! 与 [`super::tool_args_validate`]（`jsonschema`）共用同一份形状，避免手写 `json!` 与 `runner` 漂移。
//!
//! 新增工具时：在 [`super::tool_param_types`] 定义 `Deserialize + JsonSchema` 结构体，
//! `params_*` 调用 [`tool_parameters_schema_value`]，[`super::runners`] 用同一类型反序列化。

use schemars::{JsonSchema, SchemaGenerator};

/// 将类型的 JSON Schema 根文档序列化为 `serde_json::Value`，供 `ToolSpec::parameters` 与校验器使用。
pub(crate) fn tool_parameters_schema_value<T: JsonSchema>() -> serde_json::Value {
    let root = SchemaGenerator::default().into_root_schema_for::<T>();
    serde_json::to_value(&root).unwrap_or_else(|e| {
        panic!("内置工具 parameters JSON Schema 序列化失败（内部错误）: {e}");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::tool_param_types::CalcArgs;
    use serde_json::json;

    #[test]
    fn calc_schema_has_required_expression() {
        let v = tool_parameters_schema_value::<CalcArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required array");
        assert!(
            req.iter().any(|x| x == "expression"),
            "schema missing required expression: {v}"
        );
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "deny_unknown_fields should set additionalProperties false"
        );
    }
}
