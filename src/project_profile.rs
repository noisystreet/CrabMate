//! 工作区「项目画像」：只读扫描清单文件、目录结构与 tokei 语言占比，供 Web 侧栏与首轮对话注入。
//! 不执行任意用户命令；可选 `cargo metadata --no-deps` 仅用于依赖数量（失败则省略）。

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use tokei::LanguageType;

use crate::cargo_metadata::cargo_metadata_command;
use crate::config::AgentConfig;

/// 与 `tools/code_metrics.rs` 中 `code_stats` 排除目录一致，避免统计噪声。
const EXCLUDED_DIRS: &[&str] = &["target", "node_modules", "vendor", "dist", "build", ".git"];

const PROFILE_MARKDOWN_VERSION: u32 = 1;

/// 生成 Markdown 正文（UTF-8）；`max_chars` 为 0 时返回空串。
pub fn build_project_profile_markdown(workspace_root: &Path, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut sections: Vec<String> = Vec::new();
    sections.push(format!(
        "## CrabMate 项目画像（自动生成 v{}）\n",
        PROFILE_MARKDOWN_VERSION
    ));
    sections.push("_由服务端只读扫描生成，不含密钥；切换工作区或点击刷新可更新。_\n".to_string());

    if let Some(block) = section_layout(workspace_root) {
        sections.push(block);
    }
    if let Some(block) = section_top_dirs(workspace_root) {
        sections.push(block);
    }
    if let Some(block) = section_code_stats(workspace_root) {
        sections.push(block);
    }
    if let Some(block) = section_cargo_metadata(workspace_root) {
        sections.push(block);
    }
    if let Some(block) = section_package_json(workspace_root) {
        sections.push(block);
    }
    if let Some(block) = section_python_hints(workspace_root) {
        sections.push(block);
    }

    sections.push("\n### 约定与备忘\n".to_string());
    sections.push(
        "详细约定请写在 `.crabmate/agent_memory.md`（若已启用 `agent_memory_file`）或由团队在仓库文档中维护。\n"
            .to_string(),
    );

    let mut out = sections.join("\n");
    if out.chars().count() > max_chars {
        let truncated: String = out.chars().take(max_chars).collect();
        out = format!(
            "{truncated}\n\n[... 项目画像过长，已按 project_profile_inject_max_chars 截断 ...]"
        );
    }
    out
}

fn section_layout(root: &Path) -> Option<String> {
    let cargo = root.join("Cargo.toml");
    if !cargo.is_file() {
        return None;
    }
    let raw = fs::read_to_string(&cargo).ok()?;
    let v: toml::Value = toml::from_str(&raw).ok()?;
    let mut lines = vec!["### 工程类型\n".to_string()];
    if let Some(ws) = v.get("workspace") {
        lines.push("**Rust workspace**\n".to_string());
        if let Some(members) = ws.get("members").and_then(|m| m.as_array()) {
            let mut names: Vec<String> = Vec::new();
            for m in members.iter().filter_map(|x| x.as_str()) {
                let trimmed = m.trim_end_matches(['/', '\\']);
                let name = trimmed
                    .rsplit(['/', '\\'])
                    .next()
                    .unwrap_or(trimmed)
                    .to_string();
                if !name.is_empty() && name != "*" {
                    names.push(name);
                }
            }
            names.sort();
            names.dedup();
            if names.is_empty() {
                lines.push("- （workspace 未列出 members 或为空）\n".to_string());
            } else {
                let show: Vec<_> = names.iter().take(16).cloned().collect();
                lines.push(format!("- 成员 crate 目录（节选）：{}\n", show.join("、")));
                if names.len() > 16 {
                    lines.push(format!("- … 共 {} 个 member 条目\n", names.len()));
                }
            }
        } else {
            lines.push("- （未解析到 `[workspace].members`）\n".to_string());
        }
    } else if v.get("package").is_some() {
        let name = v
            .get("package")
            .and_then(|p| p.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("unknown");
        lines.push(format!("**Rust 单包（cargo）**\n- 包名：`{name}`\n"));
    } else {
        lines.push("**检测到 Cargo.toml（结构未识别为 package 或 workspace）**\n".to_string());
    }
    Some(lines.join(""))
}

fn section_top_dirs(root: &Path) -> Option<String> {
    let mut entries: Vec<String> = Vec::new();
    let rd = fs::read_dir(root).ok()?;
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let ft = e.file_type().ok()?;
        let label = if ft.is_dir() {
            format!("{name}/")
        } else {
            name
        };
        entries.push(label);
    }
    entries.sort();
    if entries.is_empty() {
        return None;
    }
    let mut out = String::from("### 顶层条目（节选）\n");
    for e in entries.iter().take(24) {
        out.push_str(&format!("- `{e}`\n"));
    }
    if entries.len() > 24 {
        out.push_str(&format!("- … 共 {} 项\n", entries.len()));
    }
    Some(out)
}

fn section_code_stats(root: &Path) -> Option<String> {
    let config = tokei::Config::default();
    let mut languages = tokei::Languages::new();
    let path_str = root.to_string_lossy().to_string();
    let paths = &[path_str];
    let excluded: Vec<&str> = EXCLUDED_DIRS.to_vec();
    languages.get_statistics(paths, &excluded, &config);

    let mut sorted: Vec<_> = languages
        .iter()
        .filter(|(_, lang)| lang.code > 0 || lang.comments > 0 || lang.blanks > 0)
        .collect();
    if sorted.is_empty() {
        return Some("### 语言与规模（tokei）\n- （未识别到源码文件）\n".to_string());
    }
    sorted.sort_by(|a, b| b.1.code.cmp(&a.1.code));

    let total_code: usize = sorted.iter().map(|(_, l)| l.code).sum();
    let total_files: usize = sorted.iter().map(|(_, l)| l.reports.len()).sum();

    let mut out = String::from("### 语言与规模（tokei，已排除 target/node_modules 等）\n");
    out.push_str(&format!(
        "- 估算代码行数：**{}**（{} 个文件）\n",
        total_code, total_files
    ));
    let mut labels: Vec<String> = Vec::new();
    for (lang_type, lang) in sorted.iter().take(8) {
        let pct = if total_code > 0 {
            (lang.code * 100) / total_code
        } else {
            0
        };
        labels.push(format!("{} {}%", language_label(**lang_type), pct));
    }
    out.push_str(&format!(
        "- 主要语言占比（按代码行）：{}\n",
        labels.join("，")
    ));
    Some(out)
}

fn language_label(t: LanguageType) -> String {
    match t {
        LanguageType::Rust => "Rust".to_string(),
        LanguageType::TypeScript => "TypeScript".to_string(),
        LanguageType::JavaScript => "JavaScript".to_string(),
        LanguageType::Python => "Python".to_string(),
        LanguageType::Go => "Go".to_string(),
        LanguageType::Cpp => "C++".to_string(),
        LanguageType::C => "C".to_string(),
        LanguageType::Css => "CSS".to_string(),
        LanguageType::Html => "HTML".to_string(),
        LanguageType::Json => "JSON".to_string(),
        LanguageType::Markdown => "Markdown".to_string(),
        LanguageType::Toml => "TOML".to_string(),
        _ => format!("{t}"),
    }
}

fn section_cargo_metadata(root: &Path) -> Option<String> {
    if !root.join("Cargo.toml").is_file() {
        return None;
    }
    let output = match cargo_metadata_command(root, true, 1).output() {
        Ok(o) => o,
        Err(_) => {
            return Some(
                "### Cargo 依赖（metadata）\n- （无法执行 `cargo metadata`，已跳过）\n".to_string(),
            );
        }
    };
    if !output.status.success() {
        return Some(
            "### Cargo 依赖（metadata）\n- （`cargo metadata --no-deps` 失败，已跳过）\n"
                .to_string(),
        );
    }
    let val: serde_json::Value = match serde_json::from_slice(&output.stdout) {
        Ok(v) => v,
        Err(_) => {
            return Some(
                "### Cargo 依赖（metadata）\n- （输出 JSON 解析失败，已跳过）\n".to_string(),
            );
        }
    };
    let Some(packages) = val.get("packages").and_then(|p| p.as_array()) else {
        return Some("### Cargo 依赖（metadata）\n- （无 packages 字段，已跳过）\n".to_string());
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

    let mut ws_names: Vec<String> = Vec::new();
    for p in packages {
        let Some(id) = p.get("id").and_then(|i| i.as_str()) else {
            continue;
        };
        if !workspace_ids.contains(id) {
            continue;
        }
        let Some(name) = p.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        ws_names.push(name.to_string());
    }
    ws_names.sort();
    ws_names.dedup();

    let dep_count = val
        .get("resolve")
        .and_then(|r| r.get("root"))
        .and_then(|rid| rid.as_str())
        .and_then(|root_id| {
            packages
                .iter()
                .find(|p| p.get("id").and_then(|i| i.as_str()) == Some(root_id))
        })
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
        .map(|a| a.len())
        .unwrap_or(0);

    let mut out = String::from("### Cargo 依赖（metadata，--no-deps）\n");
    if !ws_names.is_empty() {
        let show: Vec<_> = ws_names.iter().take(20).cloned().collect();
        out.push_str(&format!("- workspace 包（节选）：{}\n", show.join("、")));
        if ws_names.len() > 20 {
            out.push_str(&format!("- … 共 {} 个包\n", ws_names.len()));
        }
    }
    out.push_str(&format!(
        "- 根包声明的直接依赖数：**{dep_count}**（含 target 与 feature 条目，仅供参考）\n"
    ));
    Some(out)
}

fn read_package_json_summary(path: &Path, root: &Path) -> Option<String> {
    let raw = fs::read_to_string(path).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let name = v.get("name").and_then(|n| n.as_str()).unwrap_or("(未命名)");
    let mut dep_n = 0usize;
    for key in [
        "dependencies",
        "devDependencies",
        "peerDependencies",
        "optionalDependencies",
    ] {
        if let Some(o) = v.get(key).and_then(|x| x.as_object()) {
            dep_n += o.len();
        }
    }
    let label = path
        .strip_prefix(root)
        .ok()
        .map(|p| p.display().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "package.json".to_string());
    Some(format!(
        "- `{label}`：npm 包 **{name}**，声明依赖条目约 **{dep_n}** 个\n"
    ))
}

fn section_package_json(root: &Path) -> Option<String> {
    let mut blocks: Vec<String> = Vec::new();
    let root_pj = root.join("package.json");
    if root_pj.is_file()
        && let Some(line) = read_package_json_summary(&root_pj, root)
    {
        blocks.push(line);
    }
    if blocks.is_empty() {
        return None;
    }
    let mut out = String::from("### Node / 前端（package.json）\n");
    out.push_str(&blocks.join(""));
    Some(out)
}

fn section_python_hints(root: &Path) -> Option<String> {
    let mut hints: Vec<String> = Vec::new();
    if root.join("pyproject.toml").is_file() {
        hints.push("- 存在 `pyproject.toml`（Python 项目）\n".to_string());
    }
    if root.join("requirements.txt").is_file() {
        hints.push("- 存在 `requirements.txt`\n".to_string());
    }
    if root.join("uv.lock").is_file() {
        hints.push("- 存在 `uv.lock`（uv）\n".to_string());
    }
    if hints.is_empty() {
        return None;
    }
    let mut out = String::from("### Python（线索）\n");
    out.push_str(&hints.join(""));
    Some(out)
}

/// Web / CLI 首轮：合并备忘（可选预载）、项目画像、`cargo metadata`+npm 依赖摘要；全无则 `None`。
pub fn build_first_turn_user_context_markdown(
    workspace_root: &Path,
    cfg: &AgentConfig,
    memory_preloaded: Option<String>,
) -> Option<String> {
    let memory_snippet = memory_preloaded.or_else(|| {
        if cfg.agent_memory_file_enabled {
            crate::agent_memory::load_memory_snippet(
                workspace_root,
                cfg.agent_memory_file.as_str(),
                cfg.agent_memory_file_max_chars,
            )
        } else {
            None
        }
    });
    let living_snippet = if cfg.living_docs_inject_enabled && cfg.living_docs_inject_max_chars > 0 {
        crate::living_docs::load_living_docs_snippet(
            workspace_root,
            cfg.living_docs_relative_dir.as_str(),
            cfg.living_docs_inject_max_chars,
            cfg.living_docs_file_max_each_chars,
        )
    } else {
        None
    };
    let want_profile =
        cfg.project_profile_inject_enabled && cfg.project_profile_inject_max_chars > 0;
    let want_dep = cfg.project_dependency_brief_inject_enabled
        && cfg.project_dependency_brief_inject_max_chars > 0;
    if !want_profile && !want_dep && memory_snippet.is_none() && living_snippet.is_none() {
        return None;
    }
    let profile_md = if want_profile {
        build_project_profile_markdown(workspace_root, cfg.project_profile_inject_max_chars)
    } else {
        String::new()
    };
    let dep_md = if want_dep {
        crate::project_dependency_brief::build_project_dependency_brief_markdown(
            workspace_root,
            cfg.project_dependency_brief_inject_max_chars,
        )
    } else {
        String::new()
    };
    merge_first_turn_injections(
        living_snippet.as_deref(),
        memory_snippet.as_deref(),
        profile_md.as_str(),
        dep_md.as_str(),
    )
}

/// 合并活文档摘要、备忘、项目画像、依赖结构摘要为一条首轮 `user` 正文；任一段为空则跳过。
pub fn merge_first_turn_injections(
    living_docs_snippet: Option<&str>,
    memory_snippet: Option<&str>,
    profile_markdown: &str,
    dependency_brief_markdown: &str,
) -> Option<String> {
    let living = living_docs_snippet.map(str::trim).filter(|s| !s.is_empty());
    let mem = memory_snippet.map(str::trim).filter(|s| !s.is_empty());
    let prof = profile_markdown.trim();
    let dep = dependency_brief_markdown.trim();
    let mut parts: Vec<String> = Vec::new();
    if let Some(m) = living {
        parts.push(m.to_string());
    }
    if let Some(m) = mem {
        parts.push(m.to_string());
    }
    if !prof.is_empty() {
        parts.push(format!(
            "[项目画像（工作区内自动生成，仅只读扫描）]\n{prof}"
        ));
    }
    if !dep.is_empty() {
        parts.push(format!(
            "[项目依赖与结构摘要（cargo metadata + package.json，仅只读）]\n{dep}"
        ));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n---\n\n"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn profile_includes_rust_and_counts() {
        let root = std::env::temp_dir().join(format!(
            "crabmate_project_profile_test_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut f = std::fs::File::create(root.join("Cargo.toml")).unwrap();
        writeln!(
            f,
            r#"
[package]
name = "demo_prof"
version = "0.1.0"
edition = "2021"
"#
        )
        .unwrap();
        std::fs::create_dir_all(root.join("src")).unwrap();
        let mut main_rs = std::fs::File::create(root.join("src/lib.rs")).unwrap();
        writeln!(main_rs, "pub fn f() {{}}").unwrap();

        let md = build_project_profile_markdown(&root, 20_000);
        assert!(md.contains("demo_prof"));
        assert!(md.contains("tokei"));
        assert!(md.contains("Rust"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn merge_first_turn_three_parts() {
        let got =
            merge_first_turn_injections(None, Some("memo line"), "# Title\nbody", "## Dep\nx")
                .expect("some");
        assert!(got.contains("memo line"));
        assert!(got.contains("项目画像"));
        assert!(got.contains("依赖与结构摘要"));
        assert!(got.contains("# Title"));
        assert!(got.contains("## Dep"));
    }
}
