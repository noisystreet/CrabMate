//! `dependency_graph` 工具（从 `code_metrics.rs` 拆分以降低圈复杂度）。

use std::path::Path;
use std::process::Command;

use super::MAX_OUTPUT_LINES;
use crate::tools::output_util;
use crate::tools::tool_param_types::{
    DependencyGraphArgs, DependencyGraphFormat, DependencyGraphKind,
};

#[path = "code_metrics_dep_graph_cargo.rs"]
mod dep_cargo;

struct ParsedDepGraph {
    format: &'static str,
    depth: usize,
    kind: &'static str,
}

fn parse_dependency_graph_args(args_json: &str) -> Result<ParsedDepGraph, String> {
    let v = crate::tools::parse_args_json(args_json)?;
    let args: DependencyGraphArgs = serde_json::from_value(v)
        .map_err(|e| format!("参数 JSON 与 dependency_graph 形状不一致: {e}"))?;
    let format = match args.format.unwrap_or_default() {
        DependencyGraphFormat::Mermaid => "mermaid",
        DependencyGraphFormat::Dot => "dot",
        DependencyGraphFormat::Tree => "tree",
    };
    let depth = args.depth.unwrap_or(1) as usize;
    let kind = match args.kind.unwrap_or_default() {
        DependencyGraphKind::Auto => "auto",
        DependencyGraphKind::Rust => "rust",
        DependencyGraphKind::Cargo => "cargo",
        DependencyGraphKind::Go => "go",
        DependencyGraphKind::Npm => "npm",
        DependencyGraphKind::Node => "node",
    };
    Ok(ParsedDepGraph {
        format,
        depth,
        kind,
    })
}

/// 依赖关系可视化（Cargo / Go / npm）。
pub fn dependency_graph(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let p = match parse_dependency_graph_args(args_json) {
        Ok(p) => p,
        Err(e) => return e,
    };
    dispatch_dependency_graph(workspace_root, p, max_output_len)
}

fn dispatch_dependency_graph(
    workspace_root: &Path,
    p: ParsedDepGraph,
    max_output_len: usize,
) -> String {
    if let Some(s) = try_cargo_dep_graph(workspace_root, p.kind, p.format, p.depth, max_output_len)
    {
        return s;
    }
    if let Some(s) = try_go_dep_graph(workspace_root, p.kind, p.format, max_output_len) {
        return s;
    }
    if let Some(s) = try_npm_dep_graph(workspace_root, p.kind, p.format, max_output_len) {
        return s;
    }
    "未检测到 Cargo.toml / go.mod / package.json，无法生成依赖图".to_string()
}

fn rust_kind_matches(kind: &str) -> bool {
    matches!(kind, "auto" | "rust" | "cargo")
}

fn npm_kind_matches(kind: &str) -> bool {
    matches!(kind, "auto" | "npm" | "node")
}

fn try_cargo_dep_graph(
    workspace_root: &Path,
    kind: &str,
    format: &str,
    depth: usize,
    max_output_len: usize,
) -> Option<String> {
    if !workspace_root.join("Cargo.toml").is_file() || !rust_kind_matches(kind) {
        return None;
    }
    Some(dep_cargo::cargo_dep_graph(
        workspace_root,
        format,
        depth,
        max_output_len,
    ))
}

fn try_go_dep_graph(
    workspace_root: &Path,
    kind: &str,
    format: &str,
    max_output_len: usize,
) -> Option<String> {
    if !workspace_root.join("go.mod").is_file() || !matches!(kind, "auto" | "go") {
        return None;
    }
    Some(go_dep_graph(workspace_root, format, max_output_len))
}

fn try_npm_dep_graph(
    workspace_root: &Path,
    kind: &str,
    format: &str,
    max_output_len: usize,
) -> Option<String> {
    let has_pkg = workspace_root.join("package.json").is_file()
        || workspace_root
            .join("frontend")
            .join("package.json")
            .is_file();
    if !has_pkg || !npm_kind_matches(kind) {
        return None;
    }
    Some(npm_dep_graph(workspace_root, format, max_output_len))
}

fn go_dep_graph(workspace_root: &Path, format: &str, max_output_len: usize) -> String {
    let output = match Command::new("go")
        .arg("list")
        .arg("-m")
        .arg("all")
        .current_dir(workspace_root)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format!("go list -m all 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mods: Vec<&str> = stdout.lines().filter(|l| !l.trim().is_empty()).collect();

    if format == "mermaid" || format == "dot" {
        let root = mods.first().copied().unwrap_or("module");
        let root_name = root.split_whitespace().next().unwrap_or("module");
        let root_id = sanitize_id(root_name);
        let mut lines = if format == "mermaid" {
            vec!["graph TD".to_string()]
        } else {
            vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()]
        };
        for m in mods.iter().skip(1).take(100) {
            let dep_name = m.split_whitespace().next().unwrap_or("");
            if dep_name.is_empty() {
                continue;
            }
            let dep_id = sanitize_id(dep_name);
            if format == "mermaid" {
                lines.push(format!("    {} --> {}", root_id, dep_id));
            } else {
                lines.push(format!("    \"{}\" -> \"{}\";", root_id, dep_id));
            }
        }
        if format == "dot" {
            lines.push("}".to_string());
        }
        let result = lines.join("\n");
        let label = if format == "mermaid" {
            "Mermaid"
        } else {
            "DOT"
        };
        return format!(
            "{} 依赖图（Go）：\n{}",
            label,
            output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
        );
    }

    format!(
        "Go 依赖列表（共 {} 个模块）：\n{}",
        mods.len(),
        output_util::truncate_output_lines(stdout.trim_end(), max_output_len, MAX_OUTPUT_LINES)
    )
}

fn npm_dep_graph(workspace_root: &Path, format: &str, max_output_len: usize) -> String {
    let dir = if workspace_root.join("package.json").is_file() {
        workspace_root.to_path_buf()
    } else {
        workspace_root.join("frontend")
    };

    let output = match Command::new("npm")
        .arg("ls")
        .arg("--depth=1")
        .arg("--json")
        .current_dir(&dir)
        .output()
    {
        Ok(o) => o,
        Err(e) => return format!("npm ls 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    if format == "raw" || (format != "mermaid" && format != "dot") {
        let text_out = match Command::new("npm")
            .arg("ls")
            .arg("--depth=1")
            .current_dir(&dir)
            .output()
        {
            Ok(o) => String::from_utf8_lossy(&o.stdout).to_string(),
            Err(_) => stdout.to_string(),
        };
        return format!(
            "npm 依赖树：\n{}",
            output_util::truncate_output_lines(
                text_out.trim_end(),
                max_output_len,
                MAX_OUTPUT_LINES
            )
        );
    }

    let parsed: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(_) => {
            return format!(
                "npm ls --json 解析失败，原始输出：\n{}",
                output_util::truncate_output_lines(
                    stdout.trim_end(),
                    max_output_len,
                    MAX_OUTPUT_LINES
                )
            );
        }
    };
    let root_name = parsed
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("project");
    let root_id = sanitize_id(root_name);
    let deps = parsed.get("dependencies").and_then(|d| d.as_object());

    let mut lines = if format == "mermaid" {
        vec!["graph TD".to_string()]
    } else {
        vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()]
    };

    if let Some(deps_map) = deps {
        for (dep_name, _) in deps_map.iter().take(100) {
            let dep_id = sanitize_id(dep_name);
            if format == "mermaid" {
                lines.push(format!("    {} --> {}", root_id, dep_id));
            } else {
                lines.push(format!("    \"{}\" -> \"{}\";", root_id, dep_id));
            }
        }
    }
    if format == "dot" {
        lines.push("}".to_string());
    }

    let result = lines.join("\n");
    let label = if format == "mermaid" {
        "Mermaid"
    } else {
        "DOT"
    };
    format!(
        "{} 依赖图（npm）：\n{}",
        label,
        output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}

fn sanitize_id(name: &str) -> String {
    name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
        .trim_start_matches('_')
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::sanitize_id;

    #[test]
    fn test_sanitize_id() {
        assert_eq!(sanitize_id("my-crate"), "my_crate");
        assert_eq!(sanitize_id("@scope/pkg"), "scope_pkg");
    }
}
