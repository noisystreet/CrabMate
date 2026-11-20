//! #1381 门禁：宽容化策略声明 ↔ JSON Schema 一致性（遍历 `tool_specs()`，不写死工具清单）。

use super::*;
use crate::cm_tools::tools::ToolSpec;
use crate::cm_tools::tools::tool_args_coerce::{ArgsCoercionPolicy, FieldKind};

// ── #1381 门禁：策略声明 ↔ JSON Schema 一致性 ───────────────────────────
//
// 宽容化从「按工具名 if」变成「数据」之后，两类漂移不再会编译报错，只会静默降级：
// - 丢一次 `coerce:` 声明 ⇒ 某工具失去别名容错（模型传 `file_path` 时突然 invalid_args）；
// - 同义词目标 / 数值下界写错 ⇒ 只在模型恰好用那个写法时才暴露。
// 下面几条把两者都变成测试失败，且**遍历 `tool_specs()`**（不靠手写工具清单）。

/// 可能需要入站别名的通用键，以及**有意不做**该键宽容化的工具（必须写明理由）。
///
/// 新增工具若 schema 带这些键却既没声明策略、也没进豁免表，门禁 1 会失败 ——
/// 强迫在这里做一次显式决策，而不是悄悄「没有别名容错」。
const ALIAS_KEYS: &[(&str, &[&str])] = &[
    // `path` 全部是工作区路径，无一例外。
    ("path", &[]),
    // `skill_manage` 的 `content` 属新工具，无历史别名写法需要兼容，保持严格。
    ("content", &[tool_names::SKILL_MANAGE]),
    // `date_calc` 的 `from` / `to` 是日期，不是路径：按路径归一（trim、数字转串）没有意义。
    ("from", &[tool_names::DATE_CALC]),
    ("to", &[tool_names::DATE_CALC]),
];

/// 门禁 1：schema 出现通用键 ⇔ 该工具声明了对应策略（豁免表除外）。
#[test]
fn alias_keys_in_schema_require_matching_policy() {
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
        for (key, exempt) in ALIAS_KEYS {
            let in_schema = schema.pointer(&format!("/properties/{key}")).is_some();
            let has_policy = spec.args_coercion.fields.iter().any(|f| f.key == *key);
            if exempt.contains(&spec.name) {
                assert!(
                    !has_policy,
                    "工具 {}：`{key}` 已列入豁免表，却又声明了策略",
                    spec.name
                );
                continue;
            }
            assert_eq!(
                in_schema, has_policy,
                "工具 {}：schema.properties.{key} 存在（{in_schema}）但策略声明（{has_policy}）—— 要么在 spec 里加 `coerce:`，要么在 ALIAS_KEYS 豁免表里写明理由",
                spec.name
            );
        }
    }
}

/// 门禁 2：策略目标键必须是该工具 schema 的 `properties` 键，别名则**不得**出现。
///
/// 前半句抓「策略写了一个 schema 不接受的键」（coerce 之后必然校验失败）；
/// 后半句钉住非目标：别名只做入站兼容，不能反写进对外 JSON Schema。
/// 结构规则也会**写入**键（`default_empty_content` 写 `content`），一并核对。
#[test]
fn coercion_policy_keys_match_schema_properties() {
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
        for (policy, props, layer) in policy_layers(spec, &schema) {
            for field in policy.fields {
                assert!(
                    props.contains_key(field.key),
                    "工具 {} {layer}：策略目标键 `{}` 不在 schema properties 里",
                    spec.name,
                    field.key
                );
                for alias in field.aliases {
                    assert!(
                        !props.contains_key(*alias),
                        "工具 {} {layer}：别名 `{alias}` 不应出现在 schema properties 里",
                        spec.name
                    );
                }
            }
            if policy.structural.default_empty_content {
                assert!(
                    props.contains_key("content"),
                    "工具 {} {layer}：声明了 default_empty_content，但 schema 没有 content 属性",
                    spec.name
                );
            }
        }
    }
}

/// 门禁 3：`U64Min(n)` 必须与 schema 的 `minimum` 一致。
///
/// 比 schema **宽** ⇒ coerce 放行一个必然被校验拒掉的值（白转一次，报错从「类型不符」变成「越界」）；
/// 比 schema **严** ⇒ 合法值不被转换，报错退化成「expected integer, got string」。
#[test]
fn numeric_bounds_match_schema_minimum() {
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
        for (policy, props, layer) in policy_layers(spec, &schema) {
            assert_bounds_align(spec.name, layer, policy, props);
        }
    }
}

/// 门禁 4：`FieldKind` 的值类型必须与 schema `type` 兼容（`null` 忽略）。
///
/// 抓「给字符串字段声明了 `U64Min`」这类错配：那种策略会把合法输入改坏，或干脆永不生效。
///
/// **盲点（有意）**：schema 用 `$ref` / `anyOf` 而不内联 `type` 时（如 `modify_file.mode`）
/// 无从判断类型，本门禁跳过 —— 那类字段由门禁 5 的「同义词目标 ∈ schema 枚举」兜住。
#[test]
fn field_kind_matches_schema_type() {
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
        for (policy, props, layer) in policy_layers(spec, &schema) {
            assert_kinds_align(spec.name, layer, policy, props);
        }
    }
}

/// 门禁 5：枚举同义词的**目标值**必须真的出现在 schema 枚举里。
///
/// `modify_file.mode` 的 schema 是 `$ref` + `oneOf`/`const` 混合形态，写错一个目标值
/// （例如 `replace_line` 少个 `s`）只会让模型传该同义词时撞上 invalid_args。
#[test]
fn enum_synonym_targets_exist_in_schema_enum() {
    for spec in tool_specs() {
        let schema = cached_params_for_tool_name(spec.name).expect("parameters 已缓存");
        for (policy, props, layer) in policy_layers(spec, &schema) {
            for field in policy.fields {
                assert_enum_targets(spec.name, layer, &schema, props, field);
            }
        }
    }
}

/// 门禁 6：用户实际依赖的宽容化「能力」不许静默缩小。
///
/// 门禁 1–5 都只检查**已声明**的字段，所以「把 `ReadFileArgs` 绑到 `PATH_ONLY`」这类
/// 类型侧写错仍然会全绿，而行号宽容化就静默失效了。这条用一句话钉住行为面：
/// 关键字段必须在，允许以后新增、但删掉或整条策略被换掉必须显式改这里。
#[test]
fn permissive_tools_keep_their_documented_fields() {
    const EXPECT: &[(&str, &[&str])] = &[
        (
            tool_names::READ_FILE,
            &[
                "path",
                "start_line",
                "end_line",
                "max_lines",
                "anchor_line",
                "context_lines",
                "encoding",
                "count_total_lines",
            ],
        ),
        (
            tool_names::MODIFY_FILE,
            &[
                "path",
                "content",
                "start_line",
                "end_line",
                "after_line",
                "mode",
            ],
        ),
        (tool_names::CREATE_FILE, &["path", "content"]),
        (tool_names::APPEND_FILE, &["path", "content"]),
        (tool_names::COPY_FILE, &["from", "to"]),
        (tool_names::MOVE_FILE, &["from", "to"]),
    ];
    for (tool, keys) in EXPECT {
        let spec = tool_specs()
            .iter()
            .find(|s| s.name == *tool)
            .unwrap_or_else(|| panic!("注册表里应有工具 {tool}"));
        let declared: Vec<&str> = spec.args_coercion.fields.iter().map(|f| f.key).collect();
        for key in *keys {
            assert!(
                declared.contains(key),
                "工具 {tool} 的宽容化字段缺少 `{key}`（当前声明：{declared:?}）—— 这会让模型传该参数别名/字符串数字时突然 invalid_args"
            );
        }
    }
    // `modify_file` 的 `edits[]` 逐项归一不能被顺手丢掉。
    let modify = tool_specs()
        .iter()
        .find(|s| s.name == tool_names::MODIFY_FILE)
        .expect("modify_file");
    assert!(
        modify.args_coercion.nested_edits.is_some(),
        "modify_file 的 nested_edits 丢失：`edits[i]` 的行号/mode 归一会失效"
    );
}

// ── 门禁用的 schema 小工具 ─────────────────────────────────────────────

/// 策略分层视图：顶层 + `edits[]`（后者按 `properties.edits.items` 的 `properties` 对照）。
fn policy_layers<'a>(
    spec: &'a ToolSpec,
    schema: &'a Value,
) -> Vec<(&'a ArgsCoercionPolicy, &'a serde_json::Map<String, Value>, &'static str)> {
    let mut out = Vec::new();
    match schema.get("properties").and_then(|p| p.as_object()) {
        Some(props) => out.push((spec.args_coercion, props, "顶层")),
        // schema 连 `properties` 都没有时，门禁会「什么都不查」而静默通过；
        // 这正是要拦住的一类：策略挂在了一个没有参数的对象上。
        None => assert!(
            spec.args_coercion.fields.is_empty(),
            "工具 {}：schema 没有 properties，却声明了 {} 条宽容化字段",
            spec.name,
            spec.args_coercion.fields.len()
        ),
    }
    if let Some(nested) = spec.args_coercion.nested_edits {
        let props = edits_item_props(schema).unwrap_or_else(|| {
            panic!(
                "工具 {} 声明了 nested_edits，但 schema 没有 properties.edits.items 的 properties",
                spec.name
            )
        });
        out.push((nested, props, "edits[]"));
    }
    out
}

/// `properties.edits.items` 指向的对象属性（解一层 `$ref` → `$defs`）。
fn edits_item_props(schema: &Value) -> Option<&serde_json::Map<String, Value>> {
    let items = schema.pointer("/properties/edits/items")?;
    let target = match items.get("$ref").and_then(|r| r.as_str()) {
        Some(reference) => schema_def(schema, reference)?,
        None => items,
    };
    target.get("properties")?.as_object()
}

fn schema_def<'a>(root: &'a Value, reference: &str) -> Option<&'a Value> {
    let name = reference.rsplit('/').next()?;
    root.pointer(&format!("/$defs/{name}"))
}

fn assert_bounds_align(
    tool: &str,
    layer: &str,
    policy: &ArgsCoercionPolicy,
    props: &serde_json::Map<String, Value>,
) {
    for field in policy.fields {
        let FieldKind::U64Min(min) = field.kind else {
            continue;
        };
        let prop = props
            .get(field.key)
            .unwrap_or_else(|| panic!("工具 {tool} {layer}：`{}` 不在 schema properties 里", field.key));
        let schema_min = prop
            .get("minimum")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| {
                panic!(
                    "工具 {tool} {layer}：`{}` 声明了 U64Min({min})，但 schema 没有 minimum",
                    field.key
                )
            });
        assert_eq!(
            min, schema_min,
            "工具 {tool} {layer}：`{}` 的 coerce 下界 {min} 与 schema minimum {schema_min} 不一致",
            field.key
        );
    }
}

fn assert_kinds_align(
    tool: &str,
    layer: &str,
    policy: &ArgsCoercionPolicy,
    props: &serde_json::Map<String, Value>,
) {
    for field in policy.fields {
        let Some(want) = expected_json_types(field.kind) else {
            continue;
        };
        let Some(prop) = props.get(field.key) else {
            continue;
        };
        let Some(actual) = schema_types(prop) else {
            continue;
        };
        assert!(
            want.iter().any(|w| actual.iter().any(|a| a == w)),
            "工具 {tool} {layer}：`{}` 的 FieldKind 期望 {want:?}，schema 类型却是 {actual:?}",
            field.key
        );
    }
}

fn assert_enum_targets(
    tool: &str,
    layer: &str,
    schema: &Value,
    props: &serde_json::Map<String, Value>,
    field: &crate::cm_tools::tools::tool_args_coerce::FieldCoercion,
) {
    let table = match field.kind {
        FieldKind::EnumSynonyms(t) | FieldKind::EnumSynonymsStrict(t) => t,
        _ => return,
    };
    let Some(prop) = props.get(field.key) else {
        return;
    };
    let mut allowed = Vec::new();
    collect_schema_strings(schema, prop, &mut allowed);
    assert!(
        !allowed.is_empty(),
        "工具 {tool} {layer}：`{}` 的 schema 里取不到枚举值，无法校验同义词表",
        field.key
    );
    for &(from, to) in table {
        assert!(
            allowed.contains(&to),
            "工具 {tool} {layer}：`{}` 的同义词 `{from}` → `{to}`，但 schema 枚举是 {allowed:?}",
            field.key
        );
    }
}

/// `FieldKind` 期望的 JSON 类型；`Keep`（只搬键）不约束值类型。
fn expected_json_types(kind: FieldKind) -> Option<&'static [&'static str]> {
    match kind {
        FieldKind::Keep => None,
        FieldKind::PathString
        | FieldKind::TrimmedStr
        | FieldKind::EnumSynonyms(_)
        | FieldKind::EnumSynonymsStrict(_) => Some(&["string"]),
        FieldKind::U64Min(_) => Some(&["integer"]),
        FieldKind::Bool => Some(&["boolean"]),
    }
}

/// schema 声明的 JSON 类型（去掉 `null`；无 `type` 时为 `None`，例如 `$ref` / `anyOf`）。
fn schema_types(prop: &Value) -> Option<Vec<&str>> {
    let raw = match prop.get("type")? {
        Value::String(s) => vec![s.as_str()],
        Value::Array(items) => items.iter().filter_map(|x| x.as_str()).collect(),
        _ => return None,
    };
    Some(raw.into_iter().filter(|s| *s != "null").collect())
}

/// 收集枚举字面量：解 `$ref` → `$defs`，并展开 `anyOf` / `oneOf` / `allOf` 分支
/// （`schemars` 对带文档的变体输出 `const`，对无文档的输出 `enum`）。
fn collect_schema_strings<'a>(root: &'a Value, node: &'a Value, out: &mut Vec<&'a str>) {
    if let Some(reference) = node.get("$ref").and_then(|r| r.as_str()) {
        if let Some(def) = schema_def(root, reference) {
            collect_schema_strings(root, def, out);
        }
        return;
    }
    if let Some(list) = node.get("enum").and_then(|e| e.as_array()) {
        out.extend(list.iter().filter_map(|x| x.as_str()));
    }
    if let Some(c) = node.get("const").and_then(|c| c.as_str()) {
        out.push(c);
    }
    for key in ["anyOf", "oneOf", "allOf"] {
        if let Some(branches) = node.get(key).and_then(|b| b.as_array()) {
            for branch in branches {
                collect_schema_strings(root, branch, out);
            }
        }
    }
}
