//! Rust 开发工具：cargo check/test/clippy/metadata/run/tree/clean/doc/outdated/machete/udeps/publish dry-run
#![allow(clippy::result_large_err)] // `ToolError` 含 legacy 解析快照，与 `run_tool_dispatch` 一致

use std::path::Path;
use std::process::Command;

use crate::cargo_metadata::cargo_metadata_command;
use crate::tool_result::ToolError;

use super::ToolContext;
use super::output_util;
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
    run_cargo_subcommand_str_try("check", args_json, workspace_root, max_output_len)
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
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let Some(c) = ctx else {
        return run_cargo_subcommand_value_try("test", &v, workspace_root, max_output_len);
    };
    maybe_cache_cargo_test_try(&v, workspace_root, c, || {
        run_cargo_subcommand_value_try("test", &v, workspace_root, max_output_len)
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
    run_cargo_subcommand_str_try("clippy", args_json, workspace_root, max_output_len)
}

pub fn cargo_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_run_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_run_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    run_cargo_subcommand_str_try("run", args_json, workspace_root, max_output_len)
}

pub fn rust_test_one(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: Option<&ToolContext<'_>>,
) -> String {
    rust_test_one_try(args_json, workspace_root, max_output_len, ctx).unwrap_or_else(|e| e.message)
}

pub fn rust_test_one_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
    ctx: Option<&ToolContext<'_>>,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
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
        return run_cargo_subcommand_value_try("test", &merged, workspace_root, max_output_len);
    };
    maybe_cache_cargo_test_try(&merged, workspace_root, c, || {
        run_cargo_subcommand_value_try("test", &merged, workspace_root, max_output_len)
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

pub fn cargo_metadata(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_metadata_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_metadata_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    let no_deps = v.get("no_deps").and_then(|x| x.as_bool()).unwrap_or(true);
    let format_version = v
        .get("format_version")
        .and_then(|x| x.as_u64())
        .unwrap_or(1);

    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }

    let cmd = cargo_metadata_command(workspace_root, no_deps, format_version);
    run_and_format_try(cmd, max_output_len, "cargo metadata", "cargo_metadata")
}

pub fn cargo_tree(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_tree_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_tree_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let invert = v.get("invert").and_then(|x| x.as_str()).map(str::trim);
    let depth = v.get("depth").and_then(|x| x.as_u64());
    let edges = v.get("edges").and_then(|x| x.as_str()).map(str::trim);

    let mut cmd = Command::new("cargo");
    cmd.arg("tree");
    if let Some(p) = package.filter(|s| !s.is_empty()) {
        cmd.arg("--package").arg(p);
    }
    if let Some(i) = invert.filter(|s| !s.is_empty()) {
        cmd.arg("--invert").arg(i);
    }
    if let Some(d) = depth {
        cmd.arg("--depth").arg(d.to_string());
    }
    if let Some(e) = edges.filter(|s| !s.is_empty()) {
        cmd.arg("--edges").arg(e);
    }
    cmd.current_dir(workspace_root);
    run_and_format_try(cmd, max_output_len, "cargo tree", "cargo_tree")
}

pub fn cargo_clean(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_clean_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_clean_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let release = v.get("release").and_then(|x| x.as_bool()).unwrap_or(false);
    let doc = v.get("doc").and_then(|x| x.as_bool()).unwrap_or(false);
    let dry_run = v.get("dry_run").and_then(|x| x.as_bool()).unwrap_or(true);

    let mut cmd = Command::new("cargo");
    cmd.arg("clean");
    if let Some(p) = package.filter(|s| !s.is_empty()) {
        cmd.arg("--package").arg(p);
    }
    if release {
        cmd.arg("--release");
    }
    if doc {
        cmd.arg("--doc");
    }
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.current_dir(workspace_root);
    run_and_format_try(cmd, max_output_len, "cargo clean", "cargo_clean")
}

pub fn cargo_doc(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_doc_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_doc_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let no_deps = v.get("no_deps").and_then(|x| x.as_bool()).unwrap_or(true);
    let open = v.get("open").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("doc");
    if let Some(p) = package.filter(|s| !s.is_empty()) {
        cmd.arg("--package").arg(p);
    }
    if no_deps {
        cmd.arg("--no-deps");
    }
    if open {
        cmd.arg("--open");
    }
    cmd.current_dir(workspace_root);
    run_and_format_try(cmd, max_output_len, "cargo doc", "cargo_doc")
}

pub fn cargo_nextest(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_nextest_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_nextest_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let test_filter = v
        .get("test_filter")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let profile = v
        .get("profile")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let no_capture = v
        .get("nocapture")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("nextest").arg("run");
    if let Some(p) = package.filter(|s| !s.is_empty()) {
        cmd.arg("--package").arg(p);
    }
    if let Some(p) = profile {
        cmd.arg("--profile").arg(p);
    }
    if let Some(f) = test_filter {
        cmd.arg(f);
    }
    if no_capture {
        cmd.arg("--").arg("--nocapture");
    }
    cmd.current_dir(workspace_root);
    let out = run_and_format_try(cmd, max_output_len, "cargo nextest run", "cargo_nextest")?;
    if out.contains("no such command: `nextest`") {
        return Err(ToolError::invalid_args(
            "cargo nextest: 未安装 cargo-nextest，请先运行 `cargo install cargo-nextest`"
                .to_string(),
        ));
    }
    Ok(out)
}

pub fn cargo_outdated(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_outdated_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_outdated_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let workspace = v
        .get("workspace")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let depth = v.get("depth").and_then(|x| x.as_u64());

    let mut cmd = Command::new("cargo");
    cmd.arg("outdated");
    if workspace {
        cmd.arg("--workspace");
    }
    if let Some(d) = depth {
        cmd.arg("--depth").arg(d.to_string());
    }
    cmd.current_dir(workspace_root);
    let out = run_and_format_try(cmd, max_output_len, "cargo outdated", "cargo_outdated")?;
    if out.contains("no such command: `outdated`") || out.contains("no such command: outdated") {
        return Err(ToolError::invalid_args(
            "cargo outdated: 未安装 cargo-outdated，请先运行 `cargo install cargo-outdated`"
                .to_string(),
        ));
    }
    Ok(out)
}

/// 运行 **cargo machete**（需已安装 `cargo-machete`）：启发式查找 **Cargo.toml 中声明但未在源码中引用** 的依赖；与 `cargo_outdated`（版本是否可升级）互补。误报可用 `with_metadata` 或 `package.metadata.cargo-machete` 缓解。
pub fn cargo_machete(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_machete_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_machete_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let with_metadata = v
        .get("with_metadata")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let path_rel = v.get("path").and_then(|x| x.as_str()).map(str::trim);

    if let Some(p) = path_rel
        && (p.is_empty() || p.contains(".."))
    {
        return Err(ToolError::invalid_args(
            "错误：path 无效（不可为空或含 ..）".to_string(),
        ));
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("machete");
    if with_metadata {
        cmd.arg("--with-metadata");
    }
    if let Some(p) = path_rel.filter(|s| !s.is_empty()) {
        let root_canon = workspace_root
            .canonicalize()
            .unwrap_or_else(|_| workspace_root.to_path_buf());
        let joined = workspace_root.join(p);
        match joined.canonicalize() {
            Ok(abs) => {
                if !abs.starts_with(&root_canon) {
                    return Err(ToolError::invalid_args(
                        "错误：path 必须位于工作区内".to_string(),
                    ));
                }
                cmd.arg(abs);
            }
            Err(e) => {
                return Err(ToolError::invalid_args(format!(
                    "错误：无法解析 path（{}）",
                    e
                )));
            }
        }
    }
    cmd.current_dir(workspace_root);
    let out = run_and_format_try(cmd, max_output_len, "cargo machete", "cargo_machete")?;
    if out.contains("no such command: `machete`") || out.contains("no such command: machete") {
        return Err(ToolError::invalid_args(
            "cargo machete: 未安装 cargo-machete，请先运行 `cargo install cargo-machete`"
                .to_string(),
        ));
    }
    Ok(out)
}

/// 运行 **cargo udeps**（需已安装 `cargo-udeps`）：基于构建信息查找未使用依赖，通常比 machete 更准但更重；**官方文档要求 nightly 工具链**，可用参数 `nightly: true` 调用 `cargo +nightly udeps`。
pub fn cargo_udeps(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_udeps_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_udeps_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }
    let nightly = v.get("nightly").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("cargo");
    if nightly {
        cmd.arg("+nightly");
    }
    cmd.arg("udeps");
    cmd.current_dir(workspace_root);
    let out = run_and_format_try(cmd, max_output_len, "cargo udeps", "cargo_udeps")?;
    if out.contains("no such command: `udeps`") || out.contains("no such command: udeps") {
        return Err(ToolError::invalid_args(
            "cargo udeps: 未安装 cargo-udeps，请先运行 `cargo install cargo-udeps`（运行期通常需 nightly，可传 nightly: true）"
                .to_string(),
        ));
    }
    Ok(out)
}

/// `cargo publish --dry-run`：仅验证打包与发布检查，**不会**上传 registry。
pub fn cargo_publish_dry_run(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    cargo_publish_dry_run_try(args_json, workspace_root, max_output_len)
        .unwrap_or_else(|e| e.message)
}

pub fn cargo_publish_dry_run_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }

    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let allow_dirty = v
        .get("allow_dirty")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let no_verify = v
        .get("no_verify")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let features = v
        .get("features")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let all_features = v
        .get("all_features")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("publish").arg("--dry-run");
    if let Some(p) = package {
        cmd.arg("--package").arg(p);
    }
    if allow_dirty {
        cmd.arg("--allow-dirty");
    }
    if no_verify {
        cmd.arg("--no-verify");
    }
    if let Some(f) = features {
        cmd.arg("--features").arg(f);
    }
    if all_features {
        cmd.arg("--all-features");
    }
    cmd.current_dir(workspace_root);
    run_and_format_try(
        cmd,
        max_output_len,
        "cargo publish --dry-run",
        "cargo_publish_dry_run",
    )
}

pub fn cargo_fix(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    cargo_fix_try(args_json, workspace_root, max_output_len).unwrap_or_else(|e| e.message)
}

pub fn cargo_fix_try(
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;

    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }

    let confirm = v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false);
    if !confirm {
        return Err(ToolError::invalid_args(
            "拒绝执行：cargo_fix 需要 confirm=true 才会真正应用修复（避免误改代码）。".to_string(),
        ));
    }

    let broken_code = v
        .get("broken_code")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let all_targets = v
        .get("all_targets")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let package = v
        .get("package")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let features = v
        .get("features")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let all_features = v
        .get("all_features")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let edition = v
        .get("edition")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let edition_idioms = v
        .get("edition_idioms")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let allow_dirty = v
        .get("allow_dirty")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let allow_staged = v
        .get("allow_staged")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let allow_no_vcs = v
        .get("allow_no_vcs")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("fix");

    if broken_code {
        cmd.arg("--broken-code");
    }
    if all_targets {
        cmd.arg("--all-targets");
    }
    if let Some(p) = package {
        cmd.arg("--package").arg(p);
    }
    if let Some(f) = features {
        cmd.arg("--features").arg(f);
    }
    if all_features {
        cmd.arg("--all-features");
    }
    if let Some(e) = edition {
        cmd.arg("--edition").arg(e);
    }
    if edition_idioms {
        cmd.arg("--edition-idioms");
    }
    if allow_dirty {
        cmd.arg("--allow-dirty");
    }
    if allow_staged {
        cmd.arg("--allow-staged");
    }
    if allow_no_vcs {
        cmd.arg("--allow-no-vcs");
    }

    cmd.current_dir(workspace_root);
    run_and_format_try(cmd, max_output_len, "cargo fix", "cargo_fix")
}

fn run_cargo_subcommand_str_try(
    subcmd: &str,
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    let v = crate::tools::parse_args_json(args_json).map_err(ToolError::invalid_args)?;
    run_cargo_subcommand_value_try(subcmd, &v, workspace_root, max_output_len)
}

fn run_cargo_subcommand_value_try(
    subcmd: &str,
    v: &serde_json::Value,
    workspace_root: &Path,
    max_output_len: usize,
) -> Result<String, ToolError> {
    if !workspace_root.join("Cargo.toml").is_file() {
        return Err(ToolError::workspace(
            "workspace_no_cargo_toml",
            "错误：当前工作目录未找到 Cargo.toml".to_string(),
        ));
    }

    let release = v.get("release").and_then(|x| x.as_bool()).unwrap_or(false);
    let all_targets = v
        .get("all_targets")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim);
    let bin = v.get("bin").and_then(|x| x.as_str()).map(str::trim);
    let features = v
        .get("features")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let test_filter = v
        .get("test_filter")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty());
    let no_capture = v
        .get("nocapture")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let run_args = v
        .get("args")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(p) = package
        && (p.is_empty() || p.contains(char::is_whitespace))
    {
        return Err(ToolError::invalid_args(
            "错误：package 参数无效".to_string(),
        ));
    }
    if let Some(b) = bin
        && (b.is_empty() || b.contains(char::is_whitespace))
    {
        return Err(ToolError::invalid_args("错误：bin 参数无效".to_string()));
    }

    let mut cmd = Command::new("cargo");
    cmd.arg(subcmd);
    if release {
        cmd.arg("--release");
    }
    if all_targets && matches!(subcmd, "check" | "clippy") {
        cmd.arg("--all-targets");
    }
    if let Some(p) = package {
        cmd.arg("--package").arg(p);
    }
    if let Some(b) = bin {
        cmd.arg("--bin").arg(b);
    }
    if let Some(f) = features {
        cmd.arg("--features").arg(f);
    }
    if subcmd == "test" {
        if let Some(filter) = test_filter {
            cmd.arg(filter);
        }
        if no_capture {
            cmd.arg("--").arg("--nocapture");
        }
    } else if subcmd == "run" && !run_args.is_empty() {
        cmd.arg("--");
        for a in run_args {
            if let Some(s) = a.as_str() {
                cmd.arg(s);
            }
        }
    }
    cmd.current_dir(workspace_root);
    let tool_code = format!("cargo_{}", subcmd);
    run_and_format_try(
        cmd,
        max_output_len,
        &format!("cargo {}", subcmd),
        &tool_code,
    )
}

fn run_and_format_try(
    mut cmd: Command,
    max_output_len: usize,
    title: &str,
    tool_code: &str,
) -> Result<String, ToolError> {
    match cmd.output() {
        Ok(output) => {
            let exit = output.status.code().unwrap_or(-1);
            let body = output_util::merge_process_output(
                &output,
                output_util::ProcessOutputMerge::ConcatStdoutStderr,
            );
            let message = output_util::format_exited_command_output(
                title,
                exit,
                &body,
                max_output_len,
                MAX_OUTPUT_LINES,
            );
            if output.status.success() {
                Ok(message)
            } else {
                Err(ToolError::cargo_subcommand_failed(tool_code, exit, message))
            }
        }
        Err(e) => Err(ToolError::subprocess_spawn_error(title, e)),
    }
}
