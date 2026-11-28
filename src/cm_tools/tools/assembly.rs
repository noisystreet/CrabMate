//! 工具列表装配：分类 / dev tag 过滤、注册表投影、调用摘要。
//!
//! `#1530`：从 `tools/mod.rs` 拆出。

use crate::cm_types::{FunctionDef, Tool};

use super::dev_tag;
use super::specs_index::{find_spec, tool_specs};
use super::{ToolCategory, ToolSpec, ToolSummaryKind, tool_spec_requires_fastembed};

/// 构建工具列表时的分类与开发子域标签过滤。
#[derive(Clone, Copy, Default)]
pub struct ToolsBuildOptions<'a> {
    /// `None` 或 `Some(&[])`：不按顶层分类过滤（`Basic` 与 `Development` 均保留）。
    pub categories: Option<&'a [ToolCategory]>,
    /// `None` 或 `Some(&[])`：不按标签过滤。`Some(non-empty)`：仅保留 **Development** 工具中
    /// [`dev_tag::tags_for_tool_name`] 与列表 **有交集** 者；`Basic` 仍只受 `categories` 约束。
    pub dev_tags: Option<&'a [&'a str]>,
}

fn tool_passes_filters(spec: &ToolSpec, opts: ToolsBuildOptions<'_>) -> bool {
    if !cfg!(feature = "fastembed") && tool_spec_requires_fastembed(spec.name) {
        return false;
    }
    let cats = opts.categories.unwrap_or(&[]);
    if !cats.is_empty() && !cats.contains(&spec.category) {
        return false;
    }
    let Some(wanted) = opts.dev_tags.and_then(|t| (!t.is_empty()).then_some(t)) else {
        return true;
    };
    if spec.category != ToolCategory::Development {
        return true;
    }
    dev_tag::tags_for_tool_name(spec.name)
        .iter()
        .any(|tag| wanted.contains(tag))
}

/// 构建传给 API 的工具列表（表驱动注册）。
pub fn build_tools() -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions::default())
}

/// 构建传给 API 的工具列表：可按顶层分类过滤（`ToolCategory::Basic` / `ToolCategory::Development`）。
pub fn build_tools_filtered(allowed: Option<&[ToolCategory]>) -> Vec<Tool> {
    build_tools_with_options(ToolsBuildOptions {
        categories: allowed,
        dev_tags: None,
    })
}

/// 同时支持顶层分类与 Development 子域标签过滤（见 [`ToolsBuildOptions`]）。
pub fn build_tools_with_options(opts: ToolsBuildOptions<'_>) -> Vec<Tool> {
    tool_specs()
        .iter()
        .filter(|s| tool_passes_filters(s, opts))
        .map(|s| Tool {
            typ: TOOL_TYPE_FUNCTION.to_string(),
            function: FunctionDef {
                name: s.name.to_string(),
                description: s.description.to_string(),
                parameters: cached_params(s),
            },
        })
        .collect()
}

const TOOL_TYPE_FUNCTION: &str = "function";

fn cached_params(spec: &ToolSpec) -> serde_json::Value {
    use std::sync::LazyLock;
    static CACHE: LazyLock<std::collections::HashMap<&'static str, serde_json::Value>> =
        LazyLock::new(|| {
            tool_specs()
                .iter()
                .map(|s| (s.name, (s.parameters)()))
                .collect()
        });
    CACHE
        .get(spec.name)
        .cloned()
        .unwrap_or_else(|| (spec.parameters)())
}

/// 内置工具 `name` 的 parameters JSON Schema（供工作流等静态校验复用）。
pub fn cached_params_for_tool_name(name: &str) -> Option<serde_json::Value> {
    find_spec(name).map(cached_params)
}

/// 编译期注册表项的只读投影（`GET /tools` 内置源）。
///
/// 暴露**全部**注册项（含 `fastembed` 特性门剔除者），是否「在场」由调用方按
/// `tool_passes_filters` 语义自行判定，便于报告 `absent_reason`。
pub struct BuiltinToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    pub category: ToolCategory,
    /// `Development` 类工具的子域标签；`Basic` 类为空。
    pub dev_tags: &'static [&'static str],
    /// 是否需要 `fastembed` Cargo feature（当前构建未启用则不在场）。
    pub requires_fastembed: bool,
}

/// 枚举内置工具注册表（进程级 `&'static` 表，一次构建）。
pub fn builtin_tool_descriptors() -> Vec<BuiltinToolDescriptor> {
    tool_specs()
        .iter()
        .map(|s| BuiltinToolDescriptor {
            name: s.name,
            description: s.description,
            category: s.category,
            dev_tags: match s.category {
                ToolCategory::Development => dev_tag::tags_for_tool_name(s.name),
                ToolCategory::Basic => &[],
            },
            requires_fastembed: tool_spec_requires_fastembed(s.name),
        })
        .collect()
}

/// 为前端生成简短的工具调用摘要，便于在 Chat 面板中展示
pub fn summarize_tool_call(name: &str, args_json: &str) -> Option<String> {
    let spec = find_spec(name)?;
    match &spec.summary {
        ToolSummaryKind::None => None,
        ToolSummaryKind::Static(s) => Some((*s).to_string()),
        ToolSummaryKind::Dynamic(f) => {
            let v: serde_json::Value = serde_json::from_str(args_json).ok()?;
            f(&v)
        }
    }
}

/// 使用预解析的 `serde_json::Value` 生成摘要，避免重复 JSON 解析。
pub fn summarize_tool_call_parsed(name: &str, args_parsed: &serde_json::Value) -> Option<String> {
    let spec = find_spec(name)?;
    match &spec.summary {
        ToolSummaryKind::None => None,
        ToolSummaryKind::Static(s) => Some((*s).to_string()),
        ToolSummaryKind::Dynamic(f) => f(args_parsed),
    }
}
