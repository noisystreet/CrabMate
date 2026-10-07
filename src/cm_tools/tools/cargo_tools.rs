//! Rust 开发工具：cargo check/test/clippy/run、工作区内 `rustc`。
#![allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 `run_tool_dispatch` 一致

use std::path::Path;
use std::process::Command;

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

const RUST_RUSTC_MAX_ARGS: usize = 64;
const RUST_RUSTC_MAX_ARG_BYTES: usize = 8192;

fn rustc_arg_is_safe(arg: &str) -> bool {
    let a = arg.trim();
    !a.contains("..") && !a.starts_with('/')
}

/// 在工作区根目录执行 **`rustc`**（不经 shell），参数须与 `run_command` 相同：**不得**含 `..` 或以 `/` 开头的参数。
/// 适用于 `rustc --explain E0xxx`、`rustc -vV`、`rustc --print=cfg` 等；**不要求**存在 `Cargo.toml`。
pub fn rust_rustc_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let args: Vec<String> = match v.get("args") {
        Some(serde_json::Value::Array(arr)) => arr
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect(),
        Some(_) => {
            return Err(ToolError::invalid_args(
                "错误：args 必须是字符串数组".to_string(),
            ));
        }
        None => Vec::new(),
    };
    if args.len() > RUST_RUSTC_MAX_ARGS {
        return Err(ToolError::invalid_args(format!(
            "错误：rustc 参数个数超过上限 {}",
            RUST_RUSTC_MAX_ARGS
        )));
    }
    for a in &args {
        if a.len() > RUST_RUSTC_MAX_ARG_BYTES {
            return Err(ToolError::invalid_args(format!(
                "错误：单参数长度超过 {} 字节",
                RUST_RUSTC_MAX_ARG_BYTES
            )));
        }
        if !rustc_arg_is_safe(a) {
            return Err(ToolError::invalid_args(
                "错误：参数不允许包含 \"..\" 或绝对路径（以 / 开头）".to_string(),
            ));
        }
    }

    let mut cmd = Command::new("rustc");
    cmd.args(&args).current_dir(workspace_root);
    run_and_format_try(cmd, max_output_len, "rustc", "rust_rustc", None)
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

pub fn rust_test_one_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: Option<&ToolContext<'_>>,
) -> Result<String, ToolError> {
    let v = crate::cm_tools::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let filter = match v.get("test_name").and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => {
            return Err(ToolError::invalid_args(
                "错误：缺少 test_name 参数".to_string(),
            ));
        }
    };
    let mut merged = v;
    if let Some(obj) = merged.as_object_mut() {
        obj.insert("test_filter".to_string(), serde_json::Value::String(filter));
    }
    let Some(c) = ctx else {
        return run_cargo_subcommand_value_try("test", &merged, workspace_root, max_output_len, None);
    };
    maybe_cache_cargo_test_try(&merged, workspace_root, c, || {
        run_cargo_subcommand_value_try(
            "test",
            &merged,
            workspace_root,
            max_output_len,
            Some(c.command_timeout_secs),
        )
    })
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
use cargo_subcommand::{
    run_and_format_try, run_cargo_subcommand_str_try, run_cargo_subcommand_value_try,
};

#[path = "cargo_tools_run_tests.rs"]
mod tests;
