//! 安全相关工具：cargo audit / cargo deny

use std::path::Path;
use std::process::Command;

use super::output_util;

const MAX_OUTPUT_LINES: usize = 800;

pub fn cargo_audit(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }
    let deny_warnings = v
        .get("deny_warnings")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);
    let json = v.get("json").and_then(|x| x.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("audit");
    if deny_warnings {
        cmd.arg("--deny").arg("warnings");
    }
    if json {
        cmd.arg("--json");
    }
    cmd.current_dir(workspace_root);
    run_and_format(cmd, max_output_len, "cargo audit")
}

pub fn cargo_deny(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    if !workspace_root.join("Cargo.toml").is_file() {
        return "错误：当前工作目录未找到 Cargo.toml".to_string();
    }
    let checks = v
        .get("checks")
        .and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("advisories licenses bans sources");
    let all_features = v
        .get("all_features")
        .and_then(|x| x.as_bool())
        .unwrap_or(false);

    let mut cmd = Command::new("cargo");
    cmd.arg("deny").arg("check");
    for c in checks.split_whitespace() {
        cmd.arg(c);
    }
    if all_features {
        cmd.arg("--all-features");
    }
    cmd.current_dir(workspace_root);
    let out = run_and_format(cmd, max_output_len, "cargo deny check");
    if out.contains("no such command: `deny`") {
        return "cargo deny: 未安装 cargo-deny，请先运行 `cargo install cargo-deny`".to_string();
    }
    out
}

fn run_and_format(mut cmd: Command, max_output_len: usize, title: &str) -> String {
    match cmd.output() {
        Ok(output) => {
            let status = output.status.code().unwrap_or(-1);
            let body = output_util::merge_process_output(
                &output,
                output_util::ProcessOutputMerge::ConcatStdoutStderr,
            );
            if status != 0 && body.contains("no such command: `audit`") {
                return "cargo audit: 未安装 cargo-audit，请先运行 `cargo install cargo-audit`"
                    .to_string();
            }
            output_util::format_exited_command_output(
                title,
                status,
                &body,
                max_output_len,
                MAX_OUTPUT_LINES,
            )
        }
        Err(e) => output_util::format_spawn_error(
            title,
            &e,
            output_util::CommandSpawnErrorStyle::ExecuteFailed,
        ),
    }
}
