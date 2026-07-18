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
    use crate::tools::tool_param_types::{
        AddReminderArgs, ArchivePackArgs, AstGrepRunArgs, BacktraceAnalyzeArgs, CalcArgs,
        CallGraphSketchArgs, ChmodFileArgs, CiPipelineLocalArgs, CodeStatsArgs, CoverageReportArgs,
        DependencyGraphArgs, DocsHealthSweepArgs, FindReferencesArgs, FindSymbolArgs,
        FormatOnePathArgs, GitStatusArgs, GoBuildArgs, GolangciLintArgs, GradleTasksArgs,
        ListRemindersArgs, MarkdownCheckLinksArgs, MavenCompileArgs, ModifyFileArgs, NpmRunArgs,
        PackageQueryArgs, PortCheckArgs, ProcessListArgs, QualityWorkspaceArgs, RunCommandArgs,
        RunLintsArgs, ShellcheckCheckArgs, StructuredValidateArgs, SymlinkInfoArgs, TableTextArgs,
        TodoScanArgs, WorkflowExecuteArgs,
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
        // schemars 1.x emits integer bounds (1) where 0.8 emitted floats (1.0); compare as f64.
        assert_eq!(port.get("minimum").and_then(|x| x.as_f64()), Some(1.0));
        assert_eq!(port.get("maximum").and_then(|x| x.as_f64()), Some(65535.0));
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
    fn package_query_schema_requires_package() {
        let v = tool_parameters_schema_value::<PackageQueryArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required array");
        assert!(req.iter().any(|x| x == "package"));
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "deny_unknown_fields"
        );
    }

    #[test]
    fn todo_scan_schema_has_paths_markers_exclude() {
        let v = tool_parameters_schema_value::<TodoScanArgs>();
        assert!(v.pointer("/properties/paths").is_some());
        assert!(v.pointer("/properties/markers").is_some());
        assert!(v.pointer("/properties/exclude").is_some());
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
    fn npm_run_schema_requires_script() {
        let v = tool_parameters_schema_value::<NpmRunArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "script"));
    }

    #[test]
    fn go_build_schema_has_race_verbose_tags() {
        let v = tool_parameters_schema_value::<GoBuildArgs>();
        assert!(v.pointer("/properties/race").is_some());
        assert!(v.pointer("/properties/verbose").is_some());
        assert!(v.pointer("/properties/tags").is_some());
    }

    #[test]
    fn shellcheck_schema_denies_unknown() {
        let v = tool_parameters_schema_value::<ShellcheckCheckArgs>();
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn format_file_schema_requires_path() {
        let v = tool_parameters_schema_value::<FormatOnePathArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("req");
        assert!(req.iter().any(|x| x == "path"));
    }

    #[test]
    fn run_lints_schema_has_run_cargo_defaults() {
        let v = tool_parameters_schema_value::<RunLintsArgs>();
        assert!(v.pointer("/properties/run_cargo").is_some());
    }

    #[test]
    fn quality_workspace_schema_has_many_flags() {
        let v = tool_parameters_schema_value::<QualityWorkspaceArgs>();
        assert!(v.pointer("/properties/run_cargo_fmt_check").is_some());
        assert!(v.pointer("/properties/run_podman_images").is_some());
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

    #[test]
    fn maven_compile_schema_has_optional_profile() {
        let v = tool_parameters_schema_value::<MavenCompileArgs>();
        assert!(v.pointer("/properties/profile").is_some());
    }

    #[test]
    fn gradle_tasks_schema_has_tasks_array() {
        let v = tool_parameters_schema_value::<GradleTasksArgs>();
        assert_eq!(v.pointer("/properties/tasks/type"), Some(&json!("array")));
    }

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
    fn find_symbol_schema_requires_symbol() {
        let v = tool_parameters_schema_value::<FindSymbolArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "symbol"));
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn find_references_schema_requires_symbol() {
        let v = tool_parameters_schema_value::<FindReferencesArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "symbol"));
    }

    #[test]
    fn call_graph_sketch_schema_denies_unknown() {
        let v = tool_parameters_schema_value::<CallGraphSketchArgs>();
        assert_eq!(
            v.pointer("/additionalProperties"),
            Some(&json!(false)),
            "{v}"
        );
    }

    #[test]
    fn structured_validate_schema_requires_path() {
        let v = tool_parameters_schema_value::<StructuredValidateArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "path"));
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
    fn backtrace_analyze_schema_requires_backtrace() {
        let v = tool_parameters_schema_value::<BacktraceAnalyzeArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "backtrace"));
    }

    #[test]
    fn ci_pipeline_local_schema_has_run_fmt() {
        let v = tool_parameters_schema_value::<CiPipelineLocalArgs>();
        assert!(v.pointer("/properties/run_fmt").is_some());
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
    fn ast_grep_run_schema_requires_pattern_and_lang() {
        let v = tool_parameters_schema_value::<AstGrepRunArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "pattern"));
        assert!(req.iter().any(|x| x == "lang"));
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
    fn symlink_info_schema_requires_path() {
        let v = tool_parameters_schema_value::<SymlinkInfoArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "path"));
        assert!(v.pointer("/properties/path").is_some());
    }

    #[test]
    fn run_command_schema_requires_command() {
        let v = tool_parameters_schema_value::<RunCommandArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required array");
        assert!(req.iter().any(|x| x == "command"));
    }

    #[test]
    fn git_status_schema_optional_flags() {
        let v = tool_parameters_schema_value::<GitStatusArgs>();
        assert_eq!(v.get("type"), Some(&json!("object")));
        assert!(v.pointer("/properties/porcelain").is_some());
        assert!(v.pointer("/properties/branch").is_some());
    }

    #[test]
    fn chmod_file_schema_requires_path_and_mode() {
        let v = tool_parameters_schema_value::<ChmodFileArgs>();
        let req = v
            .pointer("/required")
            .and_then(|r| r.as_array())
            .expect("required");
        assert!(req.iter().any(|x| x == "path"));
        assert!(req.iter().any(|x| x == "mode"));
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
        for needle in ["full", "overwrite", "replace_lines"] {
            assert!(
                flat.contains(&needle),
                "ModifyFileMode schema should include {needle}: collected={flat:?}"
            );
        }
    }
}
