//! 首轮上下文注入：**工作区内** `cargo metadata` + `package.json` 的结构化摘要与 Mermaid workspace 依赖图。
//! 只读文件与子进程 `cargo metadata`（无网络、无任意 shell）；输出供模型理解 crate / npm 布局。

use std::collections::{BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use serde::Serialize;
use serde_json::json;

const BRIEF_VERSION: u32 = 1;
const MAX_MERMAID_NODES: usize = 48;
const MAX_MERMAID_EDGES: usize = 96;
const MAX_NPM_DEP_KEYS: usize = 64;

#[derive(Debug, Serialize)]
struct CargoBriefJson<'a> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    workspace_packages: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    workspace_dependency_edges: Vec<(String, String)>,
}

#[derive(Debug, Serialize)]
struct NpmBriefEntry {
    rel_path: String,
    name: String,
    dependency_names: Vec<String>,
}

/// 生成 Markdown（UTF-8）；`max_chars == 0` 时返回空串。
pub fn build_project_dependency_brief_markdown(workspace_root: &Path, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let cargo_block = cargo_metadata_brief(workspace_root);
    let npm_entries = npm_package_summaries(workspace_root);

    let cargo_json = CargoBriefJson {
        ok: cargo_block.error.is_none(),
        error: cargo_block.error.as_deref(),
        workspace_packages: cargo_block.workspace_packages.clone(),
        workspace_dependency_edges: cargo_block.edges.clone(),
    };

    let envelope = json!({
        "crabmate_project_dependency_brief_version": BRIEF_VERSION,
        "cargo": cargo_json,
        "npm": npm_entries,
    });

    let json_pretty = serde_json::to_string_pretty(&envelope).unwrap_or_else(|_| "{}".to_string());
    let mermaid = mermaid_workspace_graph(&cargo_block.edges);
    let npm_lines = format_npm_section(&npm_entries);

    let mut sections: Vec<String> = Vec::new();
    sections.push(format!(
        "## CrabMate 项目依赖与结构摘要（自动生成 v{}）\n",
        BRIEF_VERSION
    ));
    sections.push(
        "_由 `cargo metadata` 与 `package.json` 只读生成；仅反映 workspace 内 crate 关系与 npm 声明依赖名，不含版本号与密钥。_\n"
            .to_string(),
    );
    sections.push("### 结构化 JSON\n".to_string());
    sections.push(format!("```json\n{json_pretty}\n```\n"));
    sections.push("### Workspace 依赖图（Mermaid，成员 crate 之间）\n".to_string());
    sections.push("（边来自 `cargo metadata` 的 resolve 图；过大时截断。）\n".to_string());
    if mermaid.trim().is_empty() {
        sections.push("_（无可绘制的 workspace 内依赖边，或未检测到 Cargo 工程。）_\n".to_string());
    } else {
        sections.push(format!("```mermaid\n{mermaid}\n```\n"));
    }
    if !npm_lines.is_empty() {
        sections.push("### npm / 前端（package.json 节选）\n".to_string());
        sections.push(npm_lines);
    }

    let mut out = sections.join("\n");
    if out.chars().count() > max_chars {
        let truncated: String = out.chars().take(max_chars.saturating_sub(80)).collect();
        out = format!(
            "{truncated}\n\n[... 依赖摘要过长，已按 project_dependency_brief_inject_max_chars 截断 ...]"
        );
    }
    out
}

struct CargoBlock {
    workspace_packages: Vec<String>,
    edges: Vec<(String, String)>,
    error: Option<String>,
}

fn cargo_metadata_brief(root: &Path) -> CargoBlock {
    if !root.join("Cargo.toml").is_file() {
        return CargoBlock {
            workspace_packages: Vec::new(),
            edges: Vec::new(),
            error: None,
        };
    }

    let output = match Command::new("cargo")
        .args(["metadata", "--format-version", "1"])
        .current_dir(root)
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            return CargoBlock {
                workspace_packages: Vec::new(),
                edges: Vec::new(),
                error: Some(format!("无法执行 cargo metadata: {e}")),
            };
        }
    };
    if !output.status.success() {
        return CargoBlock {
            workspace_packages: Vec::new(),
            edges: Vec::new(),
            error: Some("`cargo metadata` 退出非零（已跳过图与包列表）".to_string()),
        };
    }

    let val: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => {
            return CargoBlock {
                workspace_packages: Vec::new(),
                edges: Vec::new(),
                error: Some("cargo metadata 输出 JSON 解析失败".to_string()),
            };
        }
    };

    let workspace_ids: HashSet<String> = val
        .get("workspace_members")
        .and_then(|w| w.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default();

    let packages = match val.get("packages").and_then(|p| p.as_array()) {
        Some(a) => a,
        None => {
            return CargoBlock {
                workspace_packages: Vec::new(),
                edges: Vec::new(),
                error: Some("metadata 缺少 packages 数组".to_string()),
            };
        }
    };

    let mut id_to_name: HashMap<String, String> = HashMap::new();
    for p in packages {
        let Some(id) = p.get("id").and_then(|i| i.as_str()) else {
            continue;
        };
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        id_to_name.insert(id.to_string(), name.to_string());
    }

    let mut ws_names: Vec<String> = workspace_ids
        .iter()
        .filter_map(|id| id_to_name.get(id).cloned())
        .collect();
    ws_names.sort();
    ws_names.dedup();

    let mut edge_set: BTreeSet<(String, String)> = BTreeSet::new();
    if let Some(nodes) = val
        .get("resolve")
        .and_then(|r| r.get("nodes"))
        .and_then(|n| n.as_array())
    {
        for n in nodes {
            let Some(from_id) = n.get("id").and_then(|i| i.as_str()) else {
                continue;
            };
            if !workspace_ids.contains(from_id) {
                continue;
            }
            let Some(from_name) = id_to_name.get(from_id) else {
                continue;
            };
            let Some(deps) = n.get("deps").and_then(|d| d.as_array()) else {
                continue;
            };
            for d in deps {
                let Some(to_id) = d.get("pkg").and_then(|p| p.as_str()) else {
                    continue;
                };
                if !workspace_ids.contains(to_id) {
                    continue;
                }
                let Some(to_name) = id_to_name.get(to_id) else {
                    continue;
                };
                if from_name == to_name {
                    continue;
                }
                edge_set.insert((from_name.clone(), to_name.clone()));
            }
        }
    }

    let edges: Vec<(String, String)> = edge_set.into_iter().collect();

    CargoBlock {
        workspace_packages: ws_names,
        edges,
        error: None,
    }
}

fn mermaid_workspace_graph(edges: &[(String, String)]) -> String {
    if edges.is_empty() {
        return String::new();
    }

    let mut node_ids: BTreeSet<String> = BTreeSet::new();
    for (a, b) in edges.iter().take(MAX_MERMAID_EDGES) {
        node_ids.insert(a.clone());
        node_ids.insert(b.clone());
        if node_ids.len() >= MAX_MERMAID_NODES {
            break;
        }
    }

    let mut lines: Vec<String> = vec!["flowchart LR".to_string()];
    for name in &node_ids {
        let nid = mermaid_safe_id(name);
        let esc = mermaid_escape_label(name);
        lines.push(format!("  {nid}[\"{esc}\"]"));
    }

    let mut n_edges = 0usize;
    for (from, to) in edges {
        if n_edges >= MAX_MERMAID_EDGES {
            lines.push(format!(
                "  note_truncate[\"… 尚有边未展示（上限 {MAX_MERMAID_EDGES}）…\"]"
            ));
            break;
        }
        if !node_ids.contains(from) || !node_ids.contains(to) {
            continue;
        }
        let a = mermaid_safe_id(from);
        let b = mermaid_safe_id(to);
        lines.push(format!("  {a} --> {b}"));
        n_edges += 1;
    }

    lines.join("\n")
}

fn mermaid_safe_id(name: &str) -> String {
    let mut s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect();
    if s.is_empty() {
        s = "pkg".to_string();
    }
    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        s.insert(0, 'n');
    }
    s
}

fn mermaid_escape_label(name: &str) -> String {
    name.replace('"', "'")
}

fn npm_package_summaries(root: &Path) -> Vec<NpmBriefEntry> {
    let mut out: Vec<NpmBriefEntry> = Vec::new();
    let candidates = [root.join("package.json")];
    for path in candidates {
        if !path.is_file() {
            continue;
        }
        if let Some(e) = read_one_package_json(&path, root) {
            out.push(e);
        }
    }
    out
}

fn read_one_package_json(path: &Path, root: &Path) -> Option<NpmBriefEntry> {
    let raw = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = v
        .get("name")
        .and_then(|n| n.as_str())
        .unwrap_or("(未命名)")
        .to_string();
    let mut dep_keys: Vec<String> = Vec::new();
    for key in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(o) = v.get(key).and_then(|x| x.as_object()) {
            dep_keys.extend(o.keys().cloned());
        }
    }
    dep_keys.sort();
    dep_keys.dedup();
    dep_keys.truncate(MAX_NPM_DEP_KEYS);

    let rel = path
        .strip_prefix(root)
        .ok()
        .map(|p| p.display().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "package.json".to_string());

    Some(NpmBriefEntry {
        rel_path: rel,
        name,
        dependency_names: dep_keys,
    })
}

fn format_npm_section(entries: &[NpmBriefEntry]) -> String {
    let mut lines = Vec::new();
    for e in entries {
        let preview = if e.dependency_names.is_empty() {
            "（无 dependencies 类字段或为空）".to_string()
        } else {
            e.dependency_names.join("、")
        };
        lines.push(format!(
            "- `{}`：npm 包 **{}** — 依赖名（节选，最多 {} 个）：{}\n",
            e.rel_path, e.name, MAX_NPM_DEP_KEYS, preview
        ));
    }
    lines.join("")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn empty_without_cargo_toml() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_dep_brief_none_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let md = build_project_dependency_brief_markdown(&dir, 8000);
        assert!(md.contains("结构化 JSON"));
        assert!(!md.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn includes_npm_when_package_json_present() {
        let dir =
            std::env::temp_dir().join(format!("crabmate_dep_brief_npm_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let mut f = std::fs::File::create(dir.join("package.json")).unwrap();
        writeln!(
            f,
            r#"{{"name":"demo-pkg","dependencies":{{"left-pad":"^1.0.0"}}}}"#
        )
        .unwrap();
        let md = build_project_dependency_brief_markdown(&dir, 8000);
        assert!(md.contains("demo-pkg"));
        assert!(md.contains("left-pad"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
