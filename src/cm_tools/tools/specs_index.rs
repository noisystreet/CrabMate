//! 内建工具注册表的查找索引（`tool_specs` 表 → name 下标）。
//!
//! `#1530`：从 `tools/mod.rs` 拆出。

use super::ToolSpec;
use super::tool_specs_registry;

pub(super) fn tool_specs() -> &'static [ToolSpec] {
    tool_specs_registry::tool_specs()
}

/// name → tool_specs() 数组下标索引，首次访问时构建，O(1) 查找。
pub(super) fn tool_spec_index() -> &'static std::collections::HashMap<&'static str, usize> {
    use std::sync::LazyLock;
    static INDEX: LazyLock<std::collections::HashMap<&'static str, usize>> = LazyLock::new(|| {
        tool_specs()
            .iter()
            .enumerate()
            .map(|(i, s)| (s.name, i))
            .collect()
    });
    &INDEX
}

pub(super) fn find_spec(name: &str) -> Option<&'static ToolSpec> {
    tool_spec_index().get(name).map(|&idx| &tool_specs()[idx])
}

/// 工具名是否已在内建注册表中 —— 与执行期 `run_tool_dispatch` 的 `find_spec` 判据**同源**。
///
/// 供**加载期**校验复用（`cm_workflow` 的节点 `tool_name` 校验）。若那里另立一张白名单，
/// 工具改名时就有第二处需要人工同步；这里直接问「执行期会不会命中」。
///
/// `mcp__*` / `dyn__*` 是运行时注册的，不在本表内，由调用方按
/// `crate::cm_kernel::tool_origin` 自行放行。
///
/// 文档里不写成 intra-doc 链接：`run_tool_dispatch` 与 `cm_tools` 均为 `pub(crate)`，
/// 链接会触发 `rustdoc::private_intra_doc_links`（pre-commit 的 `cargo doc` 将其视为错误）。
pub fn is_registered_tool_name(name: &str) -> bool {
    find_spec(name).is_some()
}
