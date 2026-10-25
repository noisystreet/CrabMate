//! `structured_patch` 实现目录（自单文件拆分以降低圈复杂度）。

use crate::cm_tools::tool_result::ToolError;

mod execute;

#[cfg(test)]
mod tests_revision;

#[allow(clippy::result_large_err)]
pub(super) fn structured_patch_execute_try(
    args_json: &str,
    working_dir: &std::path::Path,
    ctx: &crate::cm_tools::tools::ToolContext<'_>,
) -> Result<String, ToolError> {
    execute::run_structured_patch_try(args_json, working_dir, ctx)
}
