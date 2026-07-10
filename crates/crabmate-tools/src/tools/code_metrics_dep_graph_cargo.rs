//! Cargo `cargo tree` 输出 → Mermaid/DOT（从 `code_metrics_dependency_graph.rs` 再拆分，避免工具链对含反引号宏串的解析误判）。

use std::path::Path;
use std::process::Command;

use super::MAX_OUTPUT_LINES;
use crate::tools::output_util;

pub(super) fn cargo_dep_graph(
    workspace_root: &Path,
    format: &str,
    depth: usize,
    max_output_len: usize,
) -> String {
    let mut cmd = Command::new("cargo");
    cmd.arg("tree")
        .arg("--depth")
        .arg(depth.min(10).to_string())
        .arg("--no-dedupe")
        .current_dir(workspace_root);
    let output = match cmd.output() {
        Ok(o) => o,
        Err(e) => return format!("cargo tree 执行失败：{}", e),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);

    if format == "mermaid" {
        return cargo_tree_to_mermaid(&stdout, max_output_len);
    }
    if format == "dot" {
        return cargo_tree_to_dot(&stdout, max_output_len);
    }
    format!(
        "cargo tree (depth={})：\n{}",
        depth,
        output_util::truncate_output_lines(stdout.trim_end(), max_output_len, MAX_OUTPUT_LINES)
    )
}

fn is_tree_decoration(c: char) -> bool {
    matches!(c, ' ' | '│' | '├' | '└' | '─' | '|')
}

fn pkg_safe_id(name: &str) -> String {
    name.replace(|c: char| !c.is_alphanumeric() && c != '_', "_")
}

fn cargo_tree_to_mermaid(tree_output: &str, max_output_len: usize) -> String {
    let mut lines = vec!["graph TD".to_string()];
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();

    for line in tree_output.lines() {
        let trimmed = line.trim_start_matches(is_tree_decoration);
        let indent = line.len() - trimmed.len();
        let level = indent / 4;
        let pkg_name = trimmed.split_whitespace().next().unwrap_or("").to_string();
        if pkg_name.is_empty() || pkg_name == "(*)" {
            continue;
        }
        let safe_id = pkg_safe_id(&pkg_name);

        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        if let Some((_, parent)) = stack.last() {
            let edge = (parent.clone(), safe_id.clone());
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        stack.push((level, safe_id));
    }

    for (from, to) in &edges {
        lines.push(format!("    {} --> {}", from, to));
    }

    let result = lines.join("\n");
    let fence: String = ['`'; 3].iter().collect();
    format!(
        "Mermaid 依赖图（Cargo）：\n{fence}mermaid\n{body}\n{fence}",
        fence = fence,
        body = output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}

fn cargo_tree_to_dot(tree_output: &str, max_output_len: usize) -> String {
    let mut lines = vec!["digraph deps {".to_string(), "    rankdir=LR;".to_string()];
    let mut edges: Vec<(String, String)> = Vec::new();
    let mut stack: Vec<(usize, String)> = Vec::new();

    for line in tree_output.lines() {
        let trimmed = line.trim_start_matches(is_tree_decoration);
        let indent = line.len() - trimmed.len();
        let level = indent / 4;
        let pkg_name = trimmed.split_whitespace().next().unwrap_or("").to_string();
        if pkg_name.is_empty() || pkg_name == "(*)" {
            continue;
        }
        let safe_id = pkg_safe_id(&pkg_name);

        while stack.last().is_some_and(|(l, _)| *l >= level) {
            stack.pop();
        }
        if let Some((_, parent)) = stack.last() {
            let edge = (parent.clone(), safe_id.clone());
            if !edges.contains(&edge) {
                edges.push(edge);
            }
        }
        stack.push((level, safe_id));
    }

    for (from, to) in &edges {
        lines.push(format!("    \"{}\" -> \"{}\";", from, to));
    }
    lines.push("}".to_string());

    let result = lines.join("\n");
    format!(
        "DOT 依赖图（Cargo）：\n{}",
        output_util::truncate_output_lines(&result, max_output_len, MAX_OUTPUT_LINES)
    )
}
