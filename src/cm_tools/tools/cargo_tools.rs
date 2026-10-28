//! Rust 开发工具：cargo check/test/clippy/run。
#![allow(clippy::result_large_err)] // `ToolError` 含正文解析快照（`parsed`），与 `run_tool_dispatch` 一致

use std::path::Path;

use crate::cm_tools::tool_result::ToolError;

use super::ToolContext;
use super::test_result_cache::{
    TestCacheKey, TestCacheKind, cargo_test_args_fingerprint, fingerprint_rust_workspace_sources,
    store_cached, try_get_cached, wrap_cache_hit,
};

const MAX_OUTPUT_LINES: usize = 800;

pub fn cargo_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_check_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_check_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    run_cargo_subcommand_str_try("check", args_json, workspace_root, max_output_len, None)
}

pub fn cargo_test(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: Option<&ToolContext<'_>>,
) -> String {
    cargo_test_try(args_json, workspace_root, max_output_len, ctx).unwrap_or_else(|e| e.message)
}

pub fn cargo_test_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: Option<&ToolContext<'_>>,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let Some(c) = ctx else {
        return run_cargo_subcommand_value_try("test", &v, workspace_root, max_output_len, None);
    };
    maybe_cache_cargo_test_try(&v, workspace_root, c, || {
        run_cargo_subcommand_value_try(
            "test",
            &v,
            workspace_root,
            max_output_len,
            Some(c.command_timeout_secs),
        )
    })
}

pub fn cargo_clippy(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_clippy_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_clippy_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    run_cargo_subcommand_str_try("clippy", args_json, workspace_root, max_output_len, None)
}

pub fn cargo_run_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    run_cargo_subcommand_str_try("run", args_json, workspace_root, max_output_len, None)
}

fn maybe_cache_cargo_test_try(
    v: &serde_json::Value,
    workspace_root: &Path,
    ctx: &ToolContext<'_>,
    run: impl FnOnce() -> Result<String, ToolError>,
) -> Result<String, ToolError> {
    if ctx.test_result_cache_enabled
        && let Some(inputs_fp) = fingerprint_rust_workspace_sources(workspace_root)
    {
        let args_fp = cargo_test_args_fingerprint(v);
        let root = workspace_root.to_path_buf();
        let key = TestCacheKey {
            workspace_root: root,
            kind: TestCacheKind::CargoTest,
            args_fingerprint: args_fp,
            inputs_fingerprint: inputs_fp.clone(),
        };
        if let Some(hit) = try_get_cached(
            ctx.test_result_cache_enabled,
            ctx.test_result_cache_max_entries,
            &key,
        ) {
            return Ok(wrap_cache_hit(&inputs_fp, &hit));
        }
        let out = run()?;
        store_cached(
            ctx.test_result_cache_enabled,
            ctx.test_result_cache_max_entries,
            key,
            out.clone(),
        );
        return Ok(out);
    }
    run()
}

#[path = "cargo_subcommand.rs"]
mod cargo_subcommand;
pub(crate) use cargo_subcommand::cargo_subcommand_background_argv;
use cargo_subcommand::{run_cargo_subcommand_str_try, run_cargo_subcommand_value_try};

#[path = "cargo_tools_run_tests.rs"]
mod tests;
