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

use super::cached_params_for_tool_name;
use super::tool_specs_registry::tool_specs;
use crate::cm_names as tool_names;

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
/// `alias_keys_in_schema_require_matching_policy` 这类门禁用例拦住。
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
mod gates;
#[cfg(test)]
mod tests;
