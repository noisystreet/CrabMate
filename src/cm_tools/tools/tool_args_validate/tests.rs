use super::*;
use serde_json::json;

#[test]
fn known_tool_rejects_wrong_type() {
    let r = validate_parsed_value_if_known("json_format", &json!({ "text": 1 })).expect("known");
    assert!(r.is_err());
    let msg = r.unwrap_err();
    assert!(msg.contains("参数与工具 JSON Schema"), "{}", msg);
}

/// 数值越界要**直接给出可用值**，而不只是指出违规。
///
/// 回放实测：`search_in_files.context_after`（上限 10）在多个仓库 / 实例 / 会话里被反复传 12/14/40，
/// 因为原本文案只说「greater than the maximum of 10」，读完还得自己回忆上限。
#[test]
fn numeric_bound_violation_suggests_the_usable_value() {
    let r = validate_parsed_value_if_known(
        "search_in_files",
        &json!({ "pattern": "x", "context_after": 14 }),
    )
    .expect("known");
    let msg = r.expect_err("context_after=14 应越界");
    assert!(msg.contains("maximum"), "{}", msg);
    assert!(
        msg.contains("maximum = 10"),
        "越界提示应附上可用值：{msg}"
    );
    assert!(msg.contains("改为此值或更小"), "{}", msg);
}

/// 非数值类错误不该被塞进界值提示（文案保持原样）。
#[test]
fn non_numeric_violation_has_no_bound_hint() {
    let r = validate_parsed_value_if_known("json_format", &json!({ "text": 1 })).expect("known");
    let msg = r.expect_err("类型错应被拒");
    assert!(!msg.contains("改为此值"), "{}", msg);
}

#[test]
fn read_file_coerce_string_line_numbers_passes_schema() {
    let mut v = json!({
        "path": " README.md ",
        "start_line": "2",
        "end_line": "3",
        "count_total_lines": "false"
    });
    assert!(coerce_builtin_tool_args_value("read_file", &mut v));
    let r = validate_parsed_value_if_known("read_file", &v).expect("read_file registered");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn read_file_numeric_path_coerced_to_string() {
    let mut v = json!({ "path": 42 });
    assert!(coerce_builtin_tool_args_value("read_file", &mut v));
    assert_eq!(v["path"], "42");
    let r = validate_parsed_value_if_known("read_file", &v).expect("schema");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn create_file_file_path_and_body_aliases_pass_schema() {
    let mut v = json!({"file_path": "x.txt", "body": "hi"});
    assert!(coerce_builtin_tool_args_value("create_file", &mut v));
    assert_eq!(v["path"], "x.txt");
    assert_eq!(v["content"], "hi");
    let r = validate_parsed_value_if_known("create_file", &v).expect("create_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn create_file_filename_alias_passes_schema() {
    let mut v = json!({"filename": "dir/n.txt", "content": "x"});
    assert!(coerce_builtin_tool_args_value("create_file", &mut v));
    assert_eq!(v["path"], "dir/n.txt");
    assert_eq!(v["content"], "x");
    let r = validate_parsed_value_if_known("create_file", &v).expect("create_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn create_file_file_and_code_aliases_pass_schema() {
    let mut v = json!({"file": "main.rs", "code": "fn main() {}"});
    assert!(coerce_builtin_tool_args_value("create_file", &mut v));
    assert_eq!(v["path"], "main.rs");
    assert_eq!(v["content"], "fn main() {}");
    let r = validate_parsed_value_if_known("create_file", &v).expect("create_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn create_file_path_key_case_insensitive() {
    let mut v = json!({"Path": "a.txt", "Content": "z"});
    assert!(coerce_builtin_tool_args_value("create_file", &mut v));
    assert_eq!(v["path"], "a.txt");
    assert_eq!(v["content"], "z");
    let r = validate_parsed_value_if_known("create_file", &v).expect("create_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn create_file_path_only_gets_default_empty_content_for_schema() {
    let mut v = json!({"path": "only.txt"});
    assert!(coerce_builtin_tool_args_value("create_file", &mut v));
    assert_eq!(v["content"], "");
    let r = validate_parsed_value_if_known("create_file", &v).expect("create_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn copy_file_src_dst_aliases_pass_schema() {
    let mut v = json!({"src": "a.txt", "dst": "b.txt"});
    assert!(coerce_builtin_tool_args_value("copy_file", &mut v));
    assert_eq!(v["from"], "a.txt");
    assert_eq!(v["to"], "b.txt");
    let r = validate_parsed_value_if_known("copy_file", &v).expect("copy_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn effective_read_file_serializes_after_coercion() {
    let raw = r#"{"path":"x.rs","start_line":"1","max_lines":"100"}"#;
    let cow = effective_builtin_tool_args_json("read_file", raw).expect("ok");
    assert!(matches!(cow, Cow::Owned(_)));
    let r = validate_parsed_str_for_builtin("read_file", cow.as_ref()).expect("validator");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn effective_create_file_serializes_after_alias_coercion() {
    let raw = r#"{"file_path":"n.txt","text":"z"}"#;
    let cow = effective_builtin_tool_args_json("create_file", raw).expect("ok");
    assert!(matches!(cow, Cow::Owned(_)));
    let r = validate_parsed_str_for_builtin("create_file", cow.as_ref()).expect("validator");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn effective_append_file_serializes_after_alias_coercion() {
    let raw = r#"{"file_path":"n.txt","text":"z"}"#;
    let cow = effective_builtin_tool_args_json("append_file", raw).expect("ok");
    assert!(matches!(cow, Cow::Owned(_)));
    let v: Value = serde_json::from_str(cow.as_ref()).expect("json");
    assert_eq!(v["path"], "n.txt");
    assert_eq!(v["content"], "z");
    let r = validate_parsed_str_for_builtin("append_file", cow.as_ref()).expect("validator");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn modify_file_coerce_string_line_numbers_and_mode_passes_schema() {
    let mut v = json!({
        "path": "src/lib.rs",
        "mode": "REPLACE_LINES",
        "start_line": "10",
        "end_line": "12",
        "content": "fn x() {}\n"
    });
    assert!(coerce_builtin_tool_args_value("modify_file", &mut v));
    assert_eq!(v["mode"], "replace_lines");
    assert_eq!(v["start_line"].as_u64(), Some(10));
    assert_eq!(v["end_line"].as_u64(), Some(12));
    let r = validate_parsed_value_if_known("modify_file", &v).expect("modify_file registered");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn modify_file_mode_lines_synonym_passes_schema() {
    let mut v = json!({
        "path": "a.txt",
        "mode": "lines",
        "start_line": 1,
        "end_line": 1,
        "content": ""
    });
    assert!(coerce_builtin_tool_args_value("modify_file", &mut v));
    assert_eq!(v["mode"], "replace_lines");
    let r = validate_parsed_value_if_known("modify_file", &v).expect("modify_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn modify_file_insert_after_line_synonym_and_zero_line_pass_schema() {
    let mut v = json!({
        "path": "a.txt",
        "mode": "insert",
        "after_line": "0",
        "content": "x"
    });
    assert!(coerce_builtin_tool_args_value("modify_file", &mut v));
    assert_eq!(v["mode"], "insert_after_line");
    assert_eq!(v["after_line"].as_u64(), Some(0));
    let r = validate_parsed_value_if_known("modify_file", &v).expect("modify_file");
    assert!(r.is_ok(), "{r:?}");
}

#[test]
fn effective_create_file_path_only_includes_empty_content() {
    let raw = r#"{"output_path":"x"}"#;
    let cow = effective_builtin_tool_args_json("create_file", raw).expect("ok");
    assert!(matches!(cow, Cow::Owned(_)));
    assert!(
        cow.as_ref().contains("\"content\":\"\""),
        "{}",
        cow.as_ref()
    );
    let r = validate_parsed_str_for_builtin("create_file", cow.as_ref()).expect("validator");
    assert!(r.is_ok(), "{r:?}");
}

/// 旧平铺键在校验前被上抬到 `guard`，因此旧调用不会因 `deny_unknown_fields` 被拒。
#[test]
fn modify_file_legacy_guard_keys_uplift_into_guard_and_pass_schema() {
    let raw = r#"{"path":"a.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","expect_content":"L1","expect_first_line":""}"#;
    let cow = effective_builtin_tool_args_json("modify_file", raw).expect("ok");
    assert!(matches!(cow, Cow::Owned(_)));
    let v: Value = serde_json::from_str(cow.as_ref()).expect("json");
    assert_eq!(v["guard"]["content"], "L1");
    assert_eq!(v["guard"]["first_line"], "");
    assert!(v.get("expect_content").is_none(), "{v}");
    assert!(v.get("expect_first_line").is_none(), "{v}");
    let r = validate_parsed_str_for_builtin("modify_file", cow.as_ref()).expect("validator");
    assert!(r.is_ok(), "{r:?}");
}

/// schema 只暴露 `guard`：四个旧键名不再出现在工具定义里（否则模型会照旧写）。
#[test]
fn modify_file_guard_schema_exposes_guard_only() {
    let raw = r#"{"path":"a.txt","mode":"replace_lines","start_line":1,"end_line":1,"content":"X","guard":{"first_line":"L1","last_line":"L1"}}"#;
    let r = validate_parsed_str_for_builtin("modify_file", raw).expect("validator");
    assert!(r.is_ok(), "{r:?}");
    let schema =
        schemars::schema_for!(crate::cm_tools::tools::tool_param_types::ModifyFileArgs);
    let s = serde_json::to_string(&schema).expect("schema json");
    assert!(s.contains("\"guard\""), "{s}");
    for legacy in [
        "expect_content",
        "expect_first_line",
        "expect_last_line",
        "expect_line_content",
    ] {
        assert!(!s.contains(legacy), "schema 不应再暴露 {legacy}: {s}");
    }
}
