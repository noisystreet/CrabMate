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
    use crate::tools::tool_param_types::{
        CalcArgs, GolangciLintArgs, MarkdownCheckLinksArgs, PortCheckArgs, ProcessListArgs,
    };
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

    #[test]
    fn port_check_schema_requires_port_in_range() {
        let v = tool_parameters_schema_value::<PortCheckArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required array");
        assert!(req.iter().any(|x| x == "port"));
        let port = v.pointer("/properties/port").expect("port schema");
        assert_eq!(port.get("minimum"), Some(&json!(1.0)));
        assert_eq!(port.get("maximum"), Some(&json!(65535.0)));
    }

    #[test]
    fn process_list_schema_allows_optional_filter() {
        let v = tool_parameters_schema_value::<ProcessListArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        assert!(v.pointer("/properties/filter").is_some());
        assert!(v.pointer("/properties/user_only").is_some());
        assert!(v.pointer("/properties/max_count").is_some());
    }

    #[test]
    fn golangci_lint_schema_has_fix_fast() {
        let v = tool_parameters_schema_value::<GolangciLintArgs>();
        assert!(v.pointer("/properties/fix").is_some());
        assert!(v.pointer("/properties/fast").is_some());
    }

    #[test]
    fn markdown_check_links_schema_has_output_format_enum() {
        let v = tool_parameters_schema_value::<MarkdownCheckLinksArgs>();
        let defs = v.get("definitions").expect("schema definitions");
        let def = defs
            .get("MarkdownCheckLinksOutputFormat")
            .expect("MarkdownCheckLinksOutputFormat def");
        let fmt = def
            .get("enum")
            .and_then(|x| x.as_array())
            .expect("enum array");
        assert!(fmt.iter().any(|x| x == "text"));
        assert!(fmt.iter().any(|x| x == "json"));
        assert!(fmt.iter().any(|x| x == "sarif"));
    }
}
