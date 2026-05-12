//! 在工具执行前对照内置 `FunctionDef` 的 `parameters` 做 **JSON Schema** 校验
//!（`jsonschema` 自动检测草案版本），与发给上游的 `tools` 定义保持一致。
//!
//! 与 [`super::schema_check::workflow_tool_args_satisfy_required`] 的「必填键」粗检互补：
//! 此模块还校验类型、枚举、数值范围、嵌套子对象、以及 `additionalProperties` 等。
//!
//! 内置工具：在 Schema 校验与 runner 执行前做**一轮**确定性参数纠错（如 `read_file` / **`modify_file`**
//! 的 **`start_line` / `end_line`** 字符串与整型 JSON 数字、`modify_file` 的 **`mode`** 大小写/同义词、
//! `path` 的常见别名（**`file_path`** / **`filename`** / **`output_path`** 等）、**`copy_file`** 的 **`src`/`dst`**、
//! **`create_file`** 的 **`content`** 别名与缺省空串等），见 [`coerce_builtin_tool_args_value`]、
//! [`effective_builtin_tool_args_json`]。

use std::borrow::Cow;
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
fn json_number_to_u64(n: &serde_json::Number) -> Option<u64> {
    n.as_u64().or_else(|| {
        let f = n.as_f64()?;
        if f.is_finite() && f >= 1.0 && f.fract() == 0.0 {
            Some(f as u64)
        } else {
            None
        }
    })
}

fn coerce_path_string_field(path_val: &mut Value) -> bool {
    match path_val {
        Value::Number(n) => {
            *path_val = Value::String(n.to_string());
            true
        }
        Value::String(s) => {
            let t = s.trim();
            if t != s.as_str() {
                *path_val = Value::String(t.to_string());
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn coerce_read_file_one_u64_line_field(val: &mut Value) -> bool {
    match val {
        Value::String(s) => {
            let t = s.trim();
            if let Ok(n) = t.parse::<u64>()
                && n >= 1
            {
                *val = Value::Number(n.into());
                true
            } else {
                false
            }
        }
        Value::Number(n) => {
            let Some(u) = json_number_to_u64(n) else {
                return false;
            };
            if u < 1 {
                return false;
            }
            let new_v = Value::Number(u.into());
            if *val != new_v {
                *val = new_v;
                true
            } else {
                false
            }
        }
        _ => false,
    }
}

fn coerce_read_file_line_int_fields(map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    for key in [
        "start_line",
        "end_line",
        "max_lines",
        "anchor_line",
        "context_lines",
    ] {
        let Some(val) = map.get_mut(key) else {
            continue;
        };
        if coerce_read_file_one_u64_line_field(val) {
            changed = true;
        }
    }
    changed
}

fn coerce_read_file_encoding_trim(map: &mut serde_json::Map<String, Value>) -> bool {
    let Some(enc) = map.get_mut("encoding") else {
        return false;
    };
    let Value::String(s) = enc else {
        return false;
    };
    let t = s.trim();
    if t == s.as_str() {
        return false;
    }
    *enc = Value::String(t.to_string());
    true
}

fn coerce_read_file_count_total_lines(map: &mut serde_json::Map<String, Value>) -> bool {
    let Some(b) = map.get_mut("count_total_lines") else {
        return false;
    };
    match b {
        Value::String(s) => {
            let t = s.trim().to_ascii_lowercase();
            let nv = match t.as_str() {
                "true" | "1" | "yes" => Some(Value::Bool(true)),
                "false" | "0" | "no" => Some(Value::Bool(false)),
                _ => None,
            };
            let Some(vv) = nv else {
                return false;
            };
            *b = vv;
            true
        }
        Value::Number(n) => {
            let Some(u) = n.as_u64() else {
                return false;
            };
            if !matches!(u, 0 | 1) {
                return false;
            }
            *b = Value::Bool(u == 1);
            true
        }
        _ => false,
    }
}

/// `modify_file` 的 `replace_lines` 与 `read_file` 一样常用字符串行号；`mode` 常被写成大小写变体或 `lines`。
fn coerce_modify_file_extra_fields(map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    for key in ["start_line", "end_line"] {
        let Some(val) = map.get_mut(key) else {
            continue;
        };
        if coerce_read_file_one_u64_line_field(val) {
            changed = true;
        }
    }
    let Some(mode_val) = map.get_mut("mode") else {
        return changed;
    };
    let Value::String(s) = mode_val else {
        return changed;
    };
    let norm: String = s
        .trim()
        .chars()
        .map(|c| {
            if c.is_whitespace() {
                '_'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect();
    let mapped = match norm.as_str() {
        "lines" | "line" | "line_replace" | "replace" | "partial" => "replace_lines".to_string(),
        "replacelines" => "replace_lines".to_string(),
        "full" | "overwrite" | "whole" | "whole_file" => "full".to_string(),
        other if other == "replace_lines" || other == "full" => other.to_string(),
        _ => norm,
    };
    if mapped.as_str() != s.as_str() {
        *mode_val = Value::String(mapped);
        changed = true;
    }
    changed
}

fn schema_has_top_level_prop(tool_name: &str, prop: &str) -> bool {
    cached_params_for_tool_name(tool_name)
        .and_then(|schema| schema.get("properties").cloned())
        .and_then(|props| props.get(prop).cloned())
        .is_some()
}

fn remap_path_aliases(map: &mut serde_json::Map<String, Value>) -> bool {
    if map.contains_key("path") {
        return false;
    }
    const ALIASES: &[&str] = &[
        "file_path",
        "filepath",
        "relative_path",
        "rel_path",
        "filename",
        "file_name",
        "output_path",
        "write_path",
        "target_file",
        "dest_path",
        "destination_path",
    ];
    for key in ALIASES {
        if let Some(v) = map.remove(*key) {
            map.insert("path".to_string(), v);
            return true;
        }
    }
    false
}

fn remap_content_aliases(map: &mut serde_json::Map<String, Value>) -> bool {
    if map.contains_key("content") {
        return false;
    }
    const ALIASES: &[&str] = &["file_content", "body", "text"];
    for key in ALIASES {
        if let Some(v) = map.remove(*key) {
            map.insert("content".to_string(), v);
            return true;
        }
    }
    false
}

fn remap_from_to_aliases(map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    if !map.contains_key("from") {
        const FROM_KEYS: &[&str] = &["src", "source", "source_path", "old_path", "from_path"];
        for key in FROM_KEYS {
            if let Some(v) = map.remove(*key) {
                map.insert("from".to_string(), v);
                changed = true;
                break;
            }
        }
    }
    if !map.contains_key("to") {
        const TO_KEYS: &[&str] = &[
            "dst",
            "dest",
            "destination",
            "target",
            "target_path",
            "new_path",
            "to_path",
        ];
        for key in TO_KEYS {
            if let Some(v) = map.remove(*key) {
                map.insert("to".to_string(), v);
                changed = true;
                break;
            }
        }
    }
    changed
}

fn coerce_read_file_extra_fields(map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    if coerce_read_file_line_int_fields(map) {
        changed = true;
    }
    if coerce_read_file_encoding_trim(map) {
        changed = true;
    }
    if coerce_read_file_count_total_lines(map) {
        changed = true;
    }
    changed
}

fn apply_path_coercions(name: &str, map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    if schema_has_top_level_prop(name, "path") {
        if remap_path_aliases(map) {
            changed = true;
        }
        if let Some(p) = map.get_mut("path")
            && coerce_path_string_field(p)
        {
            changed = true;
        }
    }
    if matches!(name, "copy_file" | "move_file") {
        if remap_from_to_aliases(map) {
            changed = true;
        }
        for key in ["from", "to"] {
            if let Some(x) = map.get_mut(key)
                && coerce_path_string_field(x)
            {
                changed = true;
            }
        }
    }
    changed
}

fn apply_write_content_coercions(name: &str, map: &mut serde_json::Map<String, Value>) -> bool {
    let mut changed = false;
    if matches!(name, "create_file" | "modify_file")
        && schema_has_top_level_prop(name, "content")
        && remap_content_aliases(map)
    {
        changed = true;
    }
    if name == "create_file" && schema_has_top_level_prop(name, "content") {
        let needs_empty_default = matches!(map.get("content"), None | Some(Value::Null));
        if needs_empty_default {
            map.insert("content".to_string(), Value::String(String::new()));
            changed = true;
        }
    }
    changed
}

/// 对已知内置工具 JSON 对象做一轮纠错（顶层须为 object）。与各自 Schema 对齐：仅当 schema 含对应属性时才做路径别名等。
pub(crate) fn coerce_builtin_tool_args_value(name: &str, v: &mut Value) -> bool {
    let Value::Object(map) = v else {
        return false;
    };
    let mut changed = false;
    if apply_path_coercions(name, map) {
        changed = true;
    }
    if apply_write_content_coercions(name, map) {
        changed = true;
    }
    if name == "read_file" && coerce_read_file_extra_fields(map) {
        changed = true;
    }
    if name == "modify_file" && coerce_modify_file_extra_fields(map) {
        changed = true;
    }
    changed
}

/// 内置工具入参 JSON：解析成功后做一轮 [`coerce_builtin_tool_args_value`]，若有改写则返回序列化字符串供校验与 runner 共用。
pub(crate) fn effective_builtin_tool_args_json<'a>(
    name: &str,
    args_json: &'a str,
) -> Result<Cow<'a, str>, String> {
    if !validators_map().contains_key(name) {
        return Ok(Cow::Borrowed(args_json));
    }
    let mut v = super::parse_args::parse_args_json(args_json)?;
    if !coerce_builtin_tool_args_value(name, &mut v) {
        return Ok(Cow::Borrowed(args_json));
    }
    serde_json::to_string(&v)
        .map(Cow::Owned)
        .map_err(|e| format!("工具 {name} 参数纠错后 JSON 序列化失败: {e}"))
}

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
}
