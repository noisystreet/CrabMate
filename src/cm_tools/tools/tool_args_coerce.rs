//! 内置工具入参的**声明式**宽容化策略（#1381）。
//!
//! 此前宽容化按工具名 `if` 分派（`tool_args_validate::apply_path_coercions` /
//! `apply_write_content_coercions`），新增或改名一个参数要同时改 Args 结构与那条分支，
//! 漏改不报错，只表现为「模型传别名时突然 `invalid_args`」。
//!
//! 现在每条规则都是**数据**（[`FieldCoercion`]），并**绑在 Args 类型上**（[`ArgsCoercion`]）：
//! spec 写 `coerce: auto`，`builtin_tool!` 展开成 `<Args as ArgsCoercion>::POLICY` 挂到
//! [`ToolSpec`]，运行时按 spec 查表 —— 既不看工具名，也不由 spec 选常量。
//!
//! 声明可被测试枚举：`tool_args_validate` 的 6 条门禁遍历 `tool_specs()`，把「漏声明」
//! 「目标键不在 schema」「下界 / 类型 / 枚举写错」「关键工具字段缩水」都变成测试失败。
//!
//! **只做入站兼容**：任何策略都不得让 schema 的 `properties` 多出键，别名不反写 schema。
//!
//! [`ToolSpec`]: super::ToolSpec

use serde_json::{Map, Value};

use super::tool_param_types::{
    AppendFileArgs, CodeNavArgs, CodeStatsArgs, CodebaseSemanticSearchArgs, CoverageReportArgs,
    CreateDirArgs, DeleteDirArgs, FileFromToOverwriteArgs, FileWriteArgs, GhApiArgs, GitArgs,
    GlobFilesArgs, ListTreeArgs, ModifyFileArgs, ReadBinaryMetaArgs, ReadDirEnhancedArgs,
    ReadFileArgs, RustAnalyzerArgs, SearchInFilesEnhancedArgs, SearchReplaceArgs, StructuredArgs,
    TableTextArgs,
};

/// 别名键与目标键的匹配方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasMatch {
    /// 精确键名（`src` 只认 `src`，不认 `Src`）。
    Exact,
    /// 键名大小写不敏感（`Path` / `PATH` 都归一到 `path`）。
    CaseInsensitive,
}

/// 值层面的归一方式。
#[derive(Debug, Clone, Copy)]
pub enum FieldKind {
    /// 只搬键，不碰值（`content` 的别名：值原样交给 schema 校验）。
    Keep,
    /// 路径串：数字 → 字符串、去首尾空白。
    PathString,
    /// 去首尾空白（`read_file.encoding`）。
    TrimmedStr,
    /// 整型：数值字符串 → 数字，且不得小于 `min`。
    U64Min(u64),
    /// 布尔：`true/1/yes`、`false/0/no`、数字 `0/1`。
    Bool,
    /// 枚举同义词：去空白、小写、空白转 `_` 后查表；**未命中时保留归一后的串**。
    EnumSynonyms(&'static [(&'static str, &'static str)]),
    /// 枚举同义词（**只认表内命中**）：未命中时原值不动。
    EnumSynonymsStrict(&'static [(&'static str, &'static str)]),
}

/// 一个字段的宽容化声明。
#[derive(Debug, Clone, Copy)]
pub struct FieldCoercion {
    /// 目标键（必须是该工具 schema 的 `properties` 键）。
    pub key: &'static str,
    /// 入站别名（**不进** schema）。
    pub aliases: &'static [&'static str],
    pub alias_match: AliasMatch,
    pub kind: FieldKind,
}

/// 跨键 / 整对象规则（字段级声明表达不了的少数几条）。
#[derive(Debug, Clone, Copy)]
pub struct StructuralRules {
    /// 旧平铺守卫键（`expect_content` 等）上抬到 `guard` 子对象。
    pub uplift_guard_keys: bool,
    /// 缺 `content`（或为 `null`）时补空串。
    pub default_empty_content: bool,
}

impl StructuralRules {
    /// 没有任何跨键规则（自建策略时用它填空位，不必逐个字段写 `false`）。
    pub const NONE: StructuralRules = StructuralRules {
        uplift_guard_keys: false,
        default_empty_content: false,
    };
}

/// 一个工具的完整宽容化策略。
#[derive(Debug, Clone, Copy)]
pub struct ArgsCoercionPolicy {
    pub fields: &'static [FieldCoercion],
    pub structural: StructuralRules,
    /// `Some` 时对数组字段 `edits` 的每个元素再跑一遍该策略（`modify_file` 专用）。
    pub nested_edits: Option<&'static ArgsCoercionPolicy>,
}

/// 不做任何宽容化（`builtin_tool!` 未写 `coerce:` 时的缺省）。
pub const NO_COERCION: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[],
    structural: StructuralRules::NONE,
    nested_edits: None,
};

// ── 引擎 ────────────────────────────────────────────────────────

/// 对已知内置工具的入参对象跑一遍策略；返回是否发生了改写。
pub fn coerce_args_value(policy: &ArgsCoercionPolicy, v: &mut Value) -> bool {
    let Value::Object(map) = v else {
        return false;
    };
    coerce_object(policy, map)
}

/// 单个 JSON 对象：先结构规则、再字段规则、最后嵌套数组。
///
/// 顺序与旧实现一致（旧 `coerce_modify_file_extra_fields` 也是先 `uplift_guard_keys`
/// 再逐字段），因为**先 coerce 再 validate** 的次序是既有契约，颠倒会改变错误文案。
fn coerce_object(policy: &ArgsCoercionPolicy, map: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    if policy.structural.uplift_guard_keys && uplift_guard_keys(map) {
        changed = true;
    }
    for field in policy.fields {
        if coerce_field(map, field) {
            changed = true;
        }
    }
    if policy.structural.default_empty_content && default_empty_content(map) {
        changed = true;
    }
    if coerce_nested_edits(policy, map) {
        changed = true;
    }
    changed
}

/// `modify_file.edits[i]`：数组元素逐个跑嵌套策略（拆出来是为了把 `coerce_object` 的
/// 圈复杂度压在棘轮线内，而不是把分支藏进别处）。
fn coerce_nested_edits(policy: &ArgsCoercionPolicy, map: &mut Map<String, Value>) -> bool {
    let Some(nested) = policy.nested_edits else {
        return false;
    };
    let Some(Value::Array(edits)) = map.get_mut("edits") else {
        return false;
    };
    let mut changed = false;
    for edit in edits.iter_mut() {
        let Value::Object(obj) = edit else {
            continue;
        };
        if coerce_object(nested, obj) {
            changed = true;
        }
    }
    changed
}

/// 单字段：先按别名搬键（仅当目标键缺失），再按 `kind` 归一键值。
fn coerce_field(map: &mut Map<String, Value>, field: &FieldCoercion) -> bool {
    let mut changed = false;
    if !map.contains_key(field.key) {
        let found = take_alias(map, field.key, field.alias_match).or_else(|| {
            field
                .aliases
                .iter()
                .find_map(|alias| take_alias(map, alias, field.alias_match))
        });
        if let Some(v) = found {
            map.insert(field.key.to_string(), v);
            changed = true;
        }
    }
    if let Some(val) = map.get_mut(field.key)
        && coerce_value(field.kind, val)
    {
        changed = true;
    }
    changed
}

fn take_alias(map: &mut Map<String, Value>, want: &str, m: AliasMatch) -> Option<Value> {
    if let Some(v) = map.remove(want) {
        return Some(v);
    }
    if m == AliasMatch::Exact {
        return None;
    }
    let found = map.keys().find(|k| k.eq_ignore_ascii_case(want)).cloned()?;
    map.remove(&found)
}

fn coerce_value(kind: FieldKind, val: &mut Value) -> bool {
    match kind {
        FieldKind::Keep => false,
        FieldKind::PathString => coerce_path_string_field(val),
        FieldKind::TrimmedStr => trim_string_in_place(val),
        FieldKind::U64Min(min) => coerce_u64_field_min(val, min),
        FieldKind::Bool => coerce_bool_field(val),
        FieldKind::EnumSynonyms(table) => coerce_enum_synonym(val, table, true),
        FieldKind::EnumSynonymsStrict(table) => coerce_enum_synonym(val, table, false),
    }
}

fn coerce_path_string_field(val: &mut Value) -> bool {
    match val {
        Value::Number(n) => {
            *val = Value::String(n.to_string());
            true
        }
        _ => trim_string_in_place(val),
    }
}

fn trim_string_in_place(val: &mut Value) -> bool {
    let Value::String(s) = val else {
        return false;
    };
    let t = s.trim();
    if t == s.as_str() {
        return false;
    }
    *val = Value::String(t.to_string());
    true
}

/// `14.0` 这类整数值的浮点也接受，`14.5` 不接受。
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

fn coerce_u64_field_min(val: &mut Value, min: u64) -> bool {
    match val {
        Value::String(s) => {
            let t = s.trim();
            if let Ok(n) = t.parse::<u64>()
                && n >= min
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
            if u < min {
                return false;
            }
            let new_v = Value::Number(u.into());
            if *val == new_v {
                return false;
            }
            *val = new_v;
            true
        }
        _ => false,
    }
}

fn coerce_bool_field(val: &mut Value) -> bool {
    match val {
        Value::String(s) => {
            let t = s.trim().to_ascii_lowercase();
            let b = match t.as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => return false,
            };
            *val = Value::Bool(b);
            true
        }
        Value::Number(n) => {
            let Some(u) = n.as_u64() else {
                return false;
            };
            if !matches!(u, 0 | 1) {
                return false;
            }
            *val = Value::Bool(u == 1);
            true
        }
        _ => false,
    }
}

/// 枚举值归一：去首尾空白、内部空白转 `_`、ASCII 小写。
fn normalize_enum_str(s: &str) -> String {
    s.trim()
        .chars()
        .map(|c| {
            if c.is_whitespace() {
                '_'
            } else {
                c.to_ascii_lowercase()
            }
        })
        .collect()
}

/// `fallback_to_normalized = true`：未命中表也写回归一后的串（旧顶层 `mode` 行为）；
/// `false`：只认表内命中（旧 `edits[i].mode` 行为）。
fn coerce_enum_synonym(
    val: &mut Value,
    table: &'static [(&'static str, &'static str)],
    fallback_to_normalized: bool,
) -> bool {
    let Value::String(s) = val else {
        return false;
    };
    let norm = normalize_enum_str(s);
    let mapped = table
        .iter()
        .find(|(from, _)| *from == norm)
        .map(|(_, to)| *to)
        .or(if fallback_to_normalized { Some(norm.as_str()) } else { None });
    let Some(target) = mapped else {
        return false;
    };
    if target == s.as_str() {
        return false;
    }
    *val = Value::String(target.to_string());
    true
}

/// 旧平铺守卫键上抬到 `guard` 子对象；`guard` 已有的同名子键优先，不覆盖显式值。
fn uplift_guard_keys(map: &mut Map<String, Value>) -> bool {
    const LEGACY: [(&str, &str); 4] = [
        ("content", "expect_content"),
        ("first_line", "expect_first_line"),
        ("last_line", "expect_last_line"),
        ("anchor_line", "expect_line_content"),
    ];
    let mut moved: Vec<(&str, Value)> = Vec::new();
    for (new, old) in LEGACY {
        if let Some(v) = map.remove(old) {
            moved.push((new, v));
        }
    }
    if moved.is_empty() {
        return false;
    }
    let slot = map
        .entry("guard".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !slot.is_object() {
        *slot = Value::Object(Map::new());
    }
    if let Value::Object(guard) = slot {
        for (key, val) in moved {
            guard.entry(key.to_string()).or_insert(val);
        }
    }
    true
}

fn default_empty_content(map: &mut Map<String, Value>) -> bool {
    if !matches!(map.get("content"), None | Some(Value::Null)) {
        return false;
    }
    map.insert("content".to_string(), Value::String(String::new()));
    true
}

// ── 声明：通用字段 ──────────────────────────────────────────────

/// `path` 的入站别名（模型最常见的写法错误）。**不写进** schema `properties`。
pub const PATH_ALIASES: &[&str] = &[
    "file_path",
    "filepath",
    "relative_path",
    "rel_path",
    "filename",
    "file_name",
    "file",
    "name",
    "output_path",
    "write_path",
    "target_file",
    "target_path",
    "target",
    "dest_path",
    "destination_path",
];

pub const PATH_FIELD: FieldCoercion = FieldCoercion {
    key: "path",
    aliases: PATH_ALIASES,
    alias_match: AliasMatch::CaseInsensitive,
    kind: FieldKind::PathString,
};

const CONTENT_ALIASES: &[&str] = &["file_content", "body", "text", "source", "code"];

const CONTENT_FIELD: FieldCoercion = FieldCoercion {
    key: "content",
    aliases: CONTENT_ALIASES,
    alias_match: AliasMatch::CaseInsensitive,
    kind: FieldKind::Keep,
};

const fn line_no(key: &'static str, min: u64) -> FieldCoercion {
    FieldCoercion {
        key,
        aliases: &[],
        alias_match: AliasMatch::Exact,
        kind: FieldKind::U64Min(min),
    }
}

const fn trimmed_str(key: &'static str) -> FieldCoercion {
    FieldCoercion {
        key,
        aliases: &[],
        alias_match: AliasMatch::Exact,
        kind: FieldKind::TrimmedStr,
    }
}

const fn bool_field(key: &'static str) -> FieldCoercion {
    FieldCoercion {
        key,
        aliases: &[],
        alias_match: AliasMatch::Exact,
        kind: FieldKind::Bool,
    }
}

const fn path_field(key: &'static str, aliases: &'static [&'static str]) -> FieldCoercion {
    FieldCoercion {
        key,
        aliases,
        alias_match: AliasMatch::Exact,
        kind: FieldKind::PathString,
    }
}

// ── 声明：各工具策略 ────────────────────────────────────────────

/// 只需要 `path` 别名/归一的工具（`glob_files`、`git`、`structured` …）。
pub const PATH_ONLY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[PATH_FIELD],
    structural: StructuralRules::NONE,
    nested_edits: None,
};

/// `read_file`：行号字符串、`encoding` 去空白、`count_total_lines` 布尔。
pub const READ_FILE_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[
        PATH_FIELD,
        line_no("start_line", 1),
        line_no("end_line", 1),
        line_no("max_lines", 1),
        line_no("anchor_line", 1),
        line_no("context_lines", 1),
        trimmed_str("encoding"),
        bool_field("count_total_lines"),
    ],
    structural: StructuralRules::NONE,
    nested_edits: None,
};

/// `create_file`：`path` + `content` 别名，且缺 `content` 时补空串
/// （`FileWriteArgs.content` 在 schema 里是 required，这条是既有契约）。
pub const CREATE_FILE_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[PATH_FIELD, CONTENT_FIELD],
    structural: StructuralRules {
        uplift_guard_keys: false,
        default_empty_content: true,
    },
    nested_edits: None,
};

/// `append_file`：`path` + `content` 别名（`content` 在 schema 里已有 `default: ""`）。
pub const APPEND_FILE_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[PATH_FIELD, CONTENT_FIELD],
    structural: StructuralRules::NONE,
    nested_edits: None,
};

/// `modify_file` 顶层：行号、`mode` 同义词、守卫键上抬、`edits` 逐项归一。
pub const MODIFY_FILE_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[
        PATH_FIELD,
        CONTENT_FIELD,
        line_no("start_line", 1),
        line_no("end_line", 1),
        line_no("after_line", 0),
        FieldCoercion {
            key: "mode",
            aliases: &[],
            alias_match: AliasMatch::Exact,
            kind: FieldKind::EnumSynonyms(MODE_SYNONYMS),
        },
    ],
    structural: StructuralRules {
        uplift_guard_keys: true,
        default_empty_content: false,
    },
    nested_edits: Some(&MODIFY_FILE_EDIT_POLICY),
};

/// `modify_file.edits[i]`：只允许 `replace_lines` / `insert_after_line`，
/// 且**只认表内命中**（`full` / `overwrite` 这类顶层同义词不在此生效）。
const MODIFY_FILE_EDIT_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[
        line_no("start_line", 1),
        line_no("end_line", 1),
        line_no("after_line", 0),
        FieldCoercion {
            key: "mode",
            aliases: &[],
            alias_match: AliasMatch::Exact,
            kind: FieldKind::EnumSynonymsStrict(EDIT_MODE_SYNONYMS),
        },
    ],
    structural: StructuralRules {
        uplift_guard_keys: true,
        default_empty_content: false,
    },
    nested_edits: None,
};

/// `modify_file.mode` 顶层同义词。未命中时写回归一后的串（旧行为），因此不必列恒等项。
const MODE_SYNONYMS: &[(&str, &str)] = &[
    ("lines", "replace_lines"),
    ("line", "replace_lines"),
    ("line_replace", "replace_lines"),
    ("replace", "replace_lines"),
    ("partial", "replace_lines"),
    ("replacelines", "replace_lines"),
    ("insert", "insert_after_line"),
    ("insert_line", "insert_after_line"),
    ("insert_after", "insert_after_line"),
    ("append_after_line", "insert_after_line"),
    ("overwrite", "full"),
    ("whole", "full"),
    ("whole_file", "full"),
];

/// `edits[i].mode` 只接受行区间/插入两种模式，故表里只留这两个目标（含恒等项，
/// 恒等项命中后与原文相同、不会写回）。
const EDIT_MODE_SYNONYMS: &[(&str, &str)] = &[
    ("lines", "replace_lines"),
    ("line", "replace_lines"),
    ("line_replace", "replace_lines"),
    ("replace", "replace_lines"),
    ("partial", "replace_lines"),
    ("replacelines", "replace_lines"),
    ("replace_lines", "replace_lines"),
    ("insert", "insert_after_line"),
    ("insert_line", "insert_after_line"),
    ("insert_after", "insert_after_line"),
    ("append_after_line", "insert_after_line"),
    ("insert_after_line", "insert_after_line"),
];

const FROM_ALIASES: &[&str] = &["src", "source", "source_path", "old_path", "from_path"];
const TO_ALIASES: &[&str] = &[
    "dst",
    "dest",
    "destination",
    "target",
    "target_path",
    "new_path",
    "to_path",
];

/// `copy_file` / `move_file`：`from` / `to` 的入站别名（精确键名，与旧行为一致）。
pub const FROM_TO_POLICY: ArgsCoercionPolicy = ArgsCoercionPolicy {
    fields: &[
        path_field("from", FROM_ALIASES),
        path_field("to", TO_ALIASES),
    ],
    structural: StructuralRules::NONE,
    nested_edits: None,
};

// ── 类型 ↔ 策略绑定 ────────────────────────────────────────────

/// 把宽容化策略**绑定到 Args 类型**上（#1381）。
///
/// spec 里只写 `coerce: auto`，`builtin_tool!` 把它展开成 `<Args as ArgsCoercion>::POLICY`，
/// 于是「哪个类型走哪套策略」由**类型自己**决定。
///
/// 这堵的是门禁抓不到的一类漂移：把 `PATH_ONLY` 误挂到 `read_file` 上时，门禁 1（`path`）
/// 依然成立、门禁 2–5 只检查「已声明的字段」，结果 `read_file` 的行号宽容化会**静默失效**。
/// 改成类型绑定后，这种错写在编译期就不成立。
///
/// **新增 impl 时必须在对应 spec 写 `coerce: auto`** —— 这一步没有编译期检查（Rust 没有
/// specialization，无法「有 impl 就自动启用」）。当前兜底只有门禁 1：该类型的工具若带
/// `path` / `content` / `from` / `to` 会失败；若将来给「只做行号归一」这类非通用键的类型加
/// 策略，记得同时补进 `permissive_tools_keep_their_documented_fields`。
pub trait ArgsCoercion {
    const POLICY: &'static ArgsCoercionPolicy;
}

/// 只做 `path` 归一的只读 / 文件工具：17 个 Args 类型共用同一套策略。
macro_rules! path_only_args {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ArgsCoercion for $ty {
                const POLICY: &'static ArgsCoercionPolicy = &PATH_ONLY;
            }
        )*
    };
}

path_only_args!(
    CodeNavArgs,
    CodeStatsArgs,
    CodebaseSemanticSearchArgs,
    CoverageReportArgs,
    CreateDirArgs,
    DeleteDirArgs,
    GhApiArgs,
    GitArgs,
    GlobFilesArgs,
    ListTreeArgs,
    ReadBinaryMetaArgs,
    ReadDirEnhancedArgs,
    RustAnalyzerArgs,
    SearchInFilesEnhancedArgs,
    SearchReplaceArgs,
    StructuredArgs,
    TableTextArgs,
);

impl ArgsCoercion for ReadFileArgs {
    const POLICY: &'static ArgsCoercionPolicy = &READ_FILE_POLICY;
}

impl ArgsCoercion for FileWriteArgs {
    const POLICY: &'static ArgsCoercionPolicy = &CREATE_FILE_POLICY;
}

impl ArgsCoercion for AppendFileArgs {
    const POLICY: &'static ArgsCoercionPolicy = &APPEND_FILE_POLICY;
}

impl ArgsCoercion for ModifyFileArgs {
    const POLICY: &'static ArgsCoercionPolicy = &MODIFY_FILE_POLICY;
}

impl ArgsCoercion for FileFromToOverwriteArgs {
    const POLICY: &'static ArgsCoercionPolicy = &FROM_TO_POLICY;
}

#[cfg(test)]
mod tests;
