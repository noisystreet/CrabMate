//! 加载期校验节点 `tool_name` 是否已注册（拼错 / 工具改名不再等到执行该节点才暴露）。
//!
//! 背景：工作流的前序节点可能已有副作用，而 `tool_name` 是裸字符串，拼错时只有执行到
//! 该节点才报 `unknown_tool`，此时文件可能已被改了一半。加载期拦住这类错误是本模块的
//! 唯一职责（不校验 `tool_args` 是否符合该工具的 JSON Schema）。

use super::model::WorkflowSpec;
use crate::cm_tools::tool_naming::{ToolOrigin, tool_origin};
use crate::cm_tools::tools::is_registered_tool_name;

/// 校验 `nodes` 与 `for_each_pending` 的每个 `tool_name`；任一未注册即返回错误。
///
/// 放行条件：内建注册表命中（[`is_registered_tool_name`]），**或** `tool_origin` 非
/// `Builtin`（`mcp__*` / `dyn__*`）。后两者是运行时注册的，静态不可枚举，硬校验会误伤；
/// 它们能否真正执行仍由执行期判定——当前 workflow 节点走内建 dispatch，`mcp__*` 执行到
/// 该节点依旧会 `unknown_tool`，本函数只保证「不提前拦」。
///
/// `for_each_pending` 必须一起看：`for_each` 的 `tool_name` 不在 `spec.nodes` 里，
/// 只有运行时才展开成节点，漏掉它等于给 `for_each` 留后门。
pub fn validate_tool_names(spec: &WorkflowSpec) -> Result<(), String> {
    for node in spec.nodes.iter() {
        check_tool_name(&node.id, &node.tool_name)?;
    }
    for pending in spec.for_each_pending.iter() {
        let label = format!("{}（for_each）", pending.base_id);
        check_tool_name(&label, &pending.tool_name)?;
    }
    Ok(())
}

fn check_tool_name(node_id: &str, tool_name: &str) -> Result<(), String> {
    if is_registered_tool_name(tool_name) || tool_origin(tool_name) != ToolOrigin::Builtin {
        return Ok(());
    }
    Err(format!(
        "节点 {node_id} 的 tool_name「{tool_name}」未注册：请检查拼写；\
         内建工具名见 docs/工具说明.md，MCP 代理工具须以 `mcp__` 开头"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_builtin_name_passes() {
        assert!(check_tool_name("a", "read_file").is_ok());
    }

    #[test]
    fn typo_is_rejected_with_node_id_and_name() {
        let err = check_tool_name("verify", "cargo_tst").unwrap_err();
        assert!(err.contains("verify"), "got {err}");
        assert!(err.contains("cargo_tst"), "got {err}");
    }

    #[test]
    fn external_prefixes_are_exempt() {
        // 运行时注册的工具：不在内建表里，但硬校验会误伤。
        assert!(check_tool_name("a", "mcp__fanalyzer__list").is_ok());
        assert!(check_tool_name("a", "dyn__deploy").is_ok());
    }

    #[test]
    fn prefix_must_be_at_start() {
        // 与 `tool_origin` 同语义：只在开头匹配，不做包含匹配。
        assert!(check_tool_name("a", "read_mcp__x").is_err());
    }
}
