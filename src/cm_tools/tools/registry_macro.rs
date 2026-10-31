//! #1157：内建工具注册面的**单点声明**宏。
//!
//! 背景见 issue #1157：新增一个内建工具原本要动约 9 处（`cm_names`、`specs`、
//! `tool_param_types`、`tool_summary_args`、`tool_summary`、`runners`、`dev_tag`、
//! `tool_write_effect`、`registry_policy`），其中 `ToolSpec` 的 `name` / `parameters` /
//! `runner` / `summary` 与 `tool_summary_args` 的字段子集结构全是**同一件事的重复抄写**。
//!
//! 目标形态是「写一次 Args，spec 与摘要由宏派生」：Args 结构（`tool_param_types`）
//! 已经是 schema 与 runner 反序列化的共同真源，所以
//!
//! - `parameters: schema_of::<Args>` —— 由宏从 `args:` 生成，不再手写；
//! - `summary: dynamic` —— 由宏挂 `<Args as ToolArgsSummary>::summary_line`，
//!   摘要逻辑写在 Args 同侧，不再单独维护一份字段子集结构；
//! - `name` —— 仍取 `crate::cm_names::*` 常量（#1155 的名字单一真源），宏不造字面量。
//!
//! 手写 `ToolSpec { .. }` 与宏写法**可以并存**：迁移是分批的（先 2–3 个验证形态，
//! 再按 `specs/*.inc.rs` 分片推进），并存期由 `registry_golden_tests` 保证
//! 「schema / 描述 / 摘要文本」零漂移。

/// 声明一条内建工具的注册项，用在 `tool_specs_registry/specs/*.inc.rs` 的数组字面量里。
///
/// `summary` 只接受 `dynamic` / `none` 两种：`dynamic` 要求 `args` 类型实现了
/// [`ToolArgsSummary`]，`none` 用于没有自定义摘要的工具。`static` 摘要目前没有
/// 使用者，等真出现再加分支（不留「以后可能用」的空壳）。
macro_rules! builtin_tool {
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:expr,
        summary: dynamic,
        description: $description:expr $(,)?
    ) => {
        ToolSpec {
            name: $name,
            description: $description,
            category: ToolCategory::$category,
            parameters: schema_of::<$args>,
            runner: ToolRunner::Typed($runner),
            summary: ToolSummaryKind::Dynamic(
                <$args as $crate::cm_tools::tools::tool_summary_args::ToolArgsSummary>::summary_line,
            ),
            tags: &[$($tag),*],
        }
    };
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:expr,
        summary: none,
        description: $description:expr $(,)?
    ) => {
        ToolSpec {
            name: $name,
            description: $description,
            category: ToolCategory::$category,
            parameters: schema_of::<$args>,
            runner: ToolRunner::Typed($runner),
            summary: ToolSummaryKind::None,
            tags: &[$($tag),*],
        }
    };
}

pub(crate) use builtin_tool;
