//! `structured_patch` 实现目录（自单文件拆分以降低圈复杂度）。

mod execute;

pub(super) fn structured_patch_execute(
    args_json: &str,
    working_dir: &std::path::Path,
    ctx: &crate::tools::ToolContext<'_>,
) -> String {
    execute::run_structured_patch_display(args_json, working_dir, ctx)
}
