//! 在工具执行前对照内置 `FunctionDef` 的 `parameters` 做 **JSON Schema** 校验
//!（`jsonschema` 自动检测草案版本），与发给上游的 `tools` 定义保持一致。
//!
//! 与工具「必填键」粗检互补：
//! 此模块还校验类型、枚举、数值范围、嵌套子对象、以及 `additionalProperties` 等。
//!
//! 内置工具：在 Schema 校验与 runner 执行前做**一轮**确定性参数纠错（行号字符串、`mode` 同义词、
//! `path` / `content` / `from` / `to` 的入站别名、守卫键上抬等）。
//!
//! 规则本身是**数据**，声明在各工具的 `builtin_tool! { … coerce: … }`（策略常量与引擎见
//! [`super::tool_args_coerce`]）；本模块只负责「查 spec → 跑策略 → 校验」，见
//! [`coerce_builtin_tool_args_value`] 与 [`effective_builtin_tool_args_json`]。
//! 历史实现按工具名 `if` 分派，新增参数要改两处，漏改只会静默少做一步归一（#1381）。

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

use jsonschema::Validator;
use serde_json::Value;

use crate::cm_names as tool_names;
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

/// 对已知内置工具 JSON 对象做一轮宽容化（顶层须为 object）。
///
/// 规则来自 [`ToolSpec::args_coercion`]（工具在 `builtin_tool!` 里用 `coerce:` 声明），
/// 本函数**不再按工具名分支**：新增/改名参数只需改 Args 侧的声明，漏声明会由
/// `every_tool_with_path_property_declares_the_path_policy` 这类门禁用例拦住。
///
/// [`ToolSpec::args_coercion`]: super::ToolSpec
pub fn coerce_builtin_tool_args_value(name: &str, v: &mut Value) -> bool {
    let Some(spec) = super::find_spec(name) else {
        return false;
    };
    super::tool_args_coerce::coerce_args_value(spec.args_coercion, v)
}


/// 内置工具入参 JSON：解析成功后做一轮 [`coerce_builtin_tool_args_value`]，若有改写则返回序列化字符串供校验与 runner 共用。
pub fn effective_builtin_tool_args_json<'a>(
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
    let e1_text = e1.to_string();
    let mut s = if e1.instance_path().as_str().is_empty() {
        e1_text.clone()
    } else {
        format!("在路径 {}: {}", e1.instance_path(), e1_text)
    };
    s.push_str(&numeric_bound_hint(&e1_text));
    for (i, e) in iter.enumerate() {
        if i >= 2 {
            s.push_str(" …");
            break;
        }
        let t = e.to_string();
        s.push('；');
        s.push_str(&t);
        s.push_str(&numeric_bound_hint(&t));
    }
    format!(
        "参数与工具 JSON Schema 不一致：{s}（可对照本工具 parameters 的 required/类型/enum 等修正后重试。）"
    )
}

/// 数值越界时把**可用的值**摊进错误文案，而不只是指出违规。
///
/// `jsonschema` 的原文是 `14 is greater than the maximum of 10`——读完之后仍要自己回忆上限是多少，
/// 于是下一刀照样撞（实测 `search_in_files.context_after` 在多个仓库/实例/会话里被反复传 12/14/40）。
/// 这里直接附上界值，把"再撞一次"换成"改这个数"。
fn numeric_bound_hint(err_text: &str) -> String {
    let toks: Vec<&str> = err_text.split_whitespace().collect();
    let Some(pos) = toks.iter().position(|t| *t == "maximum" || *t == "minimum") else {
        return String::new();
    };
    // 形如 `… greater than the maximum of 10`：界值在关键字后第二个 token。
    let Some(raw) = toks.get(pos + 2) else {
        return String::new();
    };
    let limit: String = raw
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '-')
        .collect();
    if limit.is_empty() {
        return String::new();
    }
    let advice = if toks[pos] == "maximum" {
        "改为此值或更小"
    } else {
        "改为此值或更大"
    };
    format!("（{} = {limit}，请{advice}）", toks[pos])
}

/// 对已成功解析的 JSON 入参做 Schema 校验。仅内置工具有定义；`None` 表示无对应验证器（如未知 / MCP 名）。
pub fn validate_parsed_value_if_known(name: &str, args: &Value) -> Option<Result<(), String>> {
    let v = validators_map().get(name)?;
    if v.is_valid(args) {
        return Some(Ok(()));
    }
    Some(Err(format_instance_errors(v, args)))
}

/// 对 `args_json` 做「解析为 JSON +（**仅内置工具表** 中的名称）全量 Schema 校验」。
/// 非内置名返回 `None`（调用方不据此拦截）。解析错误与 [`parse_args::parse_args_json`] 一致。
pub fn validate_parsed_str_for_builtin(name: &str, args_json: &str) -> Option<Result<(), String>> {
    if !validators_map().contains_key(name) {
        return None;
    }
    let args = match super::parse_args::parse_args_json(args_json) {
        Ok(a) => a,
        Err(e) => {
            if name == tool_names::RUN_COMMAND
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
    /// 门禁：schema 有 `path` 的工具必须声明 `path` 策略，反之亦然。
    ///
    /// 迁移（#1381）时这条就是**等价性判据**：旧实现是「探一下 schema 有没有 `path` 属性再决定
    /// 是否做别名归一」，所以「schema 有 path」与「做了 path 归一」曾天然一致；改成声明式之后，
    /// 丢一次声明不会编译报错，只会让某工具静默失去别名容错 —— 由本测试守住。
    #[test]
    fn every_tool_with_path_property_declares_the_path_policy() {
        for spec in tool_specs() {
            let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
            let has_path = schema
                .get("properties")
                .and_then(|p| p.get("path"))
                .is_some();
            let declared = spec
                .args_coercion
                .fields
                .iter()
                .any(|f| f.key == "path");
            assert_eq!(
                has_path, declared,
                "工具 {}：schema.properties.path 存在性（{has_path}）与 coerce 声明（{declared}）不一致",
                spec.name
            );
        }
    }

    /// 门禁：策略里的目标键必须是该工具 schema 的 `properties` 键，别名则**不得**出现。
    ///
    /// 前半句抓「策略写了一个 schema 不接受的键」（coerce 之后必然校验失败），
    /// 后半句钉住非目标：别名只做入站兼容，不能被反写进对外 JSON Schema。
    #[test]
    fn coercion_policy_keys_match_schema_properties() {
        for spec in tool_specs() {
            let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
            let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
                assert!(
                    spec.args_coercion.fields.is_empty(),
                    "工具 {} 没有 properties 却声明了 coerce",
                    spec.name
                );
                continue;
            };
            for field in spec.args_coercion.fields {
                assert!(
                    props.contains_key(field.key),
                    "工具 {}：策略目标键 `{}` 不在 schema properties 里",
                    spec.name,
                    field.key
                );
                for alias in field.aliases {
                    assert!(
                        !props.contains_key(*alias),
                        "工具 {}：别名 `{alias}` 不应出现在 schema properties 里",
                        spec.name
                    );
                }
            }
            // 结构规则也会**写入**键（`default_empty_content` 写 `content`），
            // 只查 `fields` 会漏掉「挂错策略 ⇒ coerce 造出一个 schema 不接受的键」。
            if spec.args_coercion.structural.default_empty_content {
                assert!(
                    props.contains_key("content"),
                    "工具 {}：声明了 default_empty_content，但 schema 没有 content 属性",
                    spec.name
                );
            }
        }
    }
}
