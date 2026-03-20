//! Rust 开发工具：cargo check/test/clippy/metadata/run/tree/clean/doc

use std::path::Path;
use std::process::Command;

const MAX_OUTPUT_LINES: usize = 800;

pub fn cargo_check(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    run_cargo_subcommand("check", args_json, workspace_root, max_output_len)
}

pub fn cargo_test(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    run_cargo_subcommand("test", args_json, workspace_root, max_output_len)
}

pub fn cargo_clippy(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    run_cargo_subcommand("clippy", args_json, workspace_root, max_output_len)
}

pub fn cargo_run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    run_cargo_subcommand("run", args_json, workspace_root, max_output_len)
}

pub fn rust_test_one(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let filter = match v.get("test_name").and_then(|x| x.as_str()).map(str::trim) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return "错误：缺少 test_name 参数".to_string(),
    };
    let mut merged = v;
    if let Some(obj) = merged.as_object_mut() {
        obj.insert(
            "test_filter".to_string(),
            serde_json::Value::String(filter),
        );
    }
    run_cargo_subcommand(
        "test",
        &serde_json::to_string(&merged).unwrap_or_else(|_| "{}".to_string()),
        workspace_root,
        max_output_len,
    )
}

pub fn cargo_metadata(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    let no_deps = v.get("no_deps").and_then(|x| x.as_bool()).unwrap_or(true);
    let format_version = v
        .get("format_version")
        .and_then(|x| x.as_u64())
        .unwrap_or(1);

    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("metadata")
        .arg(format!("--format-version={}", format_version));
    if no_deps {
        cmd.arg("--no-deps");
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "cargo metadata")
}

pub fn cargo_tree(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
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
    run_and_format(cmd, max_output_len, "cargo tree")
}

pub fn cargo_clean(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
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
    run_and_format(cmd, max_output_len, "cargo clean")
}

pub fn cargo_doc(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
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
    run_and_format(cmd, max_output_len, "cargo doc")
}

pub fn cargo_nextest(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
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
    let no_capture = v.get("nocapture").and_then(|x| x.as_bool()).unwrap_or(false);

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
    let out = run_and_format(cmd, max_output_len, "cargo nextest run");
    if out.contains("no such command: `nextest`") {
        return "cargo nextest: 未安装 cargo-nextest，请先运行 `cargo install cargo-nextest`".to_string();
    }
    out
}

pub fn cargo_outdated(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }
    let workspace = v.get("workspace").and_then(|x| x.as_bool()).unwrap_or(false);
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
    let out = run_and_format(cmd, max_output_len, "cargo outdated");
    if out.contains("no such command: `outdated`") || out.contains("no such command: outdated") {
        return "cargo outdated: 未安装 cargo-outdated，请先运行 `cargo install cargo-outdated`".to_string();
    }
    out
}

pub fn cargo_fix(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };

    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }

    let confirm = v.get("confirm").and_then(|x| x.as_bool()).unwrap_or(false);
    if !confirm {
        return "拒绝执行：cargo_fix 需要 confirm=true 才会真正应用修复（避免误改代码）。".to_string();
    }

    let broken_code = v.get("broken_code").and_then(|x| x.as_bool()).unwrap_or(false);
    let all_targets = v.get("all_targets").and_then(|x| x.as_bool()).unwrap_or(false);
    let package = v.get("package").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let features = v.get("features").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let all_features = v.get("all_features").and_then(|x| x.as_bool()).unwrap_or(false);
    let edition = v.get("edition").and_then(|x| x.as_str()).map(str::trim).filter(|s| !s.is_empty());
    let edition_idioms = v.get("edition_idioms").and_then(|x| x.as_bool()).unwrap_or(false);
    let allow_dirty = v.get("allow_dirty").and_then(|x| x.as_bool()).unwrap_or(false);
    let allow_staged = v.get("allow_staged").and_then(|x| x.as_bool()).unwrap_or(false);
    let allow_no_vcs = v.get("allow_no_vcs").and_then(|x| x.as_bool()).unwrap_or(false);

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
    run_and_format(cmd, max_output_len, "cargo fix")
}

fn run_cargo_subcommand(
    subcmd: &str,
    args_json: &str,
    workspace_root: &Path,
    max_output_len: usize,
) -> String {
    let v: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误：{}", e),
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
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
    let no_capture = v.get("nocapture").and_then(|x| x.as_bool()).unwrap_or(false);
    let run_args = v
        .get("args")
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default();

    if let Some(p) = package {
        if p.is_empty() || p.contains(char::is_whitespace) {
            return "错误：package 参数无效".to_string();
        }
    }
    if let Some(b) = bin {
        if b.is_empty() || b.contains(char::is_whitespace) {
            return "错误：bin 参数无效".to_string();
        }
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
    } else if subcmd == "run" {
        if !run_args.is_empty() {
            cmd.arg("--");
            for a in run_args {
                if let Some(s) = a.as_str() {
                    cmd.arg(s);
                }
            }
        }
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, &format!("cargo {}", subcmd))
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            let mut body = String::new();
            if !stdout.trim().is_empty() {
                body.push_str(stdout.trim_end());
            }
            if !stderr.trim().is_empty() {
                if !body.is_empty() {
                    body.push('\n');
                }
                body.push_str(stderr.trim_end());
            }
            if body.is_empty() {
                body = "(无输出)".to_string();
            }
            format!(
                "{} (exit={}):\n{}",
                title,
                status,
                truncate_output(&body, max_output_len)
            )
        }
        Err(e) => format!("{}: 执行失败（{}）", title, e),
    }
}

fn truncate_output(s: &str, max_bytes: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if s.len() <= max_bytes && lines.len() <= MAX_OUTPUT_LINES {
        return s.to_string();
    }
    let kept_lines = lines.len().min(MAX_OUTPUT_LINES);
    let joined = lines[..kept_lines].join("\n");
    let truncated = if joined.len() <= max_bytes {
        joined
    } else {
        joined[..max_bytes].to_string()
    };
    format!(
        "{}\n\n... (输出已截断，保留前 {} 行，共 {} 行)",
        truncated,
        kept_lines,
        lines.len()
    )
}

