//! 从带 [`schemars::JsonSchema`] 的 Rust 类型生成 OpenAI 兼容 **`parameters`** JSON Schema，
//! 与 [`super::tool_args_validate`]（`jsonschema`）共用同一份形状，避免手写 `json!` 与 `runner` 漂移。
//!
//! 新增工具时：在 [`super::tool_param_types`] 定义 `Deserialize + JsonSchema` 结构体，
//! `params_*` 调用 [`tool_parameters_schema_value`]，[`super::runners`] 用同一类型反序列化。

use schemars::{JsonSchema, SchemaGenerator};

/// 将类型的 JSON Schema 根文档序列化为 `serde_json::Value`，供 `ToolSpec::parameters` 与校验器使用。
pub fn tool_parameters_schema_value<T: JsonSchema>() -> serde_json::Value {
    let root = SchemaGenerator::default().into_root_schema_for::<T>();
    serde_json::to_value(&root).unwrap_or_else(|e| {
        panic!("内置工具 parameters JSON Schema 序列化失败（内部错误）: {e}");
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cm_tools::tools::tool_param_types::{
        AddReminderArgs, JsonFormatArgs,
        CargoTestArgs, CodeNavArgs, CodeStatsArgs,
        CoverageReportArgs, DependencyGraphArgs, DocsHealthSweepArgs, GitAction, GitArgs,
        ListRemindersArgs, MarkdownCheckLinksArgs, ModifyFileArgs,
        PytestRunArgs, RunCommandArgs,
        StructuredArgs, TableTextArgs, WorkflowExecuteArgs,
    };
    #[cfg(feature = "archive-tools")]
    use crate::cm_tools::tools::tool_param_types::ArchivePackArgs;
    use serde_json::json;

    #[test]
    fn json_format_schema_has_required_text_and_closed_object() {
        let v = tool_parameters_schema_value::<JsonFormatArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required array");
        assert!(
            req.iter().any(|x| x == "text"),
            "schema missing required text: {v}"
        );
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "deny_unknown_fields should set additionalProperties false"
        );
    }

    #[test]
    fn markdown_check_links_schema_has_output_format_enum() {
        let v = tool_parameters_schema_value::<MarkdownCheckLinksArgs>();
        let defs = v.get("$defs").expect("schema $defs");
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

    #[test]
    fn code_stats_schema_has_format_enum() {
        let v = tool_parameters_schema_value::<CodeStatsArgs>();
        let defs = v.get("$defs").expect("$defs");
        let def = defs.get("CodeStatsFormat").expect("CodeStatsFormat");
        let e = def.get("enum").and_then(|x| x.as_array()).expect("enum");
        assert!(e.iter().any(|x| x == "table"));
        assert!(e.iter().any(|x| x == "json"));
    }

    #[test]
    fn dependency_graph_schema_has_depth_range() {
        let v = tool_parameters_schema_value::<DependencyGraphArgs>();
        let depth = v.pointer("/properties/depth").expect("depth");
        let dumped = depth.to_string();
        assert!(
            dumped.contains("\"minimum\"") && dumped.contains("\"maximum\""),
            "expected range on depth: {dumped}"
        );
    }

    #[test]
    fn coverage_report_schema_has_format_variants() {
        let v = tool_parameters_schema_value::<CoverageReportArgs>();
        let defs = v.get("$defs").expect("$defs");
        let def = defs
            .get("CoverageReportFormat")
            .expect("CoverageReportFormat");
        let e = def.get("enum").and_then(|x| x.as_array()).expect("enum");
        assert!(e.iter().any(|x| x == "auto"));
        assert!(e.iter().any(|x| x == "lcov"));
        assert!(e.iter().any(|x| x == "tarpaulin_json"));
    }

    #[test]
    fn add_reminder_schema_requires_title() {
        let v = tool_parameters_schema_value::<AddReminderArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "title"));
    }

    #[test]
    fn list_reminders_schema_has_include_done() {
        let v = tool_parameters_schema_value::<ListRemindersArgs>();
        assert!(v.pointer("/properties/include_done").is_some());
    }

    #[cfg(feature = "archive-tools")]
    #[test]
    fn archive_pack_schema_requires_output_sources() {
        let v = tool_parameters_schema_value::<ArchivePackArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "output"));
        assert!(req.iter().any(|x| x == "sources"));
    }

    #[test]
    fn code_nav_schema_requires_action_and_denies_unknown() {
        let v = tool_parameters_schema_value::<CodeNavArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        // union 结构体只有 `action` 是硬必填；per-action 必填（`symbol` / `path`）
        // 由 `code_nav_dispatch` 校验，schema 表达不了这种条件约束。
        assert_eq!(req.len(), 1, "{v}");
        assert!(req.iter().any(|x| x == "action"), "{v}");
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn structured_schema_requires_action_and_exposes_the_union() {
        let v = tool_parameters_schema_value::<StructuredArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "action"), "{v}");
        let props = v
            .pointer("/properties")
            .and_then(|p| p.as_object())
            .expect("properties");
        for key in [
            "action",
            "path",
            "path_a",
            "path_b",
            "query",
            "mutation",
            "value",
            "format",
            "dry_run",
            "base_revision",
        ] {
            assert!(props.contains_key(key), "缺少 {key}: {v}");
        }
        assert_eq!(v.pointer("/additionalProperties"), Some(&json!(false)));
    }

    #[test]
    fn workflow_execute_schema_exposes_workflow_or_file() {
        let v = tool_parameters_schema_value::<WorkflowExecuteArgs>();
        let props = v
            .pointer("/properties")
            .and_then(|p| p.as_object())
            .expect("properties");
        assert!(props.contains_key("workflow"));
        assert!(props.contains_key("workflow_file"));
        let has_required_workflow_key = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .is_some_and(|req| req.iter().any(|x| x == "workflow" || x == "workflow_file"));
        assert!(
            !has_required_workflow_key,
            "runtime resolves workflow vs workflow_file; schema keeps both optional: {v}"
        );
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn docs_health_sweep_schema_denies_unknown() {
        let v = tool_parameters_schema_value::<DocsHealthSweepArgs>();
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn table_text_schema_requires_action() {
        let v = tool_parameters_schema_value::<TableTextArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "action"));
    }

    #[test]
    fn run_command_schema_makes_command_optional_and_exposes_cwd_script() {
        let v = tool_parameters_schema_value::<RunCommandArgs>();
        if let Some(req) = v.pointer("/required").and_then(|r| r.as_array()) {
            assert!(!req.iter().any(|x| x == "command"));
        }
        assert!(v.pointer("/properties/command").is_some());
        assert!(v.pointer("/properties/cwd").is_some());
        assert!(v.pointer("/properties/script").is_some());
    }

    #[test]
    fn run_command_schema_exposes_async_and_timeout_secs() {
        let v = tool_parameters_schema_value::<RunCommandArgs>();
        let async_prop = v
            .pointer("/properties/async")
            .expect("properties.async");
        let async_types = async_prop.get("type").and_then(|x| x.as_array());
        assert!(
            async_types.is_some_and(|t| t.iter().any(|x| x == "boolean")),
            "async 字段（serde rename）应为 boolean（Option → 含 null）：{async_prop}"
        );
        let timeout = v
            .pointer("/properties/timeout_secs")
            .expect("properties.timeout_secs");
        let timeout_types = timeout.get("type").and_then(|x| x.as_array());
        assert!(
            timeout_types.is_some_and(|t| t.iter().any(|x| x == "integer")),
            "timeout_secs 应为 integer（Option → 含 null）：{timeout}"
        );
        let range = timeout.get("minimum").and_then(|x| x.as_u64());
        assert_eq!(range, Some(1));
        let max = timeout.get("maximum").and_then(|x| x.as_u64());
        assert_eq!(max, Some(600));
    }

    /// 后台任务（async）对 `cargo_test` / `pytest_run` 生效，其 `parameters` 须暴露 `async` 布尔开关。
    #[test]
    fn cargo_test_and_pytest_run_schemas_expose_async() {
        for (name, v) in [
            (
                "cargo_test",
                tool_parameters_schema_value::<CargoTestArgs>(),
            ),
            ("pytest_run", tool_parameters_schema_value::<PytestRunArgs>()),
        ] {
            let prop = v.pointer("/properties/async").expect("properties.async");
            let types = prop.get("type").and_then(|x| x.as_array());
            assert!(
                types.is_some_and(|t| t.iter().any(|x| x == "boolean")),
                "{name}.async 字段（serde rename）应为 boolean（Option → 含 null）：{prop}"
            );
        }
    }

    /// 递归收集 schema 中的枚举字面量：schemars 对枚举可能内联成 `enum: [...]`，
    /// 也可能拆成 `oneOf` + `const`，还可能经 `$ref` / `$defs` 呈现——三种都要能取到。
    fn collect_enum_values<'a>(v: &'a serde_json::Value, out: &mut Vec<&'a str>) {
        match v {
            serde_json::Value::Object(map) => {
                if let Some(serde_json::Value::Array(items)) = map.get("enum") {
                    out.extend(items.iter().filter_map(|x| x.as_str()));
                }
                if let Some(serde_json::Value::String(konst)) = map.get("const") {
                    out.push(konst);
                }
                for child in map.values() {
                    collect_enum_values(child, out);
                }
            }
            serde_json::Value::Array(items) => {
                for child in items {
                    collect_enum_values(child, out);
                }
            }
            _ => {}
        }
    }

    /// `GitAction` 同时是 schema enum 与 `git::read_ops::dispatch` 的分发真源：
    /// dispatch 已改为对它 `match`（新增变体不穷尽即编译失败），这里钉住 schema 侧的字符串，
    /// 避免 `rename_all` 或变体名被改后，`dispatch` 反序列化与文档示例悄悄错位。
    #[test]
    fn git_schema_action_enum_matches_documented_names() {
        let v = tool_parameters_schema_value::<GitAction>();
        let mut values = Vec::new();
        collect_enum_values(&v, &mut values);
        for expected in [
            "status",
            "diff",
            "clean_check",
            "log",
            "show",
            "blame",
            "file_history",
            "branch_list",
            "remote_status",
            "remote_list",
        ] {
            assert!(values.contains(&expected), "schema enum 缺少 {expected}：{values:?}");
        }
        // 数量对齐：新增 action 时这里会失败，提醒同步 `GIT_ACTIONS` 文案与文档示例。
        assert_eq!(values.len(), 10, "action 数量变化：{values:?}");
    }

    #[test]
    fn git_schema_requires_action_and_keeps_union_fields() {
        let v = tool_parameters_schema_value::<GitArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        // `action` 必填（合并前是 10 个无此参数的工具）。
        assert!(v.pointer("/required").is_some(), "action 应为 required");
        assert!(v.pointer("/properties/action").is_some());
        // 各 action 的字段全部在并集里可见，且都是可选。
        for field in [
            "porcelain",
            "branch",
            "mode",
            "path",
            "stat",
            "name_only",
            "base",
            "max_count",
            "rev",
            "start_line",
            "end_line",
            "include_remote",
        ] {
            assert!(
                v.pointer(&format!("/properties/{field}")).is_some(),
                "{field} 应出现在 GitArgs schema 中"
            );
        }
    }

    #[test]
    fn modify_file_mode_schema_includes_expected_modes() {
        let v = tool_parameters_schema_value::<ModifyFileArgs>();
        assert!(
            v.pointer("/properties/mode").is_some(),
            "schema should expose properties.mode"
        );
        let mode_def = v
            .pointer("/$defs/ModifyFileMode")
            .expect("$defs.ModifyFileMode");
        let mut flat: Vec<&str> = Vec::new();
        if let Some(en) = mode_def.get("enum").and_then(|e| e.as_array()) {
            flat.extend(en.iter().filter_map(|x| x.as_str()));
        }
        // schemars 1.x emits `const` for documented variants and `enum` for undocumented ones.
        if let Some(branches) = mode_def.get("oneOf").and_then(|o| o.as_array()) {
            for branch in branches {
                if let Some(en) = branch.get("enum").and_then(|e| e.as_array()) {
                    flat.extend(en.iter().filter_map(|x| x.as_str()));
                }
                if let Some(c) = branch.get("const").and_then(|x| x.as_str()) {
                    flat.push(c);
                }
            }
        }
        for needle in ["full", "overwrite", "replace_lines", "insert_after_line"] {
            assert!(
                flat.contains(&needle),
                "ModifyFileMode schema should include {needle}: collected={flat:?}"
            );
        }
    }
}
