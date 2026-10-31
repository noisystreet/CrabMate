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
/// `summary` 覆盖注册表里**实际出现过**的三种形态：
///
/// - `dynamic` —— 要求 `args` 类型实现了 [`ToolArgsSummary`]，摘要逻辑与 Args 同侧；
/// - `static "…"` —— 与参数无关的固定文案（`cargo check` 这类只做转发的包装工具）；
/// - `none` —— 没有自定义摘要。
///
/// `runner` 只接受同步执行体函数路径（对应 `ToolRunner::Typed`）。`run_command` /
/// `terminal_session` 用的 `ToolRunner::AsyncOnly` 暂无对应写法：等它们那一批迁移时再补分支，
/// 不提前留没被匹配过的空壳（macro_rules 里未匹配到的分支连语法都不会被检查）。
macro_rules! builtin_tool {
    // ── 同步执行体 + 每工具一份的动态摘要 fn ──────────────────────────────────
    // 用于「多个工具**共用同一个** Args 类型、但摘要要带各自的动词」的情形
    // （当前只有 `copy_file` / `move_file` 共用 `FileFromToOverwriteArgs`）。
    // 仍然不重抄字段子集结构：`$fn` 与 Args 同侧定义在 `tool_summary_args`。
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:tt,
        summary: dynamic by $summary:path,
        description: $description:expr $(,)?
    ) => {
        $crate::cm_tools::tools::builtin_tool!(@spec
            name: $name,
            parameters: schema_of::<$args>,
            runner: $crate::cm_tools::tools::builtin_tool!(@runner $runner),
            category: $category,
            tags: [$($tag),*],
            summary: ToolSummaryKind::Dynamic($summary),
            description: $description,
        )
    };
    // ── 同步执行体 + 动态摘要（由 Args 派生）─────────────────────────────────
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:tt,
        summary: dynamic,
        description: $description:expr $(,)?
    ) => {
        $crate::cm_tools::tools::builtin_tool!(@spec
            name: $name,
            parameters: schema_of::<$args>,
            runner: $crate::cm_tools::tools::builtin_tool!(@runner $runner),
            category: $category,
            tags: [$($tag),*],
            summary: ToolSummaryKind::Dynamic(
                <$args as $crate::cm_tools::tools::tool_summary_args::ToolArgsSummary>::summary_line,
            ),
            description: $description,
        )
    };
    // ── 同步执行体 + 固定摘要 ────────────────────────────────────────────────
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:tt,
        summary: static $summary:expr,
        description: $description:expr $(,)?
    ) => {
        $crate::cm_tools::tools::builtin_tool!(@spec
            name: $name,
            parameters: schema_of::<$args>,
            runner: $crate::cm_tools::tools::builtin_tool!(@runner $runner),
            category: $category,
            tags: [$($tag),*],
            summary: ToolSummaryKind::Static($summary),
            description: $description,
        )
    };
    // ── 同步执行体 + 无摘要 ──────────────────────────────────────────────────
    (
        name: $name:expr,
        args: $args:ty,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        runner: $runner:tt,
        summary: none,
        description: $description:expr $(,)?
    ) => {
        $crate::cm_tools::tools::builtin_tool!(@spec
            name: $name,
            parameters: schema_of::<$args>,
            runner: $crate::cm_tools::tools::builtin_tool!(@runner $runner),
            category: $category,
            tags: [$($tag),*],
            summary: ToolSummaryKind::None,
            description: $description,
        )
    };
    // ── 执行体：同步 `Typed(fn)` 或仅异步 `AsyncOnly` ────────────────────────
    // 分派放这里，而不是给每种摘要形态再复制一套 `async_only` 分支：三种摘要 × 两种
    // 执行体是 6 条近乎相同的臂，改一处漏一处就变成四份要同步维护的字面量。
    (@runner async_only) => {
        ToolRunner::AsyncOnly
    };
    (@runner $runner:tt) => {
        ToolRunner::Typed($runner)
    };
    // ── 装配：各写法最终收敛到这一处 ─────────────────────────────────────────
    (
        @spec
        name: $name:expr,
        parameters: $parameters:expr,
        runner: $runner:expr,
        category: $category:ident,
        tags: [$($tag:expr),* $(,)?],
        summary: $summary:expr,
        description: $description:expr $(,)?
    ) => {
        ToolSpec {
            name: $name,
            description: $description,
            category: ToolCategory::$category,
            parameters: $parameters,
            runner: $runner,
            summary: $summary,
            tags: &[$($tag),*],
        }
    };
}

pub(crate) use builtin_tool;
