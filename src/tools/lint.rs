use super::cargo_tools;
use super::frontend_tools;
use super::python_tools;
use super::tool_param_types::RunLintsArgs;
use std::path::Path;

/// 运行 cargo clippy、（可选）frontend 的 npm lint、（可选）`ruff check`，将结果聚合为一段文本。
///
/// 参数 JSON:
/// {
///   "run_cargo": bool,           // 可选，默认 true
///   "run_cargo_check": bool,     // 可选，默认 true；为 true 且 run_cargo 时先 `cargo check --all-targets` 再 clippy
///   "run_frontend": bool,        // 可选，默认 true（npm run lint；未指定 subdir 时按 frontend / frontend-leptos 启发式选含 package.json 的目录）
///   "run_frontend_build": bool,  // 可选，默认 false（npm run build）
///   "run_python_ruff": bool      // 可选，默认 true（无 Python 项目标记时跳过）
/// }
pub fn run(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: RunLintsArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 run_lints 形状不一致: {e}"),
    };
    let run_cargo = args.run_cargo;
    let run_cargo_check = args.run_cargo_check;
    let run_frontend = args.run_frontend;
    let run_frontend_build = args.run_frontend_build;
    let run_python_ruff = args.run_python_ruff;

    let mut sections = Vec::new();

    if run_cargo {
        if run_cargo_check {
            sections.push(run_cargo_check_only(workspace_root, max_output_len));
        }
        sections.push(run_cargo_clippy(workspace_root, max_output_len));
    }
    if run_frontend {
        sections.push(run_frontend_lint(workspace_root, max_output_len));
    }
    if run_frontend_build {
        sections.push(run_frontend_build_only(workspace_root, max_output_len));
    }
    if run_python_ruff {
        sections.push(run_python_ruff_check(workspace_root, max_output_len));
    }

    sections
        .join("\n\n====================\n\n")
        .trim()
        .to_string()
}

fn run_cargo_check_only(root: &Path, max_output_len: usize) -> String {
    if !root.join("Cargo.toml").is_file() {
        return "cargo check: 跳过（未找到 Cargo.toml）".to_string();
    }
    cargo_tools::cargo_check(r#"{"all_targets":true}"#, root, max_output_len)
}

fn run_cargo_clippy(root: &Path, max_output_len: usize) -> String {
    if !root.join("Cargo.toml").is_file() {
        return "cargo clippy: 跳过（未找到 Cargo.toml）".to_string();
    }
    cargo_tools::cargo_clippy(r#"{"all_targets":true}"#, root, max_output_len)
}

fn run_frontend_lint(root: &Path, max_output_len: usize) -> String {
    frontend_tools::frontend_lint(r#"{"script":"lint"}"#, root, max_output_len)
}

fn run_frontend_build_only(root: &Path, max_output_len: usize) -> String {
    frontend_tools::frontend_build(r#"{"script":"build"}"#, root, max_output_len)
}

fn run_python_ruff_check(root: &Path, max_output_len: usize) -> String {
    if !python_tools::workspace_has_python_project(root) {
        return "ruff check: 跳过（未找到 Python 项目标记文件）".to_string();
    }
    python_tools::ruff_check("{}", root, max_output_len)
}
